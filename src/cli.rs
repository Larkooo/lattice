use std::path::Path;
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Result};
use clap::Subcommand;

use crate::{agents, config, git, tmux};

#[derive(Subcommand, Debug)]
pub enum CliCommand {
    /// List all running agent instances (JSON)
    List,
    /// Spawn a new worker instance, prints session name
    Spawn {
        /// Agent to spawn (e.g. "claude", "codex")
        agent: String,
        /// Working directory
        #[arg(long, short)]
        dir: Option<String>,
    },
    /// Send a prompt to an instance
    Send {
        /// Session name
        session: String,
        /// The prompt message to send
        message: String,
    },
    /// Block until an instance completes, then print its output
    Watch {
        /// Session name
        session: String,
        /// Poll interval in seconds
        #[arg(long, default_value = "2")]
        interval: u64,
    },
    /// Get instance status (JSON): title, done, branch
    Status {
        /// Session name
        session: String,
    },
    /// Read the last N lines of instance output
    Read {
        /// Session name
        session: String,
        /// Number of lines to read
        #[arg(long, short, default_value = "30")]
        lines: u32,
    },
}

pub fn run_command(command: CliCommand, cfg: &config::AppConfig) -> Result<()> {
    match command {
        CliCommand::List => cmd_list(cfg),
        CliCommand::Spawn { agent, dir } => cmd_spawn(cfg, &agent, dir.as_deref()),
        CliCommand::Send { session, message } => cmd_send(&session, &message),
        CliCommand::Watch { session, interval } => cmd_watch(&session, interval),
        CliCommand::Status { session } => cmd_status(&session),
        CliCommand::Read { session, lines } => cmd_read(&session, lines),
    }
}

fn cmd_list(cfg: &config::AppConfig) -> Result<()> {
    let available = agents::detect_available_agents(&cfg.custom_agents);
    let sessions = tmux::list_sessions()?;

    let mut instances = Vec::new();
    for session in sessions {
        let agent = match agents::classify_agent_from_session(
            &session.name,
            &session.current_command,
            &available,
        ) {
            Some(a) => a,
            None => continue,
        };

        let title = agents::read_title_file(&session.name);
        let done = agents::is_done(&session.name);
        let branch = if !session.pane_current_path.is_empty() {
            git::current_branch(Path::new(&session.pane_current_path))
        } else {
            String::new()
        };

        instances.push(serde_json::json!({
            "session": session.name,
            "agent": agent.id,
            "title": title,
            "done": done,
            "branch": branch,
            "path": session.pane_current_path,
        }));
    }

    println!("{}", serde_json::to_string_pretty(&instances)?);
    Ok(())
}

fn cmd_spawn(cfg: &config::AppConfig, agent_id: &str, dir: Option<&str>) -> Result<()> {
    let available = agents::detect_available_agents(&cfg.custom_agents);
    let agent = available
        .iter()
        .find(|a| a.id == agent_id)
        .ok_or_else(|| anyhow!("agent '{}' not found (available: {})",
            agent_id,
            available.iter().map(|a| a.id.as_str()).collect::<Vec<_>>().join(", ")))?;

    let working_dir = dir
        .map(|d| {
            let expanded = if d.starts_with('~') {
                let home = std::env::var("HOME").unwrap_or_default();
                d.replacen('~', &home, 1)
            } else {
                d.to_owned()
            };
            expanded
        })
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| "/".into()).to_string_lossy().to_string());

    // Create worktree if enabled
    let (final_dir, repo_root) =
        if cfg.git_worktrees && git::is_git_repo(Path::new(&working_dir)) {
            match git::create_worktree(Path::new(&working_dir)) {
                Ok((wt_path, root)) => (wt_path.to_string_lossy().to_string(), Some(root)),
                Err(_) => (working_dir.clone(), None),
            }
        } else {
            (working_dir.clone(), None)
        };

    // Install hooks
    if cfg.lattice_coauthor {
        let _ = git::install_lattice_coauthor_hook(Path::new(&final_dir));
    } else if cfg.strip_coauthor {
        let _ = git::install_strip_coauthor_hook(Path::new(&final_dir));
    }

    // Copy build artifacts
    if let Some(ref root) = repo_root {
        git::copy_build_artifacts(root, Path::new(&final_dir));
    }

    let session_name = agents::build_managed_session_name(&agent.id);
    let title_enabled = cfg.title_injection_enabled;
    let bypass_enabled = config::is_bypass_enabled(cfg, &agent.id);

    // Workers spawned via CLI never get channels — those belong to the router
    let launch_cmd =
        agents::build_launch_command(agent, &session_name, title_enabled, bypass_enabled, &[]);

    let startup_cmds = config::get_startup_commands(cfg, &final_dir);
    let full_cmd = if startup_cmds.is_empty() {
        launch_cmd.clone()
    } else {
        let mut parts = startup_cmds.clone();
        parts.push(launch_cmd.clone());
        parts.join(" && ")
    };

    tmux::create_session(&session_name, &final_dir, &full_cmd)?;

    if title_enabled && agents::needs_title_injection(agent) {
        let msg = agents::build_title_injection(&session_name);
        let _ = tmux::send_keys_delayed(&session_name, &msg, cfg.title_injection_delay);
    }

    // Print session name so the caller (router) can use it
    println!("{session_name}");
    Ok(())
}

fn cmd_send(session: &str, message: &str) -> Result<()> {
    tmux::send_keys(session, message)?;
    println!("sent");
    Ok(())
}

fn cmd_watch(session: &str, interval_secs: u64) -> Result<()> {
    let interval = Duration::from_secs(interval_secs.max(1));

    loop {
        if agents::is_done(session) {
            // Print the final output
            return cmd_read(session, 50);
        }

        // Check session still exists
        let sessions = tmux::list_sessions()?;
        if !sessions.iter().any(|s| s.name == session) {
            return Err(anyhow!("session '{}' no longer exists", session));
        }

        thread::sleep(interval);
    }
}

fn cmd_status(session: &str) -> Result<()> {
    let title = agents::read_title_file(session);
    let done = agents::is_done(session);

    // Get path from tmux session
    let sessions = tmux::list_sessions()?;
    let tmux_session = sessions.iter().find(|s| s.name == session);

    let (path, branch) = match tmux_session {
        Some(s) if !s.pane_current_path.is_empty() => {
            let branch = git::current_branch(Path::new(&s.pane_current_path));
            (s.pane_current_path.clone(), branch)
        }
        _ => (String::new(), String::new()),
    };

    let status = serde_json::json!({
        "session": session,
        "title": title,
        "done": done,
        "branch": branch,
        "path": path,
    });

    println!("{}", serde_json::to_string_pretty(&status)?);
    Ok(())
}

fn cmd_read(session: &str, lines: u32) -> Result<()> {
    let target = format!("{session}:0.0");
    let output = std::process::Command::new("tmux")
        .arg("capture-pane")
        .arg("-p")
        .arg("-t")
        .arg(&target)
        .arg("-S")
        .arg(format!("-{lines}"))
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("failed to read session '{}': {}", session, stderr.trim()));
    }

    let text = String::from_utf8_lossy(&output.stdout);
    // Trim trailing empty lines
    let trimmed: Vec<&str> = text.lines().collect();
    let end = trimmed.iter().rposition(|l| !l.trim().is_empty()).map(|i| i + 1).unwrap_or(0);
    for line in &trimmed[..end] {
        println!("{line}");
    }

    Ok(())
}
