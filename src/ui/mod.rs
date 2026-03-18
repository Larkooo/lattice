mod settings;

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::{
    agents,
    app::{instance_category, instance_project_name, App, AppScreen, SpawnStep},
    config, git,
    pathnav::EntryKind,
};
use settings::{draw_permissions_view, draw_settings_view, draw_startup_cmds_view};

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
    for (i, cell) in cells.iter().enumerate() {
        mid_spans.push(Span::styled("\u{2502}", border_style));

        let cw = col_widths[i];
        let display_label =
            if cell.label.len() > cw { truncate(&cell.label, cw) } else { cell.label.clone() };
        let label_len = display_label.len();
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
    }
    mid_spans.push(Span::styled("\u{2502}", border_style));

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

    if start > 0 {
        lines.push(Line::from(Span::styled("...", Style::default().fg(t.muted))));
    }

    // Track which project header was last rendered so we insert one on change.
    let prev_project: Option<String> =
        if start > 0 { app.instances.get(start - 1).map(instance_project_name) } else { None };
    let mut last_project = prev_project;

    for index in start..end {
        let selected = index == app.selected_row;

        if index < app.instances.len() {
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

            let (label, style) = if is_stopping {
                let label = format!("\u{29D7} {} stopping\u{2026}", truncate(&title, 18));
                let style = if selected {
                    Style::default().fg(t.bg).bg(t.highlight_bg).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(t.yellow)
                };
                (label, style)
            } else if instance.pr_state == Some(git::PrState::Merged) {
                let label = format!("\u{21B3} {}", truncate(&title, 26));
                let style = if selected {
                    Style::default().fg(t.bg).bg(t.highlight_bg).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(t.accent)
                };
                (label, style)
            } else if instance.pr_state == Some(git::PrState::Open) {
                let label = format!("\u{2197} {}", truncate(&title, 26));
                let style = if selected {
                    Style::default().fg(t.bg).bg(t.highlight_bg).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(t.yellow)
                };
                (label, style)
            } else if instance.completed {
                let label = format!("\u{2713} {}", truncate(&title, 26));
                let style = if selected {
                    Style::default().fg(t.bg).bg(t.highlight_bg).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(t.green)
                };
                (label, style)
            } else {
                let label = truncate(&title, 28);
                let style = if selected {
                    Style::default().fg(t.bg).bg(t.highlight_bg).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(t.text)
                };
                (label, style)
            };

            lines.push(Line::from(Span::styled(label, style)));
        } else if index == app.instances.len() {
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
    } else if app.is_action_row_selected() || app.instances.is_empty() {
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
        let (state_label, state_style) = if instance.pr_state == Some(git::PrState::Merged) {
            ("merged \u{2014} ready to stop", Style::default().fg(t.accent))
        } else if instance.pr_state == Some(git::PrState::Open) {
            ("PR open \u{2014} press p to merge", Style::default().fg(t.yellow))
        } else if instance.completed {
            ("completed", Style::default().fg(t.green))
        } else if instance.session.attached {
            ("attached", Style::default().fg(t.green))
        } else {
            ("idle", Style::default().fg(t.muted))
        };

        let mut lines = vec![
            Line::from(Span::styled(
                instance.agent.label.clone(),
                Style::default().fg(t.text).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(vec![
                Span::styled("session  ", Style::default().fg(t.muted)),
                Span::styled(instance.session.name.clone(), Style::default().fg(t.text)),
            ]),
            Line::from(vec![
                Span::styled("created  ", Style::default().fg(t.muted)),
                Span::styled(instance.session.created.clone(), Style::default().fg(t.text)),
            ]),
            Line::from(vec![
                Span::styled("state    ", Style::default().fg(t.muted)),
                Span::styled(state_label, state_style),
            ]),
            Line::from(vec![
                Span::styled("kind     ", Style::default().fg(t.muted)),
                Span::styled(
                    if instance.managed { "managed" } else { "external" },
                    Style::default().fg(t.text),
                ),
            ]),
            Line::from(vec![
                Span::styled("command  ", Style::default().fg(t.muted)),
                Span::styled(instance.session.current_command.clone(), Style::default().fg(t.text)),
            ]),
            Line::from(vec![
                Span::styled("path     ", Style::default().fg(t.muted)),
                Span::styled(
                    if instance.session.pane_current_path.is_empty() {
                        "\u{2014}".to_owned()
                    } else {
                        instance.session.pane_current_path.clone()
                    },
                    Style::default().fg(t.text),
                ),
            ]),
            Line::from(""),
        ];

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
        ("stopping\u{2026}", Style::default().fg(t.yellow))
    } else if instance.pr_state == Some(git::PrState::Merged) {
        ("merged \u{2014} ready to stop", Style::default().fg(t.accent))
    } else if instance.pr_state == Some(git::PrState::Open) {
        ("PR open \u{2014} press p to merge", Style::default().fg(t.yellow))
    } else if instance.completed {
        ("completed", Style::default().fg(t.green))
    } else if instance.session.attached {
        ("attached", Style::default().fg(t.green))
    } else {
        ("idle", Style::default().fg(t.muted))
    };

    let mut lines = vec![
        Line::from(Span::styled(
            instance.agent.label.clone(),
            Style::default().fg(t.text).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("session  ", Style::default().fg(t.muted)),
            Span::styled(instance.session.name.clone(), Style::default().fg(t.text)),
        ]),
        Line::from(vec![
            Span::styled("created  ", Style::default().fg(t.muted)),
            Span::styled(instance.session.created.clone(), Style::default().fg(t.text)),
        ]),
        Line::from(vec![
            Span::styled("state    ", Style::default().fg(t.muted)),
            Span::styled(state_label, state_style),
        ]),
        Line::from(vec![
            Span::styled("windows  ", Style::default().fg(t.muted)),
            Span::styled(format!("{}", instance.session.windows), Style::default().fg(t.text)),
        ]),
        Line::from(vec![
            Span::styled("command  ", Style::default().fg(t.muted)),
            Span::styled(instance.session.current_command.clone(), Style::default().fg(t.text)),
        ]),
        Line::from(vec![
            Span::styled("path     ", Style::default().fg(t.muted)),
            Span::styled(
                if instance.session.pane_current_path.is_empty() {
                    "\u{2014}".to_owned()
                } else {
                    instance.session.pane_current_path.clone()
                },
                Style::default().fg(t.text),
            ),
        ]),
        Line::from(""),
    ];

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

    let pane_count = app.split.as_ref().map(|s| s.panes.len()).unwrap_or(0);
    let commands = if app.is_split_mode() {
        Line::from(vec![
            Span::styled("v", key_style),
            Span::styled(" add pane   ", desc_style),
            Span::styled("c", key_style),
            Span::styled(" remove   ", desc_style),
            Span::styled("\u{2190}/\u{2192}", key_style),
            Span::styled(" navigate   ", desc_style),
            Span::styled("enter", key_style),
            Span::styled(
                format!(
                    " launch ({})   ",
                    if pane_count < 2 {
                        "need 2+".to_owned()
                    } else {
                        format!("{pane_count} panes")
                    }
                ),
                desc_style,
            ),
            Span::styled("esc", key_style),
            Span::styled(" cancel   ", desc_style),
            Span::styled("q", key_style),
            Span::styled(" quit", desc_style),
        ])
    } else {
        Line::from(vec![
            Span::styled("s", key_style),
            Span::styled(" sessions   ", desc_style),
            Span::styled("1-9", key_style),
            Span::styled(" jump   ", desc_style),
            Span::styled("n", key_style),
            Span::styled(" new   ", desc_style),
            Span::styled("enter", key_style),
            Span::styled(" attach   ", desc_style),
            Span::styled("t", key_style),
            Span::styled(" terminal   ", desc_style),
            Span::styled("v", key_style),
            Span::styled(" split   ", desc_style),
            Span::styled("x", key_style),
            Span::styled(" stop   ", desc_style),
            Span::styled("p", key_style),
            Span::styled(" pr   ", desc_style),
            Span::styled("q", key_style),
            Span::styled(" quit", desc_style),
        ])
    };

    frame.render_widget(
        Paragraph::new(commands).alignment(Alignment::Center).style(Style::default().bg(t.bg)),
        area,
    );
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
                lines.push(Line::from(Span::styled(format!("  {}", agent.label), style)));
            }

            if end < app.available_agents.len() {
                lines.push(Line::from(Span::styled("  ...", Style::default().fg(t.muted))));
            }

            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("  enter", Style::default().fg(t.text).add_modifier(Modifier::BOLD)),
                Span::styled(" next   ", Style::default().fg(t.muted)),
                Span::styled("esc", Style::default().fg(t.text).add_modifier(Modifier::BOLD)),
                Span::styled(" cancel   ", Style::default().fg(t.muted)),
                Span::styled(
                    "\u{2191}/\u{2193}",
                    Style::default().fg(t.text).add_modifier(Modifier::BOLD),
                ),
                Span::styled(" move", Style::default().fg(t.muted)),
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
                Span::styled("  enter", Style::default().fg(t.text).add_modifier(Modifier::BOLD)),
                Span::styled(" select   ", Style::default().fg(t.muted)),
                Span::styled("h", Style::default().fg(t.text).add_modifier(Modifier::BOLD)),
                Span::styled(" back   ", Style::default().fg(t.muted)),
                Span::styled("esc", Style::default().fg(t.text).add_modifier(Modifier::BOLD)),
                Span::styled(" cancel", Style::default().fg(t.muted)),
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
    if input.chars().count() <= max {
        return input.to_owned();
    }

    let mut out = input.chars().take(max.saturating_sub(1)).collect::<String>();
    out.push('~');
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
