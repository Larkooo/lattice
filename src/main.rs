use std::{
    io::{self, Stdout},
    time::Duration,
};

use anyhow::Result;
use clap::Parser;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use lattice::{
    app::{App, AppScreen},
    config,
    handlers::{
        handle_main_key, handle_modal_key, handle_permissions_key, handle_settings_key,
        handle_startup_cmds_key, handle_warning_key,
    },
    ui::draw_ui,
};

#[derive(Parser, Debug)]
#[command(author, version, about = "Tmux-backed TUI for managing coding agents")]
struct Cli {
    #[arg(long, help = "Auto refresh interval in seconds")]
    refresh_seconds: Option<u64>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut cfg = config::load_config();
    config::apply_cli_overrides(&mut cfg, cli.refresh_seconds);
    run(cfg)
}

fn run(cfg: config::AppConfig) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;

    let mut app = App::new(cfg.clone());
    config::spawn_activity_monitor(&cfg);
    app.refresh();

    let loop_result = run_loop(&mut terminal, &mut app);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;

    loop_result
}

fn run_loop(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &mut App) -> Result<()> {
    while !app.should_quit {
        // Check for completed background stop operations
        app.drain_stop_results();

        terminal.draw(|frame| draw_ui(frame, app))?;

        // Poll more frequently when sessions are being stopped so the UI
        // updates promptly when the background work finishes.
        let max_wait = if app.stopping_sessions.is_empty() {
            Duration::from_millis(250)
        } else {
            Duration::from_millis(100)
        };

        let until_refresh =
            app.refresh_interval.saturating_sub(app.last_refresh.elapsed()).min(max_wait);

        if event::poll(until_refresh)? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    if app.screen == AppScreen::Warning {
                        handle_warning_key(app, key.code);
                    } else if app.modal.is_some() {
                        handle_modal_key(app, key.code);
                    } else if app.startup_cmds_open {
                        handle_startup_cmds_key(app, key.code);
                    } else if app.permissions_open {
                        handle_permissions_key(app, key.code);
                    } else if app.settings_open {
                        handle_settings_key(app, key.code);
                    } else {
                        handle_main_key(terminal, app, key.code, key.modifiers)?;
                    }
                }
                Event::Resize(_, _) => {}
                _ => {}
            }
        }

        if app.last_refresh.elapsed() >= app.refresh_interval {
            app.refresh();
        }
    }

    Ok(())
}
