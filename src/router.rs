use anyhow::Result;

use crate::{agents, config, tmux};

pub const ROUTER_SESSION_NAME: &str = "lattice_router";

const ROUTER_PROMPT_TEMPLATE: &str = include_str!("../prompts/router.md");

pub fn router_session_name() -> &'static str {
    ROUTER_SESSION_NAME
}

/// Check if the router tmux session is alive.
pub fn is_router_alive() -> bool {
    match tmux::list_sessions() {
        Ok(sessions) => sessions.iter().any(|s| s.name == ROUTER_SESSION_NAME),
        Err(_) => false,
    }
}

/// Build the router system prompt with lattice CLI usage instructions.
pub fn build_router_prompt() -> String {
    ROUTER_PROMPT_TEMPLATE.to_owned()
}

/// Spawn the router instance. Returns the session name on success.
pub fn spawn_router(cfg: &config::AppConfig) -> Result<String> {
    let router_cfg = match &cfg.router {
        Some(r) if r.enabled => r,
        _ => return Err(anyhow::anyhow!("router not enabled in config")),
    };

    let available = agents::detect_available_agents(&cfg.custom_agents);
    let agent = available
        .iter()
        .find(|a| a.id == router_cfg.agent)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "router agent '{}' not found (available: {})",
                router_cfg.agent,
                available.iter().map(|a| a.id.as_str()).collect::<Vec<_>>().join(", ")
            )
        })?;

    let session_name = ROUTER_SESSION_NAME.to_owned();

    // Build launch command: agent binary + router prompt + channels
    let mut cmd = agent.launch.clone();

    // Inject the router system prompt via the agent's prompt flag
    if let Some(flag) = &agent.prompt_flag {
        let prompt = build_router_prompt().replace('\'', "'\\''");
        cmd = format!("{} {} '{}'", cmd, flag, prompt);
    }

    // Add bypass flag if configured
    if config::is_bypass_enabled(cfg, &agent.id) {
        if let Some(flag) = &agent.bypass_flag {
            cmd = format!("{} {}", cmd, flag);
        }
    }

    // Channels go to the router, not individual instances
    for channel in &router_cfg.channels {
        cmd = format!("{} --channels {}", cmd, channel);
    }

    let working_dir = router_cfg
        .working_dir
        .as_ref()
        .map(|d| {
            if d.starts_with('~') {
                let home = std::env::var("HOME").unwrap_or_default();
                d.replacen('~', &home, 1)
            } else {
                d.clone()
            }
        })
        .unwrap_or_else(|| std::env::var("HOME").unwrap_or_else(|_| "/".to_owned()));

    let startup_cmds = config::get_startup_commands(cfg, &working_dir);
    let full_cmd = if startup_cmds.is_empty() {
        cmd
    } else {
        let mut parts = startup_cmds;
        parts.push(cmd);
        parts.join(" && ")
    };

    tmux::create_session(&session_name, &working_dir, &full_cmd)?;

    // If agent needs title injection via send-keys (no prompt flag), inject after delay
    if agent.prompt_flag.is_none() {
        let prompt = build_router_prompt();
        let _ = tmux::send_keys_delayed(
            &session_name,
            &prompt,
            cfg.title_injection_delay,
        );
    }

    Ok(session_name)
}

/// Returns true if this session name is the router.
pub fn is_router_session(session_name: &str) -> bool {
    session_name == ROUTER_SESSION_NAME
}
