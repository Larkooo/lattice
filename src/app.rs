use std::{
    collections::{HashMap, HashSet},
    env,
    sync::mpsc,
    time::{Duration, Instant},
};

use crate::{agents, config, git, pathnav, tmux};
use agents::AgentDefinition;
use pathnav::Browser;

/// How long to wait before re-checking the PR state for a session.
const PR_RECHECK_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Debug, Clone)]
pub struct AgentInstance {
    pub agent: AgentDefinition,
    pub session: tmux::Session,
    pub managed: bool,
    pub title_override: String,
    pub completed: bool,
    pub pr_state: Option<git::PrState>,
    pub pr_number: Option<u32>,
    pub pr_checks: Option<git::PrChecksSummary>,
    pub branch: String,
}

/// Returns a sort key and display label for the category an instance belongs to.
/// Categories in ascending order: Running -> Completed -> PR Open -> CI Failed -> Merged.
pub fn instance_category(instance: &AgentInstance) -> (u8, &'static str) {
    match (&instance.pr_state, instance.pr_checks.as_ref()) {
        (Some(git::PrState::Merged), _) => (4, "merged"),
        (Some(git::PrState::Open), Some(checks)) if checks.has_failures() => (3, "ci failed"),
        (Some(git::PrState::Open), _) => (2, "pr open"),
        (Some(git::PrState::Closed), _) => (1, "completed"),
        (None, _) if instance.completed => (1, "completed"),
        _ => (0, "running"),
    }
}

/// Returns the project name for an instance: the basename of its working directory.
/// When the instance is running inside a lattice worktree
/// (`<root>/.lattice/worktrees/<id>`), the name is derived from the repo root
/// rather than the numeric worktree ID.
pub fn instance_project_name(instance: &AgentInstance) -> String {
    let path = &instance.session.pane_current_path;
    if path.is_empty() || path == "/" {
        return String::new();
    }

    // Strip the worktree suffix so we use the parent repo name, not the ID.
    let effective: &str = if let Some(idx) = path.find("/.lattice/worktrees/") {
        &path[..idx]
    } else {
        path.as_str()
    };

    if let Ok(home) = env::var("HOME") {
        if effective == home {
            return "~".to_owned();
        }
    }
    std::path::Path::new(effective)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[derive(Debug, Clone)]
pub struct SplitPane {
    pub session_name: String,
}

#[derive(Debug, Clone)]
pub struct SplitState {
    pub panes: Vec<SplitPane>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnStep {
    Agent,
    Path,
    NewDirectoryName,
    CloneUrl,
    TypePath,
}

#[derive(Debug, Clone)]
pub struct SpawnModal {
    pub step: SpawnStep,
    pub selected_agent: usize,
    pub browser: Browser,
    pub new_dir_name: String,
    pub clone_url: String,
    pub typed_path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppScreen {
    Warning,
    Main,
}

#[derive(Debug, Clone)]
pub struct Warning {
    pub title: String,
    pub message: String,
    pub details: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct UiTheme {
    pub bg: ratatui::style::Color,
    pub border: ratatui::style::Color,
    pub text: ratatui::style::Color,
    pub muted: ratatui::style::Color,
    pub accent: ratatui::style::Color,
    pub highlight_bg: ratatui::style::Color,
    pub yellow: ratatui::style::Color,
    pub green: ratatui::style::Color,
    pub red: ratatui::style::Color,
}

impl UiTheme {
    pub fn from_config(tc: &config::ThemeConfig) -> Self {
        use ratatui::style::Color;
        let c = |opt: Option<[u8; 3]>, default: Color| -> Color {
            match opt {
                Some([r, g, b]) => Color::Rgb(r, g, b),
                None => default,
            }
        };
        Self {
            bg: c(tc.bg, Color::Rgb(0, 0, 0)),
            border: c(tc.border, Color::Rgb(70, 60, 55)),
            text: c(tc.text, Color::Rgb(215, 205, 195)),
            muted: c(tc.muted, Color::Rgb(130, 120, 110)),
            accent: c(tc.accent, Color::Rgb(207, 144, 89)),
            highlight_bg: c(tc.highlight, Color::Rgb(191, 111, 74)),
            yellow: c(tc.yellow, Color::Rgb(228, 175, 105)),
            green: c(tc.green, Color::Rgb(169, 195, 140)),
            red: c(tc.red, Color::Rgb(212, 96, 87)),
        }
    }
}

pub struct StopResult {
    pub session_name: String,
    pub message: String,
}

pub struct App {
    pub available_agents: Vec<AgentDefinition>,
    pub instances: Vec<AgentInstance>,
    pub selected_row: usize,
    pub selected_tab: usize,
    pub modal: Option<SpawnModal>,
    pub last_refresh: Instant,
    pub refresh_interval: Duration,
    pub should_quit: bool,
    pub status_line: String,
    pub theme: UiTheme,
    pub screen: AppScreen,
    pub warning: Option<Warning>,
    pub tmux_available: bool,
    pub config: config::AppConfig,
    pub settings_open: bool,
    pub settings_selected: usize,
    pub settings_editing: Option<String>,
    pub startup_cmds_open: bool,
    pub startup_cmds_selected: usize,
    pub startup_cmds_adding: Option<StartupCmdAddState>,
    pub permissions_open: bool,
    pub permissions_selected: usize,
    pub split: Option<SplitState>,
    pub dev_servers_open: bool,
    pub dev_servers_selected: usize,
    pub dev_servers_adding: Option<DevServerAddState>,
    /// Maps agent session name → dev server tmux session name.
    pub dev_server_sessions: HashMap<String, String>,
    pub stopping_sessions: HashSet<String>,
    pub stop_tx: mpsc::Sender<StopResult>,
    pub stop_rx: mpsc::Receiver<StopResult>,
    /// Cached PR info per session name, together with when it was last fetched.
    pub pr_cache: HashMap<String, (git::PrStatus, Instant)>,
    /// Sessions currently having their PR state fetched in a background thread.
    pub pending_pr_checks: HashSet<String>,
    pub pr_tx: mpsc::Sender<(String, git::PrStatus)>,
    pub pr_rx: mpsc::Receiver<(String, git::PrStatus)>,
}

#[derive(Debug, Clone)]
pub enum StartupCmdAddStep {
    BrowsePath,
    TypePath,
    Command,
}

#[derive(Debug, Clone)]
pub struct StartupCmdAddState {
    pub step: StartupCmdAddStep,
    pub browser: Browser,
    pub path: String,
    pub commands: Vec<String>,
    pub current_input: String,
}

#[derive(Debug, Clone)]
pub enum DevServerAddStep {
    BrowsePath,
    TypePath,
    Command,
}

#[derive(Debug, Clone)]
pub struct DevServerAddState {
    pub step: DevServerAddStep,
    pub browser: Browser,
    pub path: String,
    pub current_input: String,
}

impl App {
    pub fn new(cfg: config::AppConfig) -> Self {
        let tmux_available = tmux::is_tmux_available();
        let refresh_interval = Duration::from_secs(cfg.refresh_interval.max(1));
        let (stop_tx, stop_rx) = mpsc::channel();
        let (pr_tx, pr_rx) = mpsc::channel();

        Self {
            available_agents: Vec::new(),
            instances: Vec::new(),
            selected_row: 0,
            selected_tab: 0,
            modal: None,
            last_refresh: Instant::now() - refresh_interval,
            refresh_interval,
            should_quit: false,
            status_line: String::new(),
            theme: UiTheme::from_config(&cfg.theme),
            screen: AppScreen::Main,
            warning: None,
            tmux_available,
            config: cfg,
            settings_open: false,
            settings_selected: 0,
            settings_editing: None,
            startup_cmds_open: false,
            startup_cmds_selected: 0,
            startup_cmds_adding: None,
            dev_servers_open: false,
            dev_servers_selected: 0,
            dev_servers_adding: None,
            dev_server_sessions: HashMap::new(),
            permissions_open: false,
            permissions_selected: 0,
            split: None,
            stopping_sessions: HashSet::new(),
            stop_tx,
            stop_rx,
            pr_cache: HashMap::new(),
            pending_pr_checks: HashSet::new(),
            pr_tx,
            pr_rx,
        }
    }

    pub fn check_warnings(&mut self) {
        if !self.tmux_available {
            self.warning = Some(Warning {
                title: "tmux not found".to_owned(),
                message: "lattice requires tmux to manage agent sessions.".to_owned(),
                details: vec![
                    "install via your package manager:".to_owned(),
                    "  brew install tmux".to_owned(),
                    "  apt install tmux".to_owned(),
                    "  pacman -S tmux".to_owned(),
                ],
            });
            self.screen = AppScreen::Warning;
            return;
        }

        if self.available_agents.is_empty() {
            self.warning = Some(Warning {
                title: "no agent CLIs found".to_owned(),
                message: "lattice needs at least one supported agent CLI in PATH.".to_owned(),
                details: vec![
                    "supported agents:".to_owned(),
                    "  claude    - Claude Code".to_owned(),
                    "  codex     - Codex CLI".to_owned(),
                    "  aider     - Aider".to_owned(),
                    "  gemini    - Gemini CLI".to_owned(),
                    "  opencode  - OpenCode".to_owned(),
                ],
            });
            self.screen = AppScreen::Warning;
            return;
        }

        self.warning = None;
        self.screen = AppScreen::Main;
    }

    pub fn refresh(&mut self) {
        self.tmux_available = tmux::is_tmux_available();
        self.available_agents = agents::detect_available_agents(&self.config.custom_agents);
        self.check_warnings();

        if !self.tmux_available {
            self.last_refresh = Instant::now();
            return;
        }

        match tmux::list_sessions() {
            Ok(sessions) => {
                self.instances = sessions
                    .into_iter()
                    .filter_map(|session| {
                        let agent = agents::classify_agent_from_session(
                            &session.name,
                            &session.current_command,
                            &self.available_agents,
                        )?;
                        let managed = agents::managed_session_agent_id(&session.name).is_some();
                        let title_override = agents::read_title_file(&session.name);
                        let completed = agents::is_done(&session.name);

                        // Use cached PR state; kick off a background check if
                        // the cache is missing or stale and no check is in flight.
                        let (pr_state, pr_number, pr_checks) = self
                            .pr_cache
                            .get(&session.name)
                            .map(|(status, _)| {
                                (status.state.clone(), status.number, status.checks.clone())
                            })
                            .unwrap_or((None, None, None));

                        if !session.pane_current_path.is_empty()
                            && !self.pending_pr_checks.contains(&session.name)
                            && self
                                .pr_cache
                                .get(&session.name)
                                .map(|(_, checked_at)| checked_at.elapsed() >= PR_RECHECK_INTERVAL)
                                .unwrap_or(true)
                        {
                            self.pending_pr_checks.insert(session.name.clone());
                            let tx = self.pr_tx.clone();
                            let name = session.name.clone();
                            let path = session.pane_current_path.clone();
                            std::thread::spawn(move || {
                                let status = git::gh_pr_status(std::path::Path::new(&path));
                                let _ = tx.send((name, status));
                            });
                        }

                        // Get the git branch (cheap local call).
                        let branch = if !session.pane_current_path.is_empty() {
                            git::current_branch(std::path::Path::new(&session.pane_current_path))
                        } else {
                            String::new()
                        };

                        Some(AgentInstance {
                            agent,
                            session,
                            managed,
                            title_override,
                            completed,
                            pr_state,
                            pr_number,
                            pr_checks,
                            branch,
                        })
                    })
                    .collect();

                self.instances.sort_by(|a, b| {
                    instance_project_name(a)
                        .cmp(&instance_project_name(b))
                        .then(instance_category(a).0.cmp(&instance_category(b).0))
                        .then(a.session.name.cmp(&b.session.name))
                });
                self.clamp_selection();

                let completed_count = self.instances.iter().filter(|i| i.completed).count();
                let running_count = self.instances.len() - completed_count;
                if completed_count > 0 {
                    self.status_line = format!(
                        "{running_count} running \u{2502} {completed_count} completed \u{2502} {} agents detected",
                        self.available_agents.len()
                    );
                } else {
                    self.status_line = format!(
                        "{} running \u{2502} {} agents detected",
                        self.instances.len(),
                        self.available_agents.len()
                    );
                }
            }
            Err(err) => {
                self.instances.clear();
                self.selected_row = 0;
                self.selected_tab = 0;
                self.status_line = format!("refresh failed: {err}");
            }
        }

        self.last_refresh = Instant::now();
    }

    pub fn dashboard_row_count(&self) -> usize {
        self.instances.len() + 2 // + action row + settings row
    }

    pub fn clamp_selection(&mut self) {
        if self.selected_row >= self.dashboard_row_count() {
            self.selected_row = self.dashboard_row_count().saturating_sub(1);
        }

        if self.selected_tab > self.instances.len() {
            self.selected_tab = 0;
        }

        if self.selected_tab > 0 {
            self.selected_row = self.selected_tab - 1;
        }

        // Prune split panes whose sessions no longer exist
        if let Some(split) = &mut self.split {
            let names: std::collections::HashSet<&str> =
                self.instances.iter().map(|i| i.session.name.as_str()).collect();
            split.panes.retain(|p| names.contains(p.session_name.as_str()));
            if split.panes.is_empty() {
                self.split = None;
            }
        }
    }

    pub fn selected_instance(&self) -> Option<&AgentInstance> {
        if self.selected_row < self.instances.len() {
            self.instances.get(self.selected_row)
        } else {
            None
        }
    }

    pub fn current_tab_instance(&self) -> Option<&AgentInstance> {
        if self.selected_tab == 0 {
            return None;
        }
        self.instances.get(self.selected_tab - 1)
    }

    pub fn is_action_row_selected(&self) -> bool {
        self.selected_tab == 0 && self.selected_row == self.instances.len()
    }

    pub fn is_settings_row_selected(&self) -> bool {
        self.selected_tab == 0 && self.selected_row == self.instances.len() + 1
    }

    pub fn next_row(&mut self) {
        let count = self.dashboard_row_count();
        self.selected_row = (self.selected_row + 1) % count;
    }

    pub fn previous_row(&mut self) {
        let count = self.dashboard_row_count();
        if self.selected_row == 0 {
            self.selected_row = count.saturating_sub(1);
        } else {
            self.selected_row -= 1;
        }
    }

    pub fn next_tab(&mut self) {
        let max = self.instances.len();
        self.selected_tab = if self.selected_tab >= max { 0 } else { self.selected_tab + 1 };
        if self.selected_tab > 0 {
            self.selected_row = self.selected_tab - 1;
        }
    }

    pub fn previous_tab(&mut self) {
        let max = self.instances.len();
        self.selected_tab = if self.selected_tab == 0 { max } else { self.selected_tab - 1 };
        if self.selected_tab > 0 {
            self.selected_row = self.selected_tab - 1;
        }
    }

    pub fn open_spawn_modal(&mut self) {
        if self.available_agents.is_empty() {
            self.status_line = "No supported agent CLIs found in PATH".to_owned();
            return;
        }

        let start = self
            .config
            .default_spawn_dir
            .as_ref()
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| "/".into()));
        match Browser::new(start) {
            Ok(browser) => {
                self.modal = Some(SpawnModal {
                    step: SpawnStep::Agent,
                    selected_agent: 0,
                    browser,
                    new_dir_name: String::new(),
                    clone_url: String::new(),
                    typed_path: String::new(),
                });
            }
            Err(err) => {
                self.status_line = format!("Cannot open path browser: {err}");
            }
        }
    }

    pub fn create_instance(&mut self, agent_index: usize, working_dir: String) {
        let Some(agent) = self.available_agents.get(agent_index).cloned() else {
            self.status_line = "Invalid agent selection".to_owned();
            self.modal = None;
            return;
        };

        let final_dir =
            if self.config.git_worktrees && git::is_git_repo(std::path::Path::new(&working_dir)) {
                match git::create_worktree(std::path::Path::new(&working_dir)) {
                    Ok(wt_path) => wt_path.to_string_lossy().to_string(),
                    Err(err) => {
                        self.status_line = format!("Worktree failed: {err}, using original dir");
                        working_dir.clone()
                    }
                }
            } else {
                working_dir.clone()
            };

        // Install co-author commit-msg hook if either setting is enabled
        // Lattice co-author takes priority (replaces agent co-author with Lattice)
        if self.config.lattice_coauthor {
            if let Err(err) = git::install_lattice_coauthor_hook(std::path::Path::new(&final_dir)) {
                self.status_line = format!("Co-author hook failed: {err}");
            }
        } else if self.config.strip_coauthor {
            if let Err(err) = git::install_strip_coauthor_hook(std::path::Path::new(&final_dir)) {
                self.status_line = format!("Co-author hook failed: {err}");
            }
        }

        let session_name = agents::build_managed_session_name(&agent.id);
        let title_enabled = self.config.title_injection_enabled;
        let bypass_enabled = config::is_bypass_enabled(&self.config, &agent.id);

        let launch_cmd =
            agents::build_launch_command(&agent, &session_name, title_enabled, bypass_enabled);

        // Prepend any configured startup commands for this directory.
        let startup_cmds = config::get_startup_commands(&self.config, &final_dir);
        let full_cmd = if startup_cmds.is_empty() {
            launch_cmd.clone()
        } else {
            let mut parts = startup_cmds.clone();
            parts.push(launch_cmd.clone());
            parts.join(" && ")
        };

        match tmux::create_session(&session_name, &final_dir, &full_cmd) {
            Ok(()) => {
                // Start dev server in a companion tmux session if configured.
                if let Some(dev_cmd) = config::get_dev_server_command(&self.config, &final_dir) {
                    let dev_session = format!("{session_name}_dev");
                    // Prepend startup commands so the dev server waits for
                    // them to finish (e.g. pnpm install) before starting.
                    let full_dev_cmd = if startup_cmds.is_empty() {
                        dev_cmd
                    } else {
                        let mut parts = startup_cmds.clone();
                        parts.push(dev_cmd);
                        parts.join(" && ")
                    };
                    match tmux::create_session(&dev_session, &final_dir, &full_dev_cmd) {
                        Ok(()) => {
                            self.dev_server_sessions
                                .insert(session_name.clone(), dev_session);
                        }
                        Err(err) => {
                            self.status_line =
                                format!("Dev server failed to start: {err}");
                        }
                    }
                }

                // For agents without a system-prompt flag, inject a first
                // message asking them to write task titles to a temp file.
                // Delay gives TUI-based agents time to boot.
                if title_enabled && agents::needs_title_injection(&agent) {
                    let msg = agents::build_title_injection(&session_name);
                    let delay = self.config.title_injection_delay;
                    let _ = tmux::send_keys_delayed(&session_name, &msg, delay);
                }

                self.status_line = format!("Started {} in {}", agent.label, final_dir);
                self.modal = None;
                self.refresh();

                if let Some(pos) =
                    self.instances.iter().position(|x| x.session.name == session_name)
                {
                    self.selected_row = pos;
                    self.selected_tab = pos + 1;
                }
            }
            Err(err) => {
                self.status_line = format!("Failed to start {}: {err}", agent.label);
                self.modal = None;
            }
        }
    }

    pub fn kill_selected_instance(&mut self) {
        let Some(instance) = self.active_instance_ref().cloned() else {
            self.status_line = "Select an instance row first".to_owned();
            return;
        };

        let session_name = instance.session.name.clone();

        // Prevent double-stop
        if self.stopping_sessions.contains(&session_name) {
            self.status_line = format!("{session_name} is already stopping...");
            return;
        }

        // Check if the session was running in a worktree before killing it
        let worktree_path =
            if self.config.git_worktrees && !instance.session.pane_current_path.is_empty() {
                let p = std::path::Path::new(&instance.session.pane_current_path);
                if git::is_worktree_path(p) {
                    Some(p.to_path_buf())
                } else {
                    None
                }
            } else {
                None
            };

        // Check for an associated dev server session to clean up.
        let dev_server_session = self.dev_server_sessions.remove(&session_name);

        // Mark as stopping so the UI shows an indicator
        self.stopping_sessions.insert(session_name.clone());
        self.status_line = format!("Stopping {session_name}...");

        // Spawn a background thread to do the blocking work
        let tx = self.stop_tx.clone();
        std::thread::spawn(move || {
            // Kill the companion dev server session first.
            if let Some(ref dev_name) = dev_server_session {
                let _ = tmux::kill_session(dev_name);
            }
            let message = match tmux::kill_session(&session_name) {
                Ok(()) => {
                    agents::remove_title_file(&session_name);
                    agents::remove_done_file(&session_name);

                    if let Some(wt) = worktree_path {
                        match git::remove_worktree(&wt) {
                            Ok(()) => {
                                format!("Stopped {session_name} (worktree cleaned)")
                            }
                            Err(err) => {
                                format!("Stopped {session_name} (worktree cleanup failed: {err})")
                            }
                        }
                    } else {
                        format!("Stopped {session_name}")
                    }
                }
                Err(err) => {
                    format!("Failed to stop {session_name}: {err}")
                }
            };

            let _ = tx.send(StopResult { session_name, message });
        });
    }

    pub fn drain_stop_results(&mut self) {
        while let Ok(result) = self.stop_rx.try_recv() {
            self.stopping_sessions.remove(&result.session_name);
            self.status_line = result.message;
            // Force a refresh so the stopped instance disappears from the list
            self.last_refresh = Instant::now() - self.refresh_interval;
        }
    }

    pub fn drain_pr_results(&mut self) {
        let mut resort = false;
        while let Ok((name, status)) = self.pr_rx.try_recv() {
            self.pending_pr_checks.remove(&name);
            self.pr_cache.insert(name.clone(), (status.clone(), Instant::now()));
            if let Some(inst) = self.instances.iter_mut().find(|i| i.session.name == name) {
                if inst.pr_state != status.state || inst.pr_checks != status.checks {
                    inst.pr_state = status.state.clone();
                    inst.pr_checks = status.checks.clone();
                    resort = true;
                }
                inst.pr_number = status.number;
            }
        }
        if resort {
            self.instances.sort_by(|a, b| {
                instance_project_name(a)
                    .cmp(&instance_project_name(b))
                    .then(instance_category(a).0.cmp(&instance_category(b).0))
                    .then(a.session.name.cmp(&b.session.name))
            });
        }
    }

    pub fn active_instance_ref(&self) -> Option<&AgentInstance> {
        if self.selected_tab == 0 {
            self.selected_instance()
        } else {
            self.current_tab_instance()
        }
    }

    pub fn is_split_mode(&self) -> bool {
        self.split.is_some()
    }

    pub fn enter_split_mode(&mut self) {
        let instance = if self.selected_tab > 0 {
            self.current_tab_instance()
        } else {
            self.selected_instance()
        };
        let Some(instance) = instance else {
            self.status_line = "Select an instance first".to_owned();
            return;
        };
        let name = instance.session.name.clone();
        self.split = Some(SplitState { panes: vec![SplitPane { session_name: name }] });
        self.status_line =
            "Split: navigate tabs and press v to add panes, enter to launch".to_owned();
    }

    pub fn add_split_pane(&mut self) {
        let Some(split) = &self.split else { return };
        let shown: std::collections::HashSet<&str> =
            split.panes.iter().map(|p| p.session_name.as_str()).collect();

        // Prefer the currently viewed instance (tab or selected row)
        let candidate = if self.selected_tab > 0 {
            self.current_tab_instance()
        } else {
            self.selected_instance()
        };
        let next = candidate
            .filter(|i| !shown.contains(i.session.name.as_str()))
            .or_else(|| self.instances.iter().find(|i| !shown.contains(i.session.name.as_str())));

        if let Some(inst) = next {
            let name = inst.session.name.clone();
            let count = split.panes.len() + 1;
            if let Some(split) = &mut self.split {
                split.panes.push(SplitPane { session_name: name });
            }
            self.status_line = format!("Split: {count} panes selected, enter to launch");
        } else {
            self.status_line = "No more instances to add".to_owned();
        }
    }

    pub fn close_focused_pane(&mut self) {
        let Some(split) = &mut self.split else { return };
        if split.panes.len() <= 1 {
            self.split = None;
            self.status_line = "Split cancelled".to_owned();
            return;
        }
        split.panes.pop();
        let count = split.panes.len();
        self.status_line = format!("Split: {count} panes selected");
    }

    /// Kill the dev server for the selected instance (if one is running).
    pub fn kill_dev_server(&mut self) {
        let Some(instance) = self.active_instance_ref() else {
            self.status_line = "Select an instance first".to_owned();
            return;
        };
        let session_name = instance.session.name.clone();
        if let Some(dev_name) = self.dev_server_sessions.remove(&session_name) {
            match tmux::kill_session(&dev_name) {
                Ok(()) => self.status_line = "Dev server stopped".to_owned(),
                Err(err) => self.status_line = format!("Failed to stop dev server: {err}"),
            }
        } else {
            self.status_line = "No dev server running for this instance".to_owned();
        }
    }

    /// Restart the dev server for the selected instance. Kills the existing one
    /// (if any) and starts a fresh session using the matching config rule.
    pub fn restart_dev_server(&mut self) {
        let Some(instance) = self.active_instance_ref() else {
            self.status_line = "Select an instance first".to_owned();
            return;
        };
        let session_name = instance.session.name.clone();
        let working_dir = instance.session.pane_current_path.clone();

        // Kill existing dev server if running.
        if let Some(dev_name) = self.dev_server_sessions.remove(&session_name) {
            let _ = tmux::kill_session(&dev_name);
        }

        if working_dir.is_empty() {
            self.status_line = "No working directory for this instance".to_owned();
            return;
        }

        match config::get_dev_server_command(&self.config, &working_dir) {
            Some(dev_cmd) => {
                let dev_session = format!("{session_name}_dev");
                // Prepend startup commands so the dev server waits for
                // them to finish (e.g. pnpm install) before starting.
                let startup_cmds = config::get_startup_commands(&self.config, &working_dir);
                let full_dev_cmd = if startup_cmds.is_empty() {
                    dev_cmd
                } else {
                    let mut parts = startup_cmds;
                    parts.push(dev_cmd);
                    parts.join(" && ")
                };
                match tmux::create_session(&dev_session, &working_dir, &full_dev_cmd) {
                    Ok(()) => {
                        self.dev_server_sessions
                            .insert(session_name, dev_session);
                        self.status_line = "Dev server restarted".to_owned();
                    }
                    Err(err) => {
                        self.status_line = format!("Dev server failed to start: {err}");
                    }
                }
            }
            None => {
                self.status_line = "No dev server configured for this directory".to_owned();
            }
        }
    }

    /// Returns true if the selected instance has a dev server running.
    pub fn has_dev_server(&self) -> bool {
        self.active_instance_ref()
            .map(|i| self.dev_server_sessions.contains_key(&i.session.name))
            .unwrap_or(false)
    }
}
