mod settings;

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use std::time::{SystemTime, UNIX_EPOCH};

use crate::{
    agents,
    app::{instance_project_name, App, AppScreen, HeaderTabRegion, SpawnStep},
    config, git,
    pathnav::EntryKind,
};
use settings::{
    draw_channels_view, draw_dev_servers_view, draw_permissions_view, draw_router_settings_view,
    draw_settings_view, draw_startup_cmds_view,
};

/// Format an epoch timestamp as a human-friendly relative duration (e.g. "3m", "1h 23m").
fn format_uptime(created_epoch: u64) -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    if now <= created_epoch {
        return "just now".to_owned();
    }
    let secs = now - created_epoch;
    let mins = secs / 60;
    let hours = mins / 60;
    let days = hours / 24;
    if days > 0 {
        format!("{}d {}h", days, hours % 24)
    } else if hours > 0 {
        format!("{}h {}m", hours, mins % 60)
    } else if mins > 0 {
        format!("{}m", mins)
    } else {
        format!("{}s", secs)
    }
}

pub fn draw_ui(frame: &mut ratatui::Frame<'_>, app: &App) {
    let t = app.theme;

    frame.render_widget(Block::default().style(Style::default().bg(t.bg)), frame.area());

    match app.screen {
        AppScreen::Warning => draw_warning_screen(frame, app),
        AppScreen::Main => draw_main_screen(frame, app),
    }
}

fn draw_warning_screen(frame: &mut ratatui::Frame<'_>, app: &App) {
    let t = app.theme;
    let container = centered_rect(60, 96, frame.area());

    let Some(warning) = &app.warning else { return };

    // Center the warning vertically
    let vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(30), Constraint::Min(12), Constraint::Percentage(40)])
        .split(container);

    let area = vert[1];

    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            format!("  ! {}", warning.title),
            Style::default().fg(t.yellow).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(format!("  {}", warning.message), Style::default().fg(t.text))),
        Line::from(""),
    ];

    for detail in &warning.details {
        lines.push(Line::from(Span::styled(format!("  {detail}"), Style::default().fg(t.muted))));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("  press ", Style::default().fg(t.muted)),
        Span::styled("r", Style::default().fg(t.text).add_modifier(Modifier::BOLD)),
        Span::styled(" to retry    ", Style::default().fg(t.muted)),
        Span::styled("q", Style::default().fg(t.text).add_modifier(Modifier::BOLD)),
        Span::styled(" to quit", Style::default().fg(t.muted)),
    ]));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(t.yellow))
        .style(Style::default().bg(t.bg))
        .title(Line::from(vec![Span::styled(
            " lattice ",
            Style::default().fg(t.yellow).add_modifier(Modifier::BOLD),
        )]));

    frame.render_widget(
        Paragraph::new(Text::from(lines)).style(Style::default().bg(t.bg)).block(block),
        area,
    );
}

fn draw_main_screen(frame: &mut ratatui::Frame<'_>, app: &App) {
    let container = centered_rect(80, 96, frame.area());

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header tabs
            Constraint::Length(1), // spacer
            Constraint::Min(6),    // content
            Constraint::Length(1), // spacer
            Constraint::Length(1), // status message
            Constraint::Length(1), // horizontal rule
            Constraint::Length(1), // keybindings
        ])
        .split(container);

    draw_header(frame, sections[0], app);

    if app.startup_cmds_open {
        draw_startup_cmds_view(frame, sections[2], app);
    } else if app.dev_servers_open {
        draw_dev_servers_view(frame, sections[2], app);
    } else if app.channels_open {
        draw_channels_view(frame, sections[2], app);
    } else if app.router_settings_open {
        draw_router_settings_view(frame, sections[2], app);
    } else if app.permissions_open {
        draw_permissions_view(frame, sections[2], app);
    } else if app.settings_open {
        draw_settings_view(frame, sections[2], app);
    } else if app.selected_tab == 0 {
        draw_dashboard(frame, sections[2], app);
    } else {
        draw_instance_tab(frame, sections[2], app);
    }

    draw_status_line(frame, sections[4], app);
    draw_footer_rule(frame, sections[5], app);
    draw_footer(frame, sections[6], app);

    if app.modal.is_some() {
        draw_spawn_modal(frame, app);
    }
}

/// Renders the header as a connected bordered table row:
/// ┌──────────┬──────────┬──────────┐
/// │ lattice │  s shop  │  a acct  │
/// └──────────┴──────────┴──────────┘
fn draw_header(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
    let t = app.theme;
    let w = area.width as usize;

    // Cell 0 = "lattice" brand (maps to dashboard / tab 0)
    // Cell 1+ = instance tabs
    struct TabCell {
        label: String,
        full_title: String,
        is_selected: bool,
        is_in_split: bool,
    }

    // Collect split pane session names for header highlighting
    let split_names: Vec<String> = app
        .split
        .as_ref()
        .map(|s| s.panes.iter().map(|p| p.session_name.clone()).collect())
        .unwrap_or_default();

    let mut cells: Vec<TabCell> = Vec::new();
    cells.push(TabCell {
        label: "lattice".to_owned(),
        full_title: "lattice".to_owned(),
        is_selected: app.selected_tab == 0,
        is_in_split: false,
    });
    for (i, instance) in app.instances.iter().enumerate() {
        let title = agents::derive_display_title(
            &instance.session.name,
            &instance.session.pane_title,
            &instance.session.pane_current_path,
            &instance.title_override,
        );
        let display = truncate(&title, 14);
        let in_split = split_names.contains(&instance.session.name);
        cells.push(TabCell {
            label: display,
            full_title: title,
            is_selected: app.selected_tab == i + 1,
            is_in_split: in_split,
        });
    }

    let n = cells.len();
    if n == 0 || w < n + 1 {
        return;
    }

    // Calculate column widths (content only, not including border chars)
    let available = w.saturating_sub(n + 1);
    let base = available / n;
    let extra = available % n;
    let mut col_widths: Vec<usize> = vec![base; n];
    for w in col_widths.iter_mut().take(extra) {
        *w += 1;
    }

    let border_style = Style::default().fg(t.border);

    // Top border: ┌───┬───┬───┐
    let mut top_spans: Vec<Span> = vec![Span::styled("\u{250c}", border_style)];
    for (i, &cw) in col_widths.iter().enumerate() {
        top_spans.push(Span::styled("\u{2500}".repeat(cw), border_style));
        if i < n - 1 {
            top_spans.push(Span::styled("\u{252c}", border_style));
        } else {
            top_spans.push(Span::styled("\u{2510}", border_style));
        }
    }

    // Content: │ label │ label │
    let mut mid_spans: Vec<Span> = Vec::new();
    let tab_regions: Vec<HeaderTabRegion> = Vec::new();
    let mut col_x = area.x.saturating_add(1);
    for (i, cell) in cells.iter().enumerate() {
        mid_spans.push(Span::styled("\u{2502}", border_style));

        let cw = col_widths[i];
        let full_width = unicode_width::UnicodeWidthStr::width(cell.full_title.as_str());
        let display_label = if full_width > cw {
            let max_offset = full_width - cw;
            // Auto-scroll: pause → scroll right → pause → scroll left → repeat
            let speed = TICKER_SPEED;
            let pause = TICKER_PAUSE;
            let scroll_ticks = max_offset as u64 * speed;
            let cycle = pause + scroll_ticks + pause + scroll_ticks;
            let phase = app.tick % cycle;
            let offset = if phase < pause {
                0
            } else if phase < pause + scroll_ticks {
                ((phase - pause) / speed) as usize
            } else if phase < pause + scroll_ticks + pause {
                max_offset
            } else {
                max_offset - ((phase - pause - scroll_ticks - pause) / speed) as usize
            };
            app.ticker_active.set(true);
            scroll_str(&cell.full_title, cw, offset)
        } else if unicode_width::UnicodeWidthStr::width(cell.label.as_str()) > cw {
            truncate(&cell.label, cw)
        } else {
            cell.label.clone()
        };
        let label_len = unicode_width::UnicodeWidthStr::width(display_label.as_str());
        let pad_total = cw.saturating_sub(label_len);
        let pad_left = pad_total / 2;
        let pad_right = pad_total - pad_left;

        let text_style = if cell.is_in_split && cell.is_selected {
            Style::default().fg(t.accent).add_modifier(Modifier::BOLD)
        } else if cell.is_in_split {
            Style::default().fg(t.accent)
        } else if cell.is_selected {
            Style::default().fg(t.text).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(t.muted)
        };

        mid_spans.push(Span::styled(" ".repeat(pad_left), Style::default()));
        mid_spans.push(Span::styled(display_label, text_style));
        mid_spans.push(Span::styled(" ".repeat(pad_right), Style::default()));
        col_x = col_x.saturating_add(cw as u16).saturating_add(1);
    }
    mid_spans.push(Span::styled("\u{2502}", border_style));
    app.set_header_tab_regions(n, tab_regions);

    // Bottom border: └───┴───┴───┘
    let mut bot_spans: Vec<Span> = vec![Span::styled("\u{2514}", border_style)];
    for (i, &cw) in col_widths.iter().enumerate() {
        bot_spans.push(Span::styled("\u{2500}".repeat(cw), border_style));
        if i < n - 1 {
            bot_spans.push(Span::styled("\u{2534}", border_style));
        } else {
            bot_spans.push(Span::styled("\u{2518}", border_style));
        }
    }

    let text =
        Text::from(vec![Line::from(top_spans), Line::from(mid_spans), Line::from(bot_spans)]);

    frame.render_widget(Paragraph::new(text).style(Style::default().bg(t.bg)), area);
}

fn draw_dashboard(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(28),
            Constraint::Length(2),
            Constraint::Percentage(70),
        ])
        .split(area);

    draw_instance_list(frame, chunks[0], app);
    draw_summary_panel(frame, chunks[2], app);
}

fn draw_instance_list(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
    let t = app.theme;

    let mut lines: Vec<Line> = Vec::new();

    let total = app.dashboard_row_count();
    let capacity = area.height.saturating_sub(4) as usize;
    let (start, end) = visible_range(total, app.selected_row, capacity.max(1));

    // Show router status at the top if enabled
    if app.is_router_enabled() {
        let (label, style) = if app.router_alive {
            ("\u{25C9} router".to_owned(), Style::default().fg(t.green))
        } else if app.router_spawning {
            ("\u{25C9} router starting\u{2026}".to_owned(), Style::default().fg(t.yellow))
        } else {
            ("\u{25C9} router offline".to_owned(), Style::default().fg(t.red))
        };
        lines.push(Line::from(Span::styled(label, style)));
        lines.push(Line::from(""));
    }

    if start > 0 {
        lines.push(Line::from(Span::styled("...", Style::default().fg(t.muted))));
    }

    // Returns the project name for any dashboard row that has one (live or
    // dormant). Returns None for the action/settings rows so we don't try to
    // render a header for them.
    let live_count = app.instances.len();
    let dormant_count = app.dormant_instances.len();
    let project_at = |idx: usize| -> Option<String> {
        if idx < live_count {
            Some(instance_project_name(&app.instances[idx]))
        } else if idx < live_count + dormant_count {
            Some(app.dormant_instances[idx - live_count].project_name())
        } else {
            None
        }
    };

    // Track which project header was last rendered so we insert one on change.
    let prev_project: Option<String> =
        if start > 0 { project_at(start - 1) } else { None };
    let mut last_project = prev_project;

    for index in start..end {
        let selected = index == app.selected_row;

        if index < live_count {
            let instance = &app.instances[index];
            let project = instance_project_name(instance);

            if last_project.as_deref() != Some(&project) {
                if last_project.is_some() {
                    lines.push(Line::from(""));
                }
                let header = if project.is_empty() {
                    "~ unknown ~".to_owned()
                } else {
                    format!("~ {project} ~")
                };
                lines.push(Line::from(Span::styled(header, Style::default().fg(t.accent))));
                last_project = Some(project);
            }

            let title = agents::derive_display_title(
                &instance.session.name,
                &instance.session.pane_title,
                &instance.session.pane_current_path,
                &instance.title_override,
            );

            let is_stopping = app.stopping_sessions.contains(&instance.session.name);

            let fit = |s: &str, max: usize| -> String { truncate(s, max) };

            let (label, style) = if is_stopping {
                let label = format!("\u{29D7} {} stopping\u{2026}", fit(&title, 18));
                let style = if selected {
                    Style::default().fg(t.bg).bg(t.highlight_bg).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(t.yellow)
                };
                (label, style)
            } else if instance
                .pr_checks
                .as_ref()
                .map(|checks| checks.has_failures())
                .unwrap_or(false)
            {
                let label = format!("\u{2717} {}", fit(&title, 26));
                let style = if selected {
                    Style::default().fg(t.bg).bg(t.highlight_bg).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(t.red)
                };
                (label, style)
            } else if instance.pr_state == Some(git::PrState::Merged) {
                let label = format!("\u{21B3} {}", fit(&title, 26));
                let style = if selected {
                    Style::default().fg(t.bg).bg(t.highlight_bg).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(t.accent)
                };
                (label, style)
            } else if instance.pr_state == Some(git::PrState::Open) {
                let label = format!("\u{2197} {}", fit(&title, 26));
                let style = if selected {
                    Style::default().fg(t.bg).bg(t.highlight_bg).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(t.yellow)
                };
                (label, style)
            } else if instance.completed {
                let label = format!("\u{2713} {}", fit(&title, 26));
                let style = if selected {
                    Style::default().fg(t.bg).bg(t.highlight_bg).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(t.green)
                };
                (label, style)
            } else {
                let label = fit(&title, 28);
                let style = if selected {
                    Style::default().fg(t.bg).bg(t.highlight_bg).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(t.text)
                };
                (label, style)
            };

            lines.push(Line::from(Span::styled(label, style)));
        } else if index < live_count + dormant_count {
            // Dormant worktree row — survivor of a reboot or quit.
            let dormant = &app.dormant_instances[index - live_count];
            let project = dormant.project_name();

            if last_project.as_deref() != Some(&project) {
                if last_project.is_some() {
                    lines.push(Line::from(""));
                }
                let header = if project.is_empty() {
                    "~ unknown ~".to_owned()
                } else {
                    format!("~ {project} ~")
                };
                lines.push(Line::from(Span::styled(header, Style::default().fg(t.accent))));
                last_project = Some(project);
            }

            let title = dormant.display_title();
            let resumable = dormant.claude_session_id.is_some();
            let label = if resumable {
                format!("z {} (resumable)", truncate(&title, 18))
            } else {
                format!("z {}", truncate(&title, 28))
            };
            let style = if selected {
                Style::default().fg(t.bg).bg(t.highlight_bg).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(t.muted)
            };
            lines.push(Line::from(Span::styled(label, style)));
        } else if index == app.action_row_index() {
            // "New Instance" action row
            if !lines.is_empty() {
                lines.push(Line::from(""));
            }
            let style = if selected {
                Style::default().fg(t.bg).bg(t.highlight_bg).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(t.accent)
            };
            lines.push(Line::from(Span::styled("+ new instance", style)));
        } else {
            // "Settings" row
            let style = if selected {
                Style::default().fg(t.bg).bg(t.highlight_bg).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(t.accent)
            };
            lines.push(Line::from(Span::styled("# settings", style)));
        }
    }

    if end < total {
        lines.push(Line::from(Span::styled("...", Style::default().fg(t.muted))));
    }

    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .style(Style::default().bg(t.bg))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn draw_summary_panel(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
    let t = app.theme;

    let lines = if app.is_settings_row_selected() {
        let c = &app.config;
        vec![
            Line::from(Span::styled(
                "settings",
                Style::default().fg(t.text).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled("Press enter to edit settings.", Style::default().fg(t.text))),
            Line::from(""),
            Line::from(vec![
                Span::styled("config   ", Style::default().fg(t.muted)),
                Span::styled(
                    format!("{}", config::config_path().display()),
                    Style::default().fg(t.text),
                ),
            ]),
            Line::from(""),
            Line::from(Span::styled("~ current values ~", Style::default().fg(t.accent))),
            Line::from(vec![
                Span::styled("refresh interval       ", Style::default().fg(t.muted)),
                Span::styled(format!("{}s", c.refresh_interval), Style::default().fg(t.text)),
            ]),
            Line::from(vec![
                Span::styled("default spawn dir      ", Style::default().fg(t.muted)),
                Span::styled(
                    c.default_spawn_dir.as_deref().unwrap_or("(none)").to_owned(),
                    Style::default().fg(t.text),
                ),
            ]),
            Line::from(vec![
                Span::styled("title injection        ", Style::default().fg(t.muted)),
                Span::styled(
                    if c.title_injection_enabled { "on" } else { "off" },
                    if c.title_injection_enabled {
                        Style::default().fg(t.green)
                    } else {
                        Style::default().fg(t.muted)
                    },
                ),
            ]),
            Line::from(vec![
                Span::styled("title injection delay  ", Style::default().fg(t.muted)),
                Span::styled(format!("{}s", c.title_injection_delay), Style::default().fg(t.text)),
            ]),
            Line::from(vec![
                Span::styled("git worktrees          ", Style::default().fg(t.muted)),
                Span::styled(
                    if c.git_worktrees { "on" } else { "off" },
                    if c.git_worktrees {
                        Style::default().fg(t.green)
                    } else {
                        Style::default().fg(t.muted)
                    },
                ),
            ]),
            Line::from(vec![
                Span::styled("sound on completion    ", Style::default().fg(t.muted)),
                Span::styled(
                    if c.notifications.sound_on_completion { "on" } else { "off" },
                    if c.notifications.sound_on_completion {
                        Style::default().fg(t.green)
                    } else {
                        Style::default().fg(t.muted)
                    },
                ),
            ]),
            Line::from(vec![
                Span::styled("sound method           ", Style::default().fg(t.muted)),
                Span::styled(
                    match c.notifications.sound_method {
                        config::SoundMethod::Bell => "bell",
                        config::SoundMethod::Command => "command",
                    },
                    Style::default().fg(t.text),
                ),
            ]),
            Line::from(vec![
                Span::styled("sound command          ", Style::default().fg(t.muted)),
                Span::styled(c.notifications.sound_command.clone(), Style::default().fg(t.text)),
            ]),
        ]
    } else if let Some(dormant) = app.selected_dormant() {
        let mut l = vec![
            Line::from(Span::styled(
                dormant.display_title(),
                Style::default().fg(t.text).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled("dormant worktree", Style::default().fg(t.muted))),
            Line::from(""),
        ];

        let resumable = dormant.claude_session_id.is_some();
        let (state_label, state_style) = if resumable {
            ("resumable (claude transcript found)", Style::default().fg(t.green))
        } else {
            ("no claude transcript — would launch fresh", Style::default().fg(t.muted))
        };
        l.push(Line::from(vec![
            Span::styled("state    ", Style::default().fg(t.muted)),
            Span::styled(state_label, state_style),
        ]));

        if !dormant.branch.is_empty() {
            l.push(Line::from(vec![
                Span::styled("branch   ", Style::default().fg(t.muted)),
                Span::styled(dormant.branch.clone(), Style::default().fg(t.text)),
            ]));
        }

        l.push(Line::from(vec![
            Span::styled("path     ", Style::default().fg(t.muted)),
            Span::styled(
                dormant.worktree_path.to_string_lossy().into_owned(),
                Style::default().fg(t.text),
            ),
        ]));

        if let Some(ref id) = dormant.claude_session_id {
            l.push(Line::from(vec![
                Span::styled("session  ", Style::default().fg(t.muted)),
                Span::styled(id.clone(), Style::default().fg(t.text)),
            ]));
        }

        l.push(Line::from(""));
        if resumable {
            l.push(Line::from(Span::styled(
                "Press enter to resume Claude in this worktree.",
                Style::default().fg(t.text),
            )));
        } else {
            l.push(Line::from(Span::styled(
                "No transcript to resume. Press x to remove the worktree,",
                Style::default().fg(t.text),
            )));
            l.push(Line::from(Span::styled(
                "or use + new instance to start fresh in this dir.",
                Style::default().fg(t.text),
            )));
        }
        l.push(Line::from(""));
        l.push(Line::from(Span::styled(
            "x  remove worktree",
            Style::default().fg(t.muted),
        )));

        l
    } else if app.is_action_row_selected()
        || (app.instances.is_empty() && app.dormant_instances.is_empty())
    {
        let mut l = vec![
            Line::from(Span::styled(
                "new instance",
                Style::default().fg(t.text).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Press enter to launch the spawn wizard.",
                Style::default().fg(t.text),
            )),
            Line::from(Span::styled(
                "Select an agent CLI, pick a working directory,",
                Style::default().fg(t.text),
            )),
            Line::from(Span::styled(
                "and a new tmux session will be created.",
                Style::default().fg(t.text),
            )),
            Line::from(""),
        ];

        if !app.available_agents.is_empty() {
            l.push(Line::from(Span::styled("~ detected agents ~", Style::default().fg(t.accent))));
            for agent in &app.available_agents {
                l.push(Line::from(vec![
                    Span::styled(
                        agent.id.to_string(),
                        Style::default().fg(t.text).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(format!("  {}", agent.label), Style::default().fg(t.muted)),
                ]));
            }
        }
        l
    } else if let Some(instance) = app.selected_instance() {
        let (state_label, state_style) = instance_state_label(app, instance);

        let title = agents::derive_display_title(
            &instance.session.name,
            &instance.session.pane_title,
            &instance.session.pane_current_path,
            &instance.title_override,
        );

        let mut lines = vec![
            Line::from(Span::styled(
                title,
                Style::default().fg(t.text).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(instance.agent.label.clone(), Style::default().fg(t.muted))),
            Line::from(""),
            Line::from(vec![
                Span::styled("state    ", Style::default().fg(t.muted)),
                Span::styled(state_label, state_style),
            ]),
        ];

        if let Some(pr_num) = instance.pr_number {
            let pr_style = match instance.pr_state {
                Some(git::PrState::Merged) => Style::default().fg(t.accent),
                Some(git::PrState::Open) => Style::default().fg(t.yellow),
                Some(git::PrState::Closed) => Style::default().fg(t.red),
                None => Style::default().fg(t.text),
            };
            lines.push(Line::from(vec![
                Span::styled("pr       ", Style::default().fg(t.muted)),
                Span::styled(format!("#{pr_num}"), pr_style),
            ]));
        }

        if let Some(checks) = instance.pr_checks.as_ref() {
            if let Some(ci_label) = checks.short_label() {
                let ci_style = if checks.has_failures() {
                    Style::default().fg(t.red)
                } else if checks.has_pending() {
                    Style::default().fg(t.yellow)
                } else {
                    Style::default().fg(t.green)
                };
                lines.push(Line::from(vec![
                    Span::styled("ci       ", Style::default().fg(t.muted)),
                    Span::styled(ci_label, ci_style),
                ]));
                if checks.has_failures() {
                    lines.push(Line::from(vec![
                        Span::styled("checks   ", Style::default().fg(t.muted)),
                        Span::styled(checks.failed.join(", "), Style::default().fg(t.text)),
                    ]));
                }
            }
        }

        if !instance.branch.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("branch   ", Style::default().fg(t.muted)),
                Span::styled(instance.branch.clone(), Style::default().fg(t.text)),
            ]));
        }

        lines.push(Line::from(vec![
            Span::styled("uptime   ", Style::default().fg(t.muted)),
            Span::styled(
                format_uptime(instance.session.created_epoch),
                Style::default().fg(t.text),
            ),
        ]));

        lines.push(Line::from(vec![
            Span::styled("path     ", Style::default().fg(t.muted)),
            Span::styled(
                if instance.session.pane_current_path.is_empty() {
                    "\u{2014}".to_owned()
                } else {
                    instance.session.pane_current_path.clone()
                },
                Style::default().fg(t.text),
            ),
        ]));

        if let Some(url) = app.dev_server_urls.get(&instance.session.name) {
            lines.push(Line::from(vec![
                Span::styled("dev      ", Style::default().fg(t.muted)),
                Span::styled(url.clone(), Style::default().fg(t.green)),
            ]));
        } else if app.dev_server_sessions.contains_key(&instance.session.name) {
            lines.push(Line::from(vec![
                Span::styled("dev      ", Style::default().fg(t.muted)),
                Span::styled("starting\u{2026}", Style::default().fg(t.yellow)),
            ]));
        }

        lines.push(Line::from(""));

        let preview_space = area.height.saturating_sub(lines.len() as u16 + 1) as usize;
        let preview_take = preview_space.max(4);
        let preview: Vec<String> = instance
            .session
            .preview
            .iter()
            .rev()
            .take(preview_take)
            .cloned()
            .collect::<Vec<String>>()
            .into_iter()
            .rev()
            .collect();

        if preview.is_empty() {
            lines.push(Line::from(Span::styled(
                "(no output captured)",
                Style::default().fg(t.muted),
            )));
        } else {
            for line in preview {
                lines.push(Line::from(Span::styled(line, Style::default().fg(t.muted))));
            }
        }

        lines
    } else {
        vec![Line::from(Span::styled("select an instance", Style::default().fg(t.muted)))]
    };

    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .style(Style::default().fg(t.text).bg(t.bg))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_instance_tab(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
    let t = app.theme;
    let Some(instance) = app.current_tab_instance() else {
        draw_dashboard(frame, area, app);
        return;
    };

    let is_stopping = app.stopping_sessions.contains(&instance.session.name);

    let (state_label, state_style) = if is_stopping {
        ("stopping\u{2026}".to_owned(), Style::default().fg(t.yellow))
    } else {
        instance_state_label(app, instance)
    };

    let title = agents::derive_display_title(
        &instance.session.name,
        &instance.session.pane_title,
        &instance.session.pane_current_path,
        &instance.title_override,
    );

    let mut lines = vec![
        Line::from(Span::styled(title, Style::default().fg(t.text).add_modifier(Modifier::BOLD))),
        Line::from(Span::styled(instance.agent.label.clone(), Style::default().fg(t.muted))),
        Line::from(""),
        Line::from(vec![
            Span::styled("state    ", Style::default().fg(t.muted)),
            Span::styled(state_label, state_style),
        ]),
    ];

    if let Some(pr_num) = instance.pr_number {
        let pr_style = match instance.pr_state {
            Some(git::PrState::Merged) => Style::default().fg(t.accent),
            Some(git::PrState::Open) => Style::default().fg(t.yellow),
            Some(git::PrState::Closed) => Style::default().fg(t.red),
            None => Style::default().fg(t.text),
        };
        lines.push(Line::from(vec![
            Span::styled("pr       ", Style::default().fg(t.muted)),
            Span::styled(format!("#{pr_num}"), pr_style),
        ]));
    }

    if let Some(checks) = instance.pr_checks.as_ref() {
        if let Some(ci_label) = checks.short_label() {
            let ci_style = if checks.has_failures() {
                Style::default().fg(t.red)
            } else if checks.has_pending() {
                Style::default().fg(t.yellow)
            } else {
                Style::default().fg(t.green)
            };
            lines.push(Line::from(vec![
                Span::styled("ci       ", Style::default().fg(t.muted)),
                Span::styled(ci_label, ci_style),
            ]));
            if checks.has_failures() {
                lines.push(Line::from(vec![
                    Span::styled("checks   ", Style::default().fg(t.muted)),
                    Span::styled(checks.failed.join(", "), Style::default().fg(t.text)),
                ]));
            }
        }
    }

    if !instance.branch.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("branch   ", Style::default().fg(t.muted)),
            Span::styled(instance.branch.clone(), Style::default().fg(t.text)),
        ]));
    }

    lines.push(Line::from(vec![
        Span::styled("uptime   ", Style::default().fg(t.muted)),
        Span::styled(format_uptime(instance.session.created_epoch), Style::default().fg(t.text)),
    ]));

    lines.push(Line::from(vec![
        Span::styled("path     ", Style::default().fg(t.muted)),
        Span::styled(
            if instance.session.pane_current_path.is_empty() {
                "\u{2014}".to_owned()
            } else {
                instance.session.pane_current_path.clone()
            },
            Style::default().fg(t.text),
        ),
    ]));

    if let Some(url) = app.dev_server_urls.get(&instance.session.name) {
        lines.push(Line::from(vec![
            Span::styled("dev      ", Style::default().fg(t.muted)),
            Span::styled(url.clone(), Style::default().fg(t.green)),
        ]));
    } else if app.dev_server_sessions.contains_key(&instance.session.name) {
        lines.push(Line::from(vec![
            Span::styled("dev      ", Style::default().fg(t.muted)),
            Span::styled("starting\u{2026}", Style::default().fg(t.yellow)),
        ]));
    }

    lines.push(Line::from(""));

    let preview_take = area.height.saturating_sub(lines.len() as u16 + 1) as usize;
    let preview: Vec<String> = instance
        .session
        .preview
        .iter()
        .rev()
        .take(preview_take.max(4))
        .cloned()
        .collect::<Vec<String>>()
        .into_iter()
        .rev()
        .collect();

    if preview.is_empty() {
        lines.push(Line::from(Span::styled("(no output captured)", Style::default().fg(t.muted))));
    } else {
        lines.push(Line::from(Span::styled("~ live buffer ~", Style::default().fg(t.accent))));
        for line in preview {
            lines.push(Line::from(Span::styled(line, Style::default().fg(t.text))));
        }
    }

    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .style(Style::default().fg(t.text).bg(t.bg))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_status_line(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
    let t = app.theme;

    if !app.status_line.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                app.status_line.clone(),
                Style::default().fg(t.muted),
            )))
            .alignment(Alignment::Center)
            .style(Style::default().bg(t.bg)),
            area,
        );
    }
}

fn instance_state_label(app: &App, instance: &crate::app::AgentInstance) -> (String, Style) {
    let t = app.theme;
    if instance.pr_checks.as_ref().map(|checks| checks.has_failures()).unwrap_or(false) {
        ("checks failing".to_owned(), Style::default().fg(t.red))
    } else if instance.pr_state == Some(git::PrState::Merged) {
        ("merged".to_owned(), Style::default().fg(t.accent))
    } else if instance.pr_state == Some(git::PrState::Open) {
        ("pr open".to_owned(), Style::default().fg(t.yellow))
    } else if instance.completed {
        ("completed".to_owned(), Style::default().fg(t.green))
    } else if instance.session.attached {
        ("attached".to_owned(), Style::default().fg(t.green))
    } else {
        ("idle".to_owned(), Style::default().fg(t.muted))
    }
}

fn draw_footer_rule(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
    let t = app.theme;
    let w = area.width as usize;
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "\u{2500}".repeat(w),
            Style::default().fg(t.border),
        )))
        .style(Style::default().bg(t.bg)),
        area,
    );
}

fn draw_footer(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
    let t = app.theme;

    let key_style = Style::default().fg(t.text).add_modifier(Modifier::BOLD);
    let desc_style = Style::default().fg(t.muted);

    // Helper to build a key-desc pair with trailing spacing.
    let kb = |k: &str, d: &str| -> Vec<Span<'_>> {
        vec![Span::styled(k.to_owned(), key_style), Span::styled(format!(" {d}   "), desc_style)]
    };
    // Same but no trailing spacing (for last item).
    let kb_last = |k: &str, d: &str| -> Vec<Span<'_>> {
        vec![Span::styled(k.to_owned(), key_style), Span::styled(format!(" {d}"), desc_style)]
    };

    let spans: Vec<Span<'_>> = if app.modal.is_some() {
        // ── Spawn modal ──
        if let Some(modal) = app.modal.as_ref() {
            match modal.step {
                SpawnStep::Agent => [
                    kb("1-9", "select"),
                    kb("\u{2191}/\u{2193}", "navigate"),
                    kb("enter", "next"),
                    kb_last("esc", "close"),
                ]
                .concat(),
                SpawnStep::Path => [
                    kb(".", "here"),
                    kb("/", "type"),
                    kb("+", "mkdir"),
                    kb("g", "clone"),
                    kb("-", "up"),
                    kb("~", "home"),
                    kb("h", "back"),
                    kb_last("esc", "close"),
                ]
                .concat(),
                SpawnStep::NewDirectoryName => {
                    [kb("enter", "create"), kb_last("esc", "back")].concat()
                }
                SpawnStep::CloneUrl => [kb("enter", "clone"), kb_last("esc", "back")].concat(),
                SpawnStep::TypePath => [kb("enter", "go"), kb_last("esc", "back")].concat(),
            }
        } else {
            vec![]
        }
    } else if app.startup_cmds_open {
        // ── Startup commands sub-view ──
        if app.startup_cmds_adding.is_some() {
            [kb("\u{2191}/\u{2193}", "navigate"), kb("enter", "confirm"), kb_last("esc", "back")]
                .concat()
        } else {
            [
                kb("\u{2191}/\u{2193}", "navigate"),
                kb("a", "add"),
                kb("x", "remove"),
                kb_last("esc", "back"),
            ]
            .concat()
        }
    } else if app.dev_servers_open {
        // ── Dev servers sub-view ──
        if app.dev_servers_adding.is_some() {
            [kb("\u{2191}/\u{2193}", "navigate"), kb("enter", "confirm"), kb_last("esc", "back")]
                .concat()
        } else {
            [
                kb("\u{2191}/\u{2193}", "navigate"),
                kb("a", "add"),
                kb("x", "remove"),
                kb_last("esc", "back"),
            ]
            .concat()
        }
    } else if app.channels_open {
        // ── Channels sub-view ──
        if app.channels_adding.is_some() {
            [kb("enter", "save"), kb_last("esc", "cancel")].concat()
        } else {
            [
                kb("\u{2191}/\u{2193}", "navigate"),
                kb("a", "add"),
                kb("x", "remove"),
                kb_last("esc", "back"),
            ]
            .concat()
        }
    } else if app.router_settings_open {
        // ── Router settings sub-view ──
        if app.router_settings_editing.is_some() {
            [kb("enter", "save"), kb_last("esc", "cancel")].concat()
        } else {
            [kb("\u{2191}/\u{2193}", "navigate"), kb("enter", "edit/toggle"), kb_last("esc", "back")]
                .concat()
        }
    } else if app.permissions_open {
        // ── Permissions sub-view ──
        [kb("\u{2191}/\u{2193}", "navigate"), kb("enter", "toggle"), kb_last("esc", "back")]
            .concat()
    } else if app.settings_open {
        // ── Settings view ──
        if app.settings_editing.is_some() {
            [kb("enter", "save"), kb_last("esc", "cancel")].concat()
        } else {
            [kb("\u{2191}/\u{2193}", "navigate"), kb("enter", "edit"), kb_last("esc", "back")]
                .concat()
        }
    } else if app.is_split_mode() {
        // ── Split selection ──
        let pane_count = app.split.as_ref().map(|s| s.panes.len()).unwrap_or(0);
        [
            kb("v", "add pane"),
            kb("c", "remove"),
            kb("\u{2190}/\u{2192}", "navigate"),
            vec![
                Span::styled("enter".to_owned(), key_style),
                Span::styled(
                    format!(
                        " launch ({})   ",
                        if pane_count < 2 { "need 2+".to_owned() } else { format!("{pane_count}") }
                    ),
                    desc_style,
                ),
            ],
            kb("esc", "cancel"),
            kb_last("q", "quit"),
        ]
        .concat()
    } else {
        // ── Main view ── context-dependent on selection
        let active = app.active_instance_ref();
        let on_dashboard = app.selected_tab == 0;

        let mut s: Vec<Span<'_>> = Vec::new();

        if !on_dashboard {
            s.extend(kb("s", "sessions"));
        }

        if !app.instances.is_empty() {
            s.extend(kb("1-9", "jump"));
        }

        s.extend(kb("n", "new"));

        let dormant = if on_dashboard { app.selected_dormant() } else { None };

        if on_dashboard && app.is_action_row_selected() {
            s.extend(kb("enter", "spawn"));
        } else if on_dashboard && app.is_settings_row_selected() {
            s.extend(kb("enter", "settings"));
        } else if let Some(d) = dormant {
            if d.claude_session_id.is_some() {
                s.extend(kb("enter", "resume"));
            }
            s.extend(kb("x", "remove"));
        } else if active.is_some() {
            s.extend(kb("enter", "attach"));
            s.extend(kb("t", "terminal"));
        }

        if !app.instances.is_empty() {
            s.extend(kb("v", "split"));
        }

        if active.is_some() {
            s.extend(kb("x", "stop"));

            // Dev server keybinds
            if app.has_dev_server() {
                s.extend(kb("R", "restart dev"));
                s.extend(kb("D", "stop dev"));
                if active
                    .map(|i| app.dev_server_urls.contains_key(&i.session.name))
                    .unwrap_or(false)
                {
                    s.extend(kb("O", "view dev"));
                }
            } else if active
                .map(|i| {
                    !i.session.pane_current_path.is_empty()
                        && config::get_dev_server_command(&app.config, &i.session.pane_current_path)
                            .is_some()
                })
                .unwrap_or(false)
            {
                s.extend(kb("R", "start dev"));
            }

            // Dynamic PR keybinds
            match active {
                Some(i)
                    if i.pr_state == Some(git::PrState::Open)
                        && i.pr_checks.as_ref().map(|c| c.has_failures()).unwrap_or(false) =>
                {
                    s.extend(kb("f", "fix ci"));
                    s.extend(kb("p", "merge pr"));
                    s.extend(kb("o", "view pr"));
                }
                Some(i) if i.pr_state == Some(git::PrState::Merged) => {
                    s.extend(kb("o", "view pr"));
                }
                Some(i) if i.pr_state == Some(git::PrState::Open) => {
                    s.extend(kb("p", "merge pr"));
                    s.extend(kb("o", "view pr"));
                }
                _ => s.extend(kb("p", "open pr")),
            }
        }

        s.extend(kb_last("q", "quit"));
        s
    };

    // If the keybinds overflow the footer width, wrap-around ticker; otherwise center.
    let view_w = area.width as usize;
    let total_w: usize = {
        use unicode_width::UnicodeWidthStr;
        spans.iter().map(|s| UnicodeWidthStr::width(s.content.as_ref())).sum()
    };

    if total_w > view_w {
        app.ticker_active.set(true);
        let visible = ticker_spans(&spans, view_w, app.tick);
        let commands = Line::from(visible);
        frame.render_widget(Paragraph::new(commands).style(Style::default().bg(t.bg)), area);
    } else {
        let commands = Line::from(spans);
        frame.render_widget(
            Paragraph::new(commands).alignment(Alignment::Center).style(Style::default().bg(t.bg)),
            area,
        );
    }
}

fn draw_spawn_modal(frame: &mut ratatui::Frame<'_>, app: &App) {
    let t = app.theme;
    let Some(modal) = app.modal.as_ref() else {
        return;
    };

    let area = centered_rect(70, 75, frame.area());
    frame.render_widget(Clear, area);

    let selected_agent = app
        .available_agents
        .get(modal.selected_agent)
        .map(|a| a.label.clone())
        .unwrap_or_else(|| "none".to_owned());

    let agent_step_style = if modal.step == SpawnStep::Agent {
        Style::default().fg(t.accent)
    } else {
        Style::default().fg(t.green)
    };
    let path_step_style = if modal.step == SpawnStep::Path
        || modal.step == SpawnStep::NewDirectoryName
        || modal.step == SpawnStep::CloneUrl
        || modal.step == SpawnStep::TypePath
    {
        Style::default().fg(t.accent)
    } else {
        Style::default().fg(t.muted)
    };

    let mut lines = vec![
        Line::from(Span::styled(
            "spawn new instance",
            Style::default().fg(t.text).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("  1 ", agent_step_style.add_modifier(Modifier::BOLD)),
            Span::styled("agent", agent_step_style),
            Span::styled("  ", Style::default()),
            Span::styled(selected_agent.clone(), Style::default().fg(t.muted)),
        ]),
        Line::from(vec![
            Span::styled("  2 ", path_step_style.add_modifier(Modifier::BOLD)),
            Span::styled("path", path_step_style),
        ]),
        Line::from(""),
    ];

    match modal.step {
        SpawnStep::Agent => {
            lines.push(Line::from(Span::styled(
                "  ~ select agent ~",
                Style::default().fg(t.accent),
            )));

            let capacity = area.height.saturating_sub(12) as usize;
            let (start, end) =
                visible_range(app.available_agents.len(), modal.selected_agent, capacity.max(1));
            if start > 0 {
                lines.push(Line::from(Span::styled("  ...", Style::default().fg(t.muted))));
            }

            for i in start..end {
                let agent = &app.available_agents[i];
                let selected = i == modal.selected_agent;
                let style = if selected {
                    Style::default().fg(t.bg).bg(t.highlight_bg).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(t.text)
                };
                let num_style = if selected {
                    Style::default().fg(t.bg).bg(t.highlight_bg)
                } else {
                    Style::default().fg(t.muted)
                };
                let num_label = if i < 9 { format!("{}", i + 1) } else { " ".to_owned() };
                lines.push(Line::from(vec![
                    Span::styled(format!("  {num_label} "), num_style),
                    Span::styled(agent.label.clone(), style),
                ]));
            }

            if end < app.available_agents.len() {
                lines.push(Line::from(Span::styled("  ...", Style::default().fg(t.muted))));
            }

            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("  1-9", Style::default().fg(t.text).add_modifier(Modifier::BOLD)),
                Span::styled(" select   ", Style::default().fg(t.muted)),
                Span::styled("enter", Style::default().fg(t.text).add_modifier(Modifier::BOLD)),
                Span::styled(" next   ", Style::default().fg(t.muted)),
                Span::styled(
                    "\u{2191}/\u{2193}",
                    Style::default().fg(t.text).add_modifier(Modifier::BOLD),
                ),
                Span::styled(" move   ", Style::default().fg(t.muted)),
                Span::styled("esc", Style::default().fg(t.text).add_modifier(Modifier::BOLD)),
                Span::styled(" cancel", Style::default().fg(t.muted)),
            ]));
        }
        SpawnStep::Path => {
            lines.push(Line::from(vec![
                Span::styled("  cwd ", Style::default().fg(t.muted)),
                Span::styled(
                    format!("{}", modal.browser.cwd().display()),
                    Style::default().fg(t.text),
                ),
            ]));
            lines.push(Line::from(""));

            let entries = modal.browser.entries();
            let capacity = area.height.saturating_sub(13) as usize;
            let (start, end) =
                visible_range(entries.len(), modal.browser.selected(), capacity.max(1));

            if start > 0 {
                lines.push(Line::from(Span::styled("  ...", Style::default().fg(t.muted))));
            }

            for (i, entry) in entries.iter().enumerate().skip(start).take(end - start) {
                let icon = match entry.kind {
                    EntryKind::SelectCurrent => "\u{2192}",
                    EntryKind::CreateDirectory => "+",
                    EntryKind::CloneFromUrl => "\u{21e3}",
                    EntryKind::TypePath => "/",
                    EntryKind::Parent => "\u{2190}",
                    EntryKind::Directory => " ",
                };

                let style = if i == modal.browser.selected() {
                    Style::default().fg(t.bg).bg(t.highlight_bg).add_modifier(Modifier::BOLD)
                } else if matches!(
                    entry.kind,
                    EntryKind::CreateDirectory | EntryKind::CloneFromUrl | EntryKind::TypePath
                ) {
                    Style::default().fg(t.accent)
                } else if matches!(entry.kind, EntryKind::SelectCurrent) {
                    Style::default().fg(t.green)
                } else {
                    Style::default().fg(t.text)
                };

                lines.push(Line::from(Span::styled(format!("  {} {}", icon, entry.label), style)));
            }

            if end < entries.len() {
                lines.push(Line::from(Span::styled("  ...", Style::default().fg(t.muted))));
            }

            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("  .", Style::default().fg(t.text).add_modifier(Modifier::BOLD)),
                Span::styled(" here   ", Style::default().fg(t.muted)),
                Span::styled("/", Style::default().fg(t.text).add_modifier(Modifier::BOLD)),
                Span::styled(" type   ", Style::default().fg(t.muted)),
                Span::styled("+", Style::default().fg(t.text).add_modifier(Modifier::BOLD)),
                Span::styled(" mkdir   ", Style::default().fg(t.muted)),
                Span::styled("g", Style::default().fg(t.text).add_modifier(Modifier::BOLD)),
                Span::styled(" clone   ", Style::default().fg(t.muted)),
                Span::styled("-", Style::default().fg(t.text).add_modifier(Modifier::BOLD)),
                Span::styled(" up   ", Style::default().fg(t.muted)),
                Span::styled("~", Style::default().fg(t.text).add_modifier(Modifier::BOLD)),
                Span::styled(" home   ", Style::default().fg(t.muted)),
                Span::styled("h", Style::default().fg(t.text).add_modifier(Modifier::BOLD)),
                Span::styled(" back", Style::default().fg(t.muted)),
            ]));
        }
        SpawnStep::NewDirectoryName => {
            lines.push(Line::from(vec![
                Span::styled("  cwd ", Style::default().fg(t.muted)),
                Span::styled(
                    format!("{}", modal.browser.cwd().display()),
                    Style::default().fg(t.text),
                ),
            ]));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled("  directory name", Style::default().fg(t.muted))));
            lines.push(Line::from(Span::styled(
                if modal.new_dir_name.is_empty() {
                    "  _".to_owned()
                } else {
                    format!("  {}_", modal.new_dir_name)
                },
                Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("  enter", Style::default().fg(t.text).add_modifier(Modifier::BOLD)),
                Span::styled(" create   ", Style::default().fg(t.muted)),
                Span::styled("esc", Style::default().fg(t.text).add_modifier(Modifier::BOLD)),
                Span::styled(" back", Style::default().fg(t.muted)),
            ]));
        }
        SpawnStep::CloneUrl => {
            lines.push(Line::from(vec![
                Span::styled("  cwd ", Style::default().fg(t.muted)),
                Span::styled(
                    format!("{}", modal.browser.cwd().display()),
                    Style::default().fg(t.text),
                ),
            ]));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled("  git URL", Style::default().fg(t.muted))));
            lines.push(Line::from(Span::styled(
                if modal.clone_url.is_empty() {
                    "  _".to_owned()
                } else {
                    format!("  {}_", modal.clone_url)
                },
                Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("  enter", Style::default().fg(t.text).add_modifier(Modifier::BOLD)),
                Span::styled(" clone   ", Style::default().fg(t.muted)),
                Span::styled("esc", Style::default().fg(t.text).add_modifier(Modifier::BOLD)),
                Span::styled(" back", Style::default().fg(t.muted)),
            ]));
        }
        SpawnStep::TypePath => {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "  enter path (~ supported)",
                Style::default().fg(t.muted),
            )));
            lines.push(Line::from(Span::styled(
                if modal.typed_path.is_empty() {
                    "  _".to_owned()
                } else {
                    format!("  {}_", modal.typed_path)
                },
                Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("  enter", Style::default().fg(t.text).add_modifier(Modifier::BOLD)),
                Span::styled(" go to path   ", Style::default().fg(t.muted)),
                Span::styled("esc", Style::default().fg(t.text).add_modifier(Modifier::BOLD)),
                Span::styled(" back", Style::default().fg(t.muted)),
            ]));
        }
    }

    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .style(Style::default().fg(t.text).bg(t.bg))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(Line::from(vec![Span::styled(
                        " spawn ",
                        Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
                    )]))
                    .border_style(Style::default().fg(t.accent))
                    .style(Style::default().bg(t.bg)),
            )
            .wrap(Wrap { trim: false }),
        area,
    );
}

pub fn visible_range(total: usize, selected: usize, capacity: usize) -> (usize, usize) {
    if total == 0 {
        return (0, 0);
    }
    if total <= capacity {
        return (0, total);
    }

    let half = capacity / 2;
    let mut start = selected.saturating_sub(half);
    let max_start = total.saturating_sub(capacity);
    if start > max_start {
        start = max_start;
    }

    (start, (start + capacity).min(total))
}

pub fn truncate(input: &str, max: usize) -> String {
    use unicode_width::UnicodeWidthChar;
    use unicode_width::UnicodeWidthStr;

    if UnicodeWidthStr::width(input) <= max {
        return input.to_owned();
    }

    let mut out = String::new();
    let mut width = 0;
    let target = max.saturating_sub(1); // reserve 1 column for '~'
    for c in input.chars() {
        let cw = UnicodeWidthChar::width(c).unwrap_or(0);
        if width + cw > target {
            break;
        }
        out.push(c);
        width += cw;
    }
    out.push('~');
    out
}

/// Gap (in spaces) inserted between the end and the repeated start in ticker wrap-around.
const TICKER_GAP: usize = 4;
/// Frames per 1-column scroll step.
const TICKER_SPEED: u64 = 2;
/// Frames to pause at the start position before scrolling begins.
const TICKER_PAUSE: u64 = 8;

/// Continuous wrap-around ticker for a list of styled spans.
///
/// Scrolls left forever. When text exits on the left, it re-enters from the right
/// with a small gap — like a rotating donut. Pauses briefly at the home position.
fn ticker_spans<'a>(spans: &[Span<'a>], view_width: usize, tick: u64) -> Vec<Span<'a>> {
    use unicode_width::UnicodeWidthChar;
    use unicode_width::UnicodeWidthStr;

    let total_w: usize = spans.iter().map(|s| UnicodeWidthStr::width(s.content.as_ref())).sum();
    if total_w <= view_width {
        return spans.to_vec();
    }

    let cycle_len = (total_w + TICKER_GAP) as u64;
    // Pause at home, then scroll continuously
    let raw =
        if tick < TICKER_PAUSE { 0 } else { ((tick - TICKER_PAUSE) / TICKER_SPEED) % cycle_len };
    let offset = raw as usize;

    // Build the visible window by walking the "doubled" content: spans + gap + spans
    let gap_style = if let Some(first) = spans.last() { first.style } else { Style::default() };
    let gap_span = Span::styled(" ".repeat(TICKER_GAP), gap_style);

    // Virtual stream: [spans..., gap, spans...]
    // We iterate twice over spans with a gap in between, slicing the window [offset, offset+view_width).
    let mut result: Vec<Span<'a>> = Vec::new();
    let mut col = 0usize;
    let window_end = offset + view_width;

    let iter_spans = spans.iter().chain(std::iter::once(&gap_span)).chain(spans.iter());

    for span in iter_spans {
        let span_w = UnicodeWidthStr::width(span.content.as_ref());
        let span_end = col + span_w;

        if span_end <= offset {
            col = span_end;
            continue;
        }
        if col >= window_end {
            break;
        }

        let skip = if offset > col { offset - col } else { 0 };
        let take = (window_end - col).min(span_w) - skip;

        let mut out = String::new();
        let mut w = 0usize;
        let mut skipped = 0usize;
        for c in span.content.chars() {
            let cw = UnicodeWidthChar::width(c).unwrap_or(0);
            if skipped < skip {
                skipped += cw;
                continue;
            }
            if w + cw > take {
                break;
            }
            out.push(c);
            w += cw;
        }
        if !out.is_empty() {
            result.push(Span::styled(out, span.style));
        }

        col = span_end;
    }

    result
}

/// Bounded horizontal scroll for a plain string.
fn scroll_str(input: &str, view_width: usize, offset: usize) -> String {
    use unicode_width::UnicodeWidthChar;
    use unicode_width::UnicodeWidthStr;

    let full_w = UnicodeWidthStr::width(input);
    if full_w <= view_width {
        return input.to_owned();
    }

    let offset = offset.min(full_w - view_width);
    let mut out = String::new();
    let mut col = 0usize;
    let mut taken = 0usize;

    for c in input.chars() {
        let cw = UnicodeWidthChar::width(c).unwrap_or(0);
        let char_end = col + cw;
        col = char_end;
        if char_end <= offset {
            continue;
        }
        if taken + cw > view_width {
            break;
        }
        out.push(c);
        taken += cw;
    }
    out
}

pub fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}
