use std::{io::Stdout, time::Duration};

use anyhow::Result;
use crossterm::{
    event::{
        DisableMouseCapture, EnableMouseCapture, KeyCode, KeyModifiers, MouseEvent,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::{
    agents,
    app::{
        App, DevServerAddState, DevServerAddStep, SpawnStep, StartupCmdAddState, StartupCmdAddStep,
    },
    config, git,
    pathnav::{ActivateResult, Browser},
    tmux,
};

pub fn handle_warning_key(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Char('q') | KeyCode::Esc => app.should_quit = true,
        KeyCode::Char('r') => app.refresh(),
        _ => {}
    }
}

pub fn handle_main_mouse(_app: &mut App, _mouse: MouseEvent) {
    // Tab titles now auto-scroll via tick-based animation, so no manual
    // scroll handling is needed. Mouse events are intentionally ignored
    // to prevent horizontal trackpad movement from affecting the UI.
}

pub fn handle_modal_key(app: &mut App, code: KeyCode) {
    enum Action {
        None,
        Close,
        CreateInstance { agent_index: usize, working_dir: String },
        CreateDirectory { name: String },
        CloneRepo { url: String },
    }

    let mut action = Action::None;
    let mut status_override: Option<String> = None;

    if let Some(modal) = app.modal.as_mut() {
        match modal.step {
            SpawnStep::Agent => match code {
                KeyCode::Esc => action = Action::Close,
                KeyCode::Char('j') | KeyCode::Down => {
                    if !app.available_agents.is_empty() {
                        modal.selected_agent =
                            (modal.selected_agent + 1) % app.available_agents.len();
                    }
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    if !app.available_agents.is_empty() {
                        if modal.selected_agent == 0 {
                            modal.selected_agent = app.available_agents.len() - 1;
                        } else {
                            modal.selected_agent -= 1;
                        }
                    }
                }
                KeyCode::Enter => modal.step = SpawnStep::Path,
                // Number keys: select agent by index and advance to path step
                KeyCode::Char(c @ '1'..='9') => {
                    let idx = (c as usize) - ('1' as usize);
                    if idx < app.available_agents.len() {
                        modal.selected_agent = idx;
                        modal.step = SpawnStep::Path;
                    }
                }
                _ => {}
            },
            SpawnStep::Path => match code {
                KeyCode::Esc => action = Action::Close,
                KeyCode::Left | KeyCode::Char('h') => modal.step = SpawnStep::Agent,
                KeyCode::Char('j') | KeyCode::Down => modal.browser.next(),
                KeyCode::Char('k') | KeyCode::Up => modal.browser.previous(),
                KeyCode::PageDown => {
                    for _ in 0..10 {
                        modal.browser.next();
                    }
                }
                KeyCode::PageUp => {
                    for _ in 0..10 {
                        modal.browser.previous();
                    }
                }
                KeyCode::Enter => match modal.browser.activate_selected() {
                    Ok(ActivateResult::Selected(path)) => {
                        action = Action::CreateInstance {
                            agent_index: modal.selected_agent,
                            working_dir: path.to_string_lossy().to_string(),
                        }
                    }
                    Ok(ActivateResult::ChangedDirectory) => {}
                    Ok(ActivateResult::StartCreateDirectory) => {
                        modal.step = SpawnStep::NewDirectoryName;
                        modal.new_dir_name.clear();
                    }
                    Ok(ActivateResult::StartCloneFromUrl) => {
                        modal.step = SpawnStep::CloneUrl;
                        modal.clone_url.clear();
                    }
                    Ok(ActivateResult::StartTypePath) => {
                        modal.step = SpawnStep::TypePath;
                        modal.typed_path = modal.browser.cwd().to_string_lossy().to_string();
                    }
                    Err(err) => {
                        status_override = Some(format!("Path navigation failed: {err}"));
                    }
                },
                // Shortcut: select current directory
                KeyCode::Char('.') => {
                    action = Action::CreateInstance {
                        agent_index: modal.selected_agent,
                        working_dir: modal.browser.cwd().to_string_lossy().to_string(),
                    }
                }
                // Shortcut: create new directory
                KeyCode::Char('+') => {
                    modal.step = SpawnStep::NewDirectoryName;
                    modal.new_dir_name.clear();
                }
                // Shortcut: clone from git URL
                KeyCode::Char('g') => {
                    modal.step = SpawnStep::CloneUrl;
                    modal.clone_url.clear();
                }
                // Shortcut: type path directly
                KeyCode::Char('/') => {
                    modal.step = SpawnStep::TypePath;
                    modal.typed_path = modal.browser.cwd().to_string_lossy().to_string();
                }
                // Shortcut: go to parent directory
                KeyCode::Char('-') | KeyCode::Backspace => {
                    if let Err(err) = modal.browser.go_to_parent() {
                        status_override = Some(format!("Navigation failed: {err}"));
                    }
                }
                // Shortcut: jump to home directory
                KeyCode::Char('~') => {
                    if let Ok(home) = std::env::var("HOME") {
                        let home_path = std::path::Path::new(&home);
                        if let Err(err) = modal.browser.navigate_to(home_path) {
                            status_override = Some(format!("Navigation failed: {err}"));
                        }
                    }
                }
                // Shortcut: open in current directory with Enter-like behavior
                KeyCode::Char('l') | KeyCode::Right => match modal.browser.activate_selected() {
                    Ok(ActivateResult::Selected(path)) => {
                        action = Action::CreateInstance {
                            agent_index: modal.selected_agent,
                            working_dir: path.to_string_lossy().to_string(),
                        }
                    }
                    Ok(ActivateResult::ChangedDirectory) => {}
                    Ok(ActivateResult::StartCreateDirectory) => {
                        modal.step = SpawnStep::NewDirectoryName;
                        modal.new_dir_name.clear();
                    }
                    Ok(ActivateResult::StartCloneFromUrl) => {
                        modal.step = SpawnStep::CloneUrl;
                        modal.clone_url.clear();
                    }
                    Ok(ActivateResult::StartTypePath) => {
                        modal.step = SpawnStep::TypePath;
                        modal.typed_path = modal.browser.cwd().to_string_lossy().to_string();
                    }
                    Err(err) => {
                        status_override = Some(format!("Path navigation failed: {err}"));
                    }
                },
                _ => {}
            },
            SpawnStep::TypePath => match code {
                KeyCode::Esc => {
                    modal.step = SpawnStep::Path;
                    modal.typed_path.clear();
                }
                KeyCode::Enter => {
                    let input = modal.typed_path.trim().to_owned();
                    let home = std::env::var("HOME").unwrap_or_default();
                    let expanded =
                        if input.starts_with('~') { input.replacen('~', &home, 1) } else { input };
                    let path = std::path::Path::new(&expanded);
                    if path.is_dir() {
                        match modal.browser.navigate_to(path) {
                            Ok(()) => {
                                modal.step = SpawnStep::Path;
                                modal.typed_path.clear();
                            }
                            Err(err) => {
                                status_override = Some(format!("Cannot navigate to path: {err}"));
                            }
                        }
                    } else {
                        status_override = Some("Not a valid directory".to_owned());
                    }
                }
                KeyCode::Backspace => {
                    modal.typed_path.pop();
                }
                KeyCode::Char(c) => {
                    if !c.is_control() {
                        modal.typed_path.push(c);
                    }
                }
                _ => {}
            },
            SpawnStep::NewDirectoryName => match code {
                KeyCode::Esc => {
                    modal.step = SpawnStep::Path;
                    modal.new_dir_name.clear();
                }
                KeyCode::Enter => {
                    action = Action::CreateDirectory { name: modal.new_dir_name.clone() }
                }
                KeyCode::Backspace => {
                    modal.new_dir_name.pop();
                }
                KeyCode::Char(c) => {
                    if !c.is_control() {
                        modal.new_dir_name.push(c);
                    }
                }
                _ => {}
            },
            SpawnStep::CloneUrl => match code {
                KeyCode::Esc => {
                    modal.step = SpawnStep::Path;
                    modal.clone_url.clear();
                }
                KeyCode::Enter => action = Action::CloneRepo { url: modal.clone_url.clone() },
                KeyCode::Backspace => {
                    modal.clone_url.pop();
                }
                KeyCode::Char(c) => {
                    if !c.is_control() {
                        modal.clone_url.push(c);
                    }
                }
                _ => {}
            },
        }
    }

    if let Some(status) = status_override {
        app.status_line = status;
    }

    match action {
        Action::None => {}
        Action::Close => app.modal = None,
        Action::CreateInstance { agent_index, working_dir } => {
            app.create_instance(agent_index, working_dir)
        }
        Action::CreateDirectory { name } => {
            if let Some(modal) = app.modal.as_mut() {
                match modal.browser.create_directory(&name) {
                    Ok(path) => {
                        modal.step = SpawnStep::Path;
                        modal.new_dir_name.clear();
                        app.status_line = format!("Created {}", path.display());
                    }
                    Err(err) => {
                        app.status_line = format!("Create directory failed: {err}");
                    }
                }
            }
        }
        Action::CloneRepo { url } => {
            if let Some(modal) = app.modal.as_mut() {
                let dest = modal.browser.cwd().to_path_buf();
                match git::clone_repo(&url, &dest) {
                    Ok(clone_path) => {
                        app.status_line = format!("Cloned into {}", clone_path.display());
                        // Navigate browser into the cloned directory
                        modal.step = SpawnStep::Path;
                        modal.clone_url.clear();
                        if let Ok(new_browser) = Browser::new(clone_path) {
                            modal.browser = new_browser;
                        }
                    }
                    Err(err) => {
                        app.status_line = format!("Clone failed: {err}");
                    }
                }
            }
        }
    }
}

pub fn handle_main_key(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
    code: KeyCode,
    _modifiers: KeyModifiers,
) -> Result<()> {
    // Split-selection keybinds take priority
    if app.is_split_mode() {
        match code {
            KeyCode::Char('q') => app.should_quit = true,
            KeyCode::Esc => {
                // Cancel split selection
                app.split = None;
                app.status_line = "Split cancelled".to_owned();
            }
            KeyCode::Char('v') => app.add_split_pane(),
            KeyCode::Char('c') => app.close_focused_pane(),
            // Tab navigation to browse instances while selecting
            KeyCode::Char('h') | KeyCode::Left => app.previous_tab(),
            KeyCode::Char('l') | KeyCode::Right | KeyCode::Tab => app.next_tab(),
            KeyCode::Char('j') | KeyCode::Down => {
                if app.selected_tab == 0 {
                    app.next_row();
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if app.selected_tab == 0 {
                    app.previous_row();
                }
            }
            KeyCode::Char('r') => app.refresh(),
            KeyCode::Enter => {
                // Launch native tmux split and attach
                if let Some(split) = app.split.take() {
                    let targets: Vec<String> =
                        split.panes.iter().map(|p| p.session_name.clone()).collect();
                    if targets.len() < 2 {
                        app.status_line =
                            "Add at least 2 panes (press v on another tab)".to_owned();
                        app.split = Some(split);
                    } else {
                        let ts = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs();
                        let split_name = format!("lattice_split_{ts}");
                        match tmux::create_split_session(&split_name, &targets) {
                            Ok(()) => {
                                let attach_result = attach_into_session(terminal, &split_name);
                                // Clean up temp session on return
                                let _ = tmux::kill_session(&split_name);
                                match attach_result {
                                    Ok(()) => app.status_line = "Exited split view".to_owned(),
                                    Err(err) => {
                                        app.status_line = format!("Split attach failed: {err}")
                                    }
                                }
                                app.refresh();
                            }
                            Err(err) => {
                                app.status_line = format!("Failed to create split: {err}");
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        return Ok(());
    }

    match code {
        KeyCode::Char('q') | KeyCode::Esc => app.should_quit = true,
        KeyCode::Char('j') | KeyCode::Down => {
            if app.selected_tab == 0 {
                app.next_row();
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if app.selected_tab == 0 {
                app.previous_row();
            }
        }
        KeyCode::Char('h') | KeyCode::Left => app.previous_tab(),
        KeyCode::Char('l') | KeyCode::Right | KeyCode::Tab => app.next_tab(),
        KeyCode::Char('s') | KeyCode::Char('d') => app.selected_tab = 0,
        KeyCode::Char('n') => app.open_spawn_modal(),
        KeyCode::Char('v') => app.enter_split_mode(),
        KeyCode::Char('t') => {
            if let Some(instance) = app.active_instance_ref() {
                let name = instance.session.name.clone();
                let dir = if instance.session.pane_current_path.is_empty() {
                    ".".to_owned()
                } else {
                    instance.session.pane_current_path.clone()
                };
                match tmux::split_window(&name, &dir) {
                    Ok(()) => app.status_line = format!("Opened terminal in {name}"),
                    Err(err) => app.status_line = format!("Failed to split terminal: {err}"),
                }
            }
        }
        KeyCode::Char('x') => app.kill_selected_instance(),
        KeyCode::Char('f') => {
            if let Some(instance) = app.active_instance_ref().cloned() {
                if instance.pr_state != Some(git::PrState::Open) {
                    app.status_line = "No open PR to fix".to_owned();
                } else if let Some(checks) = instance.pr_checks.as_ref() {
                    if !checks.has_failures() {
                        app.status_line = "No failing CI checks on this PR".to_owned();
                    } else {
                        match tmux::send_keys(
                            &instance.session.name,
                            &agents::build_fix_ci_prompt(&checks.failed),
                        ) {
                            Ok(()) => {
                                app.status_line = "CI fix prompt sent to the instance".to_owned()
                            }
                            Err(err) => {
                                app.status_line = format!("Failed to send CI fix prompt: {err}")
                            }
                        }
                    }
                } else {
                    app.status_line = "CI status not available yet".to_owned();
                }
            } else {
                app.status_line = "Select an instance first".to_owned();
            }
        }
        KeyCode::Char('p') => {
            if let Some(instance) = app.active_instance_ref().cloned() {
                match &instance.pr_state {
                    Some(git::PrState::Merged) => {
                        app.status_line =
                            "PR already merged \u{2014} press x to stop instance".to_owned();
                    }
                    Some(git::PrState::Open) => {
                        match tmux::send_keys(
                            &instance.session.name,
                            &agents::build_merge_pr_prompt(),
                        ) {
                            Ok(()) => {
                                app.status_line =
                                    "Merge prompt sent \u{2014} instance ready to stop (press x)"
                                        .to_owned()
                            }
                            Err(err) => {
                                app.status_line = format!("Failed to send merge prompt: {err}")
                            }
                        }
                    }
                    _ => {
                        match tmux::send_keys(&instance.session.name, &agents::build_pr_prompt()) {
                            Ok(()) => {
                                app.status_line =
                                    "PR prompt sent \u{2014} press p again once PR is open"
                                        .to_owned()
                            }
                            Err(err) => {
                                app.status_line = format!("Failed to send PR prompt: {err}")
                            }
                        }
                    }
                }
            } else {
                app.status_line = "Select an instance first".to_owned();
            }
        }
        KeyCode::Char('o') => {
            if let Some(instance) = app.active_instance_ref() {
                match &instance.pr_state {
                    Some(git::PrState::Open) | Some(git::PrState::Merged) => {
                        let dir = instance.session.pane_current_path.clone();
                        if !dir.is_empty() {
                            git::gh_pr_open_in_browser(std::path::Path::new(&dir));
                            app.status_line = "Opening PR in browser\u{2026}".to_owned();
                        } else {
                            app.status_line = "No working directory for this instance".to_owned();
                        }
                    }
                    _ => {
                        app.status_line = "No PR to open \u{2014} press p to create one".to_owned();
                    }
                }
            } else {
                app.status_line = "Select an instance first".to_owned();
            }
        }
        KeyCode::Char('O') => {
            if let Some(instance) = app.active_instance_ref() {
                let session_name = instance.session.name.clone();
                if let Some(url) = app.dev_server_urls.get(&session_name) {
                    git::open_url_in_browser(url);
                    app.status_line = "Opening dev server in browser…".to_owned();
                } else if app.dev_server_sessions.contains_key(&session_name) {
                    app.status_line = "Dev server is starting — URL not detected yet".to_owned();
                } else {
                    app.status_line = "No dev server running for this instance".to_owned();
                }
            } else {
                app.status_line = "Select an instance first".to_owned();
            }
        }
        KeyCode::Char('r') => app.refresh(),
        KeyCode::Char('D') => app.kill_dev_server(),
        KeyCode::Char('R') => app.restart_dev_server(),
        KeyCode::Char(c @ '1'..='9') => {
            let idx = (c as usize) - ('0' as usize);
            if idx <= app.instances.len() {
                app.selected_tab = idx;
                app.selected_row = idx - 1;
            }
        }
        KeyCode::Enter => {
            if app.selected_tab == 0 && app.is_settings_row_selected() {
                app.settings_open = true;
                app.settings_selected = 0;
                app.settings_editing = None;
            } else if app.selected_tab == 0 && app.is_action_row_selected() {
                app.open_spawn_modal();
            } else if let Some(instance) = app.active_instance_ref() {
                let name = instance.session.name.clone();
                let attach_result = attach_into_session(terminal, &name);
                match attach_result {
                    Ok(()) => app.status_line = format!("Detached from {}", name),
                    Err(err) => app.status_line = format!("Attach failed for {}: {err}", name),
                }
                app.refresh();
            }
        }
        _ => {}
    }

    Ok(())
}

pub const SETTINGS_COUNT: usize = 15;

pub fn setting_label(index: usize) -> &'static str {
    match index {
        0 => "Refresh interval",
        1 => "Default spawn dir",
        2 => "Title injection",
        3 => "Title injection delay",
        4 => "Git worktrees",
        5 => "Strip co-author",
        6 => "Lattice co-author",
        7 => "Sound on completion",
        8 => "Sound method",
        9 => "Sound command",
        10 => "Startup commands",
        11 => "Dev servers",
        12 => "Agent permissions",
        13 => "Router channels",
        14 => "Router",
        _ => "",
    }
}

pub fn setting_value(config: &config::AppConfig, index: usize) -> String {
    match index {
        0 => format!("{}", config.refresh_interval),
        1 => config.default_spawn_dir.clone().unwrap_or_default(),
        2 => {
            if config.title_injection_enabled {
                "on".to_owned()
            } else {
                "off".to_owned()
            }
        }
        3 => format!("{}", config.title_injection_delay),
        4 => {
            if config.git_worktrees {
                "on".to_owned()
            } else {
                "off".to_owned()
            }
        }
        5 => {
            if config.strip_coauthor {
                "on".to_owned()
            } else {
                "off".to_owned()
            }
        }
        6 => {
            if config.lattice_coauthor {
                "on".to_owned()
            } else {
                "off".to_owned()
            }
        }
        7 => {
            if config.notifications.sound_on_completion {
                "on".to_owned()
            } else {
                "off".to_owned()
            }
        }
        8 => match config.notifications.sound_method {
            config::SoundMethod::Bell => "bell".to_owned(),
            config::SoundMethod::Command => "command".to_owned(),
        },
        9 => config.notifications.sound_command.clone(),
        10 => {
            let n = config.startup_commands.len();
            if n == 0 {
                "none configured".to_owned()
            } else {
                format!("{n} rule{}", if n == 1 { "" } else { "s" })
            }
        }
        11 => {
            let n = config.dev_servers.len();
            if n == 0 {
                "none configured".to_owned()
            } else {
                format!("{n} rule{}", if n == 1 { "" } else { "s" })
            }
        }
        12 => {
            let n = config.permissions_bypass.values().filter(|&&v| v).count();
            if n == 0 {
                "all restricted".to_owned()
            } else {
                format!("{n} bypassed")
            }
        }
        13 => {
            let n = config.router.as_ref().map(|r| r.channels.len()).unwrap_or(0);
            if n == 0 {
                "none configured".to_owned()
            } else {
                format!("{n} channel{}", if n == 1 { "" } else { "s" })
            }
        }
        14 => {
            match &config.router {
                Some(r) if r.enabled => {
                    let n = r.channels.len();
                    format!("enabled ({n} channel{})", if n == 1 { "" } else { "s" })
                }
                _ => "disabled".to_owned(),
            }
        }
        _ => String::new(),
    }
}

pub fn setting_is_bool(index: usize) -> bool {
    matches!(index, 2 | 4 | 5 | 6 | 7)
}

pub fn setting_is_cycle(index: usize) -> bool {
    index == 8
}

pub fn apply_setting(app: &mut App, index: usize, value: &str) {
    match index {
        0 => {
            if let Ok(v) = value.parse::<u64>() {
                let v = v.max(1);
                app.config.refresh_interval = v;
                app.refresh_interval = Duration::from_secs(v);
            }
        }
        1 => {
            if value.is_empty() {
                app.config.default_spawn_dir = None;
            } else {
                app.config.default_spawn_dir = Some(value.to_owned());
            }
        }
        2 => {
            app.config.title_injection_enabled = !app.config.title_injection_enabled;
        }
        3 => {
            if let Ok(v) = value.parse::<u32>() {
                app.config.title_injection_delay = v;
            }
        }
        4 => {
            app.config.git_worktrees = !app.config.git_worktrees;
        }
        5 => {
            app.config.strip_coauthor = !app.config.strip_coauthor;
        }
        6 => {
            app.config.lattice_coauthor = !app.config.lattice_coauthor;
        }
        7 => {
            app.config.notifications.sound_on_completion =
                !app.config.notifications.sound_on_completion;
        }
        8 => {
            app.config.notifications.sound_method = match app.config.notifications.sound_method {
                config::SoundMethod::Bell => config::SoundMethod::Command,
                config::SoundMethod::Command => config::SoundMethod::Bell,
            };
        }
        9 => {
            app.config.notifications.sound_command = value.to_owned();
        }
        _ => {}
    }
}

pub fn handle_settings_key(app: &mut App, code: KeyCode) {
    if let Some(ref mut buf) = app.settings_editing {
        // In edit mode
        match code {
            KeyCode::Esc => {
                app.settings_editing = None;
            }
            KeyCode::Enter => {
                let value = buf.clone();
                let idx = app.settings_selected;
                apply_setting(app, idx, &value);
                app.settings_editing = None;
                match config::save_config(&app.config) {
                    Ok(()) => app.status_line = "Settings saved".to_owned(),
                    Err(e) => app.status_line = format!("Save failed: {e}"),
                }
            }
            KeyCode::Backspace => {
                buf.pop();
            }
            KeyCode::Char(c) => {
                buf.push(c);
            }
            _ => {}
        }
        return;
    }

    match code {
        KeyCode::Esc | KeyCode::Char('q') => {
            app.settings_open = false;
        }
        KeyCode::Char('j') | KeyCode::Down => {
            app.settings_selected = (app.settings_selected + 1) % SETTINGS_COUNT;
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if app.settings_selected == 0 {
                app.settings_selected = SETTINGS_COUNT - 1;
            } else {
                app.settings_selected -= 1;
            }
        }
        KeyCode::Enter => {
            let idx = app.settings_selected;
            if idx == 10 {
                // Open startup commands sub-view
                app.startup_cmds_open = true;
                app.startup_cmds_selected = 0;
                app.startup_cmds_adding = None;
            } else if idx == 11 {
                // Open dev servers sub-view
                app.dev_servers_open = true;
                app.dev_servers_selected = 0;
                app.dev_servers_adding = None;
            } else if idx == 12 {
                // Open agent permissions sub-view
                app.permissions_open = true;
                app.permissions_selected = 0;
            } else if idx == 13 {
                // Open channels sub-view
                app.channels_open = true;
                app.channels_selected = 0;
                app.channels_adding = None;
            } else if idx == 14 {
                // Open router sub-view
                app.router_settings_open = true;
                app.router_settings_selected = 0;
                app.router_settings_editing = None;
            } else if setting_is_bool(idx) || setting_is_cycle(idx) {
                apply_setting(app, idx, "");
                match config::save_config(&app.config) {
                    Ok(()) => app.status_line = "Settings saved".to_owned(),
                    Err(e) => app.status_line = format!("Save failed: {e}"),
                }
            } else {
                app.settings_editing = Some(setting_value(&app.config, idx));
            }
        }
        _ => {}
    }
}

pub fn handle_startup_cmds_key(app: &mut App, code: KeyCode) {
    // If we're in the add flow, handle that first.
    if let Some(ref mut state) = app.startup_cmds_adding {
        match state.step {
            StartupCmdAddStep::BrowsePath => match code {
                KeyCode::Esc => {
                    app.startup_cmds_adding = None;
                }
                KeyCode::Char('j') | KeyCode::Down => state.browser.next(),
                KeyCode::Char('k') | KeyCode::Up => state.browser.previous(),
                KeyCode::PageDown => {
                    for _ in 0..10 {
                        state.browser.next();
                    }
                }
                KeyCode::PageUp => {
                    for _ in 0..10 {
                        state.browser.previous();
                    }
                }
                KeyCode::Enter => match state.browser.activate_selected() {
                    Ok(ActivateResult::Selected(path)) => {
                        state.path = path.to_string_lossy().to_string();
                        state.step = StartupCmdAddStep::Command;
                    }
                    Ok(ActivateResult::StartTypePath) => {
                        state.current_input = state.browser.cwd().to_string_lossy().to_string();
                        state.step = StartupCmdAddStep::TypePath;
                    }
                    Ok(ActivateResult::ChangedDirectory) => {}
                    Ok(_) => {} // ignore create dir / clone in this context
                    Err(err) => {
                        app.status_line = format!("Path navigation failed: {err}");
                    }
                },
                _ => {}
            },
            StartupCmdAddStep::TypePath => match code {
                KeyCode::Esc => {
                    state.step = StartupCmdAddStep::BrowsePath;
                    state.current_input.clear();
                }
                KeyCode::Backspace => {
                    state.current_input.pop();
                }
                KeyCode::Char(c) => {
                    if !c.is_control() {
                        state.current_input.push(c);
                    }
                }
                KeyCode::Enter => {
                    let input = state.current_input.trim().to_owned();
                    let home = std::env::var("HOME").unwrap_or_default();
                    let expanded = if input.starts_with('~') {
                        input.replacen('~', &home, 1)
                    } else {
                        input.clone()
                    };
                    let path = std::path::Path::new(&expanded);
                    if path.is_dir() {
                        match state.browser.navigate_to(path) {
                            Ok(()) => {
                                state.step = StartupCmdAddStep::BrowsePath;
                                state.current_input.clear();
                            }
                            Err(err) => {
                                app.status_line = format!("Cannot navigate to path: {err}");
                            }
                        }
                    } else {
                        app.status_line = "Not a valid directory".to_owned();
                    }
                }
                _ => {}
            },
            StartupCmdAddStep::Command => match code {
                KeyCode::Esc => {
                    if state.commands.is_empty() && state.current_input.is_empty() {
                        app.startup_cmds_adding = None;
                    } else {
                        // Go back, discard current command input
                        state.current_input.clear();
                        if state.commands.is_empty() {
                            state.step = StartupCmdAddStep::BrowsePath;
                        }
                    }
                }
                KeyCode::Backspace => {
                    state.current_input.pop();
                }
                KeyCode::Char(c) => {
                    state.current_input.push(c);
                }
                KeyCode::Enter => {
                    let input = state.current_input.trim().to_owned();
                    if input.is_empty() {
                        // Empty input = done adding commands
                        if state.commands.is_empty() {
                            app.startup_cmds_adding = None;
                            return;
                        }
                        let entry = config::StartupCommandsConfig {
                            path: state.path.clone(),
                            commands: state.commands.clone(),
                        };
                        app.config.startup_commands.push(entry);
                        app.startup_cmds_adding = None;
                        match config::save_config(&app.config) {
                            Ok(()) => app.status_line = "Startup command rule added".to_owned(),
                            Err(e) => app.status_line = format!("Save failed: {e}"),
                        }
                    } else {
                        state.commands.push(input);
                        state.current_input.clear();
                    }
                }
                _ => {}
            },
        }
        return;
    }

    let count = app.config.startup_commands.len();

    match code {
        KeyCode::Esc | KeyCode::Char('q') => {
            app.startup_cmds_open = false;
        }
        KeyCode::Char('j') | KeyCode::Down => {
            if count > 0 {
                app.startup_cmds_selected = (app.startup_cmds_selected + 1) % count;
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if count > 0 {
                if app.startup_cmds_selected == 0 {
                    app.startup_cmds_selected = count - 1;
                } else {
                    app.startup_cmds_selected -= 1;
                }
            }
        }
        KeyCode::Char('a') => {
            let start_dir = app
                .config
                .default_spawn_dir
                .clone()
                .or_else(|| {
                    std::env::current_dir().ok().and_then(|p| p.to_str().map(|s| s.to_owned()))
                })
                .unwrap_or_else(|| "/".to_owned());
            match Browser::new_simple(std::path::PathBuf::from(&start_dir)) {
                Ok(browser) => {
                    app.startup_cmds_adding = Some(StartupCmdAddState {
                        step: StartupCmdAddStep::BrowsePath,
                        browser,
                        path: String::new(),
                        commands: Vec::new(),
                        current_input: String::new(),
                    });
                }
                Err(err) => {
                    app.status_line = format!("Cannot open path browser: {err}");
                }
            }
        }
        KeyCode::Char('x') => {
            if count > 0 && app.startup_cmds_selected < count {
                app.config.startup_commands.remove(app.startup_cmds_selected);
                if app.startup_cmds_selected >= app.config.startup_commands.len()
                    && app.startup_cmds_selected > 0
                {
                    app.startup_cmds_selected -= 1;
                }
                match config::save_config(&app.config) {
                    Ok(()) => app.status_line = "Startup command rule removed".to_owned(),
                    Err(e) => app.status_line = format!("Save failed: {e}"),
                }
            }
        }
        _ => {}
    }
}

pub fn handle_dev_servers_key(app: &mut App, code: KeyCode) {
    // If we're in the add flow, handle that first.
    if let Some(ref mut state) = app.dev_servers_adding {
        match state.step {
            DevServerAddStep::BrowsePath => match code {
                KeyCode::Esc => {
                    app.dev_servers_adding = None;
                }
                KeyCode::Char('j') | KeyCode::Down => state.browser.next(),
                KeyCode::Char('k') | KeyCode::Up => state.browser.previous(),
                KeyCode::PageDown => {
                    for _ in 0..10 {
                        state.browser.next();
                    }
                }
                KeyCode::PageUp => {
                    for _ in 0..10 {
                        state.browser.previous();
                    }
                }
                KeyCode::Enter => match state.browser.activate_selected() {
                    Ok(ActivateResult::Selected(path)) => {
                        state.path = path.to_string_lossy().to_string();
                        state.step = DevServerAddStep::Command;
                    }
                    Ok(ActivateResult::StartTypePath) => {
                        state.current_input = state.browser.cwd().to_string_lossy().to_string();
                        state.step = DevServerAddStep::TypePath;
                    }
                    Ok(ActivateResult::ChangedDirectory) => {}
                    Ok(_) => {}
                    Err(err) => {
                        app.status_line = format!("Path navigation failed: {err}");
                    }
                },
                _ => {}
            },
            DevServerAddStep::TypePath => match code {
                KeyCode::Esc => {
                    state.step = DevServerAddStep::BrowsePath;
                    state.current_input.clear();
                }
                KeyCode::Backspace => {
                    state.current_input.pop();
                }
                KeyCode::Char(c) => {
                    if !c.is_control() {
                        state.current_input.push(c);
                    }
                }
                KeyCode::Enter => {
                    let input = state.current_input.trim().to_owned();
                    let home = std::env::var("HOME").unwrap_or_default();
                    let expanded = if input.starts_with('~') {
                        input.replacen('~', &home, 1)
                    } else {
                        input.clone()
                    };
                    let path = std::path::Path::new(&expanded);
                    if path.is_dir() {
                        match state.browser.navigate_to(path) {
                            Ok(()) => {
                                state.step = DevServerAddStep::BrowsePath;
                                state.current_input.clear();
                            }
                            Err(err) => {
                                app.status_line = format!("Cannot navigate to path: {err}");
                            }
                        }
                    } else {
                        app.status_line = "Not a valid directory".to_owned();
                    }
                }
                _ => {}
            },
            DevServerAddStep::Command => match code {
                KeyCode::Esc => {
                    if state.current_input.is_empty() {
                        app.dev_servers_adding = None;
                    } else {
                        state.current_input.clear();
                    }
                }
                KeyCode::Backspace => {
                    state.current_input.pop();
                }
                KeyCode::Char(c) => {
                    state.current_input.push(c);
                }
                KeyCode::Enter => {
                    let input = state.current_input.trim().to_owned();
                    if input.is_empty() {
                        app.dev_servers_adding = None;
                        return;
                    }
                    let entry =
                        config::DevServerConfig { path: state.path.clone(), command: input };
                    app.config.dev_servers.push(entry);
                    app.dev_servers_adding = None;
                    match config::save_config(&app.config) {
                        Ok(()) => app.status_line = "Dev server rule added".to_owned(),
                        Err(e) => app.status_line = format!("Save failed: {e}"),
                    }
                }
                _ => {}
            },
        }
        return;
    }

    let count = app.config.dev_servers.len();

    match code {
        KeyCode::Esc | KeyCode::Char('q') => {
            app.dev_servers_open = false;
        }
        KeyCode::Char('j') | KeyCode::Down => {
            if count > 0 {
                app.dev_servers_selected = (app.dev_servers_selected + 1) % count;
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if count > 0 {
                if app.dev_servers_selected == 0 {
                    app.dev_servers_selected = count - 1;
                } else {
                    app.dev_servers_selected -= 1;
                }
            }
        }
        KeyCode::Char('a') => {
            let start_dir = app
                .config
                .default_spawn_dir
                .clone()
                .or_else(|| {
                    std::env::current_dir().ok().and_then(|p| p.to_str().map(|s| s.to_owned()))
                })
                .unwrap_or_else(|| "/".to_owned());
            match Browser::new_simple(std::path::PathBuf::from(&start_dir)) {
                Ok(browser) => {
                    app.dev_servers_adding = Some(DevServerAddState {
                        step: DevServerAddStep::BrowsePath,
                        browser,
                        path: String::new(),
                        current_input: String::new(),
                    });
                }
                Err(err) => {
                    app.status_line = format!("Cannot open path browser: {err}");
                }
            }
        }
        KeyCode::Char('x') => {
            if count > 0 && app.dev_servers_selected < count {
                app.config.dev_servers.remove(app.dev_servers_selected);
                if app.dev_servers_selected >= app.config.dev_servers.len()
                    && app.dev_servers_selected > 0
                {
                    app.dev_servers_selected -= 1;
                }
                match config::save_config(&app.config) {
                    Ok(()) => app.status_line = "Dev server rule removed".to_owned(),
                    Err(e) => app.status_line = format!("Save failed: {e}"),
                }
            }
        }
        _ => {}
    }
}

pub fn handle_permissions_key(app: &mut App, code: KeyCode) {
    let count = app.available_agents.len();

    match code {
        KeyCode::Esc | KeyCode::Char('q') => {
            app.permissions_open = false;
        }
        KeyCode::Char('j') | KeyCode::Down => {
            if count > 0 {
                app.permissions_selected = (app.permissions_selected + 1) % count;
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if count > 0 {
                if app.permissions_selected == 0 {
                    app.permissions_selected = count - 1;
                } else {
                    app.permissions_selected -= 1;
                }
            }
        }
        KeyCode::Enter => {
            if let Some(agent) = app.available_agents.get(app.permissions_selected) {
                if agent.bypass_flag.is_some() {
                    let current = config::is_bypass_enabled(&app.config, &agent.id);
                    app.config.permissions_bypass.insert(agent.id.clone(), !current);
                    match config::save_config(&app.config) {
                        Ok(()) => app.status_line = "Permissions saved".to_owned(),
                        Err(e) => app.status_line = format!("Save failed: {e}"),
                    }
                }
            }
        }
        _ => {}
    }
}

pub fn handle_channels_key(app: &mut App, code: KeyCode) {
    // If adding a channel, handle text input.
    if let Some(ref mut buf) = app.channels_adding {
        match code {
            KeyCode::Esc => {
                app.channels_adding = None;
            }
            KeyCode::Enter => {
                let channel = buf.trim().to_owned();
                if !channel.is_empty() {
                    let r = ensure_router_config(&mut app.config);
                    r.channels.push(channel);
                    match config::save_config(&app.config) {
                        Ok(()) => app.status_line = "Channel added".to_owned(),
                        Err(e) => app.status_line = format!("Save failed: {e}"),
                    }
                }
                app.channels_adding = None;
            }
            KeyCode::Backspace => {
                buf.pop();
            }
            KeyCode::Char(c) => {
                buf.push(c);
            }
            _ => {}
        }
        return;
    }

    let count = app.config.router.as_ref().map(|r| r.channels.len()).unwrap_or(0);

    match code {
        KeyCode::Esc | KeyCode::Char('q') => {
            app.channels_open = false;
        }
        KeyCode::Char('j') | KeyCode::Down => {
            if count > 0 {
                app.channels_selected = (app.channels_selected + 1) % count;
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if count > 0 {
                if app.channels_selected == 0 {
                    app.channels_selected = count - 1;
                } else {
                    app.channels_selected -= 1;
                }
            }
        }
        KeyCode::Enter | KeyCode::Char('a') => {
            app.channels_adding = Some(String::new());
        }
        KeyCode::Char('x') => {
            if let Some(ref mut r) = app.config.router {
                if app.channels_selected < r.channels.len() {
                    r.channels.remove(app.channels_selected);
                    if app.channels_selected > 0 && app.channels_selected >= r.channels.len() {
                        app.channels_selected = r.channels.len().saturating_sub(1);
                    }
                    match config::save_config(&app.config) {
                        Ok(()) => app.status_line = "Channel removed".to_owned(),
                        Err(e) => app.status_line = format!("Save failed: {e}"),
                    }
                }
            }
        }
        _ => {}
    }
}

pub fn attach_into_session(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    name: &str,
) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;

    let attach_result = tmux::attach_session(name);

    execute!(terminal.backend_mut(), EnterAlternateScreen, EnableMouseCapture)?;
    enable_raw_mode()?;
    terminal.hide_cursor()?;
    terminal.clear()?;

    attach_result
}

// Router settings sub-view has 4 fields:
// 0: enabled (bool toggle)
// 1: agent (text)
// 2: working_dir (text)
// 3: auto_restart (bool toggle)
const ROUTER_SETTINGS_COUNT: usize = 4;

pub fn router_setting_label(index: usize) -> &'static str {
    match index {
        0 => "Enabled",
        1 => "Agent",
        2 => "Working dir",
        3 => "Auto restart",
        _ => "",
    }
}

pub fn router_setting_value(router: &Option<config::RouterConfig>, index: usize) -> String {
    let r = match router {
        Some(r) => r,
        None => {
            return match index {
                0 => "off".to_owned(),
                1 => "claude".to_owned(),
                2 => "~".to_owned(),
                3 => "off".to_owned(),
                _ => String::new(),
            }
        }
    };
    match index {
        0 => if r.enabled { "on" } else { "off" }.to_owned(),
        1 => r.agent.clone(),
        2 => r.working_dir.clone().unwrap_or_else(|| "~".to_owned()),
        3 => if r.auto_restart { "on" } else { "off" }.to_owned(),
        _ => String::new(),
    }
}

pub fn router_setting_is_bool(index: usize) -> bool {
    matches!(index, 0 | 3)
}

fn ensure_router_config(cfg: &mut config::AppConfig) -> &mut config::RouterConfig {
    if cfg.router.is_none() {
        cfg.router = Some(config::RouterConfig {
            enabled: false,
            agent: "claude".to_owned(),
            channels: Vec::new(),
            working_dir: None,
            auto_restart: true,
        });
    }
    cfg.router.as_mut().unwrap()
}

pub fn handle_router_settings_key(app: &mut App, code: KeyCode) {
    if let Some(ref mut buf) = app.router_settings_editing {
        match code {
            KeyCode::Esc => {
                app.router_settings_editing = None;
            }
            KeyCode::Enter => {
                let value = buf.clone();
                let idx = app.router_settings_selected;
                let r = ensure_router_config(&mut app.config);
                match idx {
                    1 => r.agent = value,
                    2 => {
                        if value.is_empty() {
                            r.working_dir = None;
                        } else {
                            r.working_dir = Some(value);
                        }
                    }
                    _ => {}
                }
                app.router_settings_editing = None;
                match config::save_config(&app.config) {
                    Ok(()) => app.status_line = "Router settings saved".to_owned(),
                    Err(e) => app.status_line = format!("Save failed: {e}"),
                }
            }
            KeyCode::Backspace => {
                buf.pop();
            }
            KeyCode::Char(c) => {
                buf.push(c);
            }
            _ => {}
        }
        return;
    }

    match code {
        KeyCode::Esc | KeyCode::Char('q') => {
            app.router_settings_open = false;
        }
        KeyCode::Char('j') | KeyCode::Down => {
            app.router_settings_selected =
                (app.router_settings_selected + 1) % ROUTER_SETTINGS_COUNT;
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if app.router_settings_selected == 0 {
                app.router_settings_selected = ROUTER_SETTINGS_COUNT - 1;
            } else {
                app.router_settings_selected -= 1;
            }
        }
        KeyCode::Enter => {
            let idx = app.router_settings_selected;
            if router_setting_is_bool(idx) {
                let r = ensure_router_config(&mut app.config);
                match idx {
                    0 => {
                        r.enabled = !r.enabled;
                        let now_enabled = r.enabled;
                        // Kill the router session when disabling
                        if !now_enabled {
                            let _ = crate::tmux::kill_session(
                                crate::router::ROUTER_SESSION_NAME,
                            );
                            app.router_alive = false;
                        }
                    }
                    3 => r.auto_restart = !r.auto_restart,
                    _ => {}
                }
                match config::save_config(&app.config) {
                    Ok(()) => {
                        let msg = match idx {
                            0 if ensure_router_config(&mut app.config).enabled => {
                                "Router enabled".to_owned()
                            }
                            0 => "Router disabled and stopped".to_owned(),
                            _ => "Router settings saved".to_owned(),
                        };
                        app.status_line = msg;
                    }
                    Err(e) => app.status_line = format!("Save failed: {e}"),
                }
            } else {
                app.router_settings_editing =
                    Some(router_setting_value(&app.config.router, idx));
            }
        }
        _ => {}
    }
}
