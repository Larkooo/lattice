use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Paragraph, Wrap},
};

use super::visible_range;
use crate::{
    app::{App, DevServerAddStep, StartupCmdAddStep},
    config,
    handlers::{
        router_setting_is_bool, router_setting_label, router_setting_value, setting_is_bool,
        setting_label, setting_value, SETTINGS_COUNT,
    },
    pathnav::EntryKind,
};

pub fn draw_channels_view(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
    let t = app.theme;

    let mut lines = vec![
        Line::from(Span::styled(
            "channels",
            Style::default().fg(t.text).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "configure --channels flags passed when launching agents",
            Style::default().fg(t.muted),
        )),
        Line::from(""),
    ];

    if let Some(ref buf) = app.channels_adding {
        let agent_label = app
            .available_agents
            .get(app.channels_selected)
            .map(|a| a.label.as_str())
            .unwrap_or("?");
        lines.push(Line::from(Span::styled(
            format!("add channel for {agent_label}:"),
            Style::default().fg(t.accent),
        )));
        lines.push(Line::from(Span::styled(
            if buf.is_empty() { "  _".to_owned() } else { format!("  {buf}_") },
            Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));
        let key_style = Style::default().fg(t.text).add_modifier(Modifier::BOLD);
        let desc_style = Style::default().fg(t.muted);
        lines.push(Line::from(vec![
            Span::styled("enter", key_style),
            Span::styled(" save   ", desc_style),
            Span::styled("esc", key_style),
            Span::styled(" cancel", desc_style),
        ]));
    } else if app.available_agents.is_empty() {
        lines.push(Line::from(Span::styled("no agents detected", Style::default().fg(t.muted))));
    } else {
        for (i, agent) in app.available_agents.iter().enumerate() {
            let selected = i == app.channels_selected;
            let channels = app.config.channels.get(&agent.id);
            let count = channels.map(|c| c.len()).unwrap_or(0);

            let label = format!("{:<16}", agent.label);
            let summary = if count == 0 {
                "none".to_owned()
            } else {
                format!("{count} channel{}", if count == 1 { "" } else { "s" })
            };

            let row_style = if selected {
                Style::default().fg(t.bg).bg(t.highlight_bg).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(t.text)
            };

            let summary_style = if selected {
                row_style
            } else if count == 0 {
                Style::default().fg(t.muted)
            } else {
                Style::default().fg(t.green)
            };

            lines.push(Line::from(vec![
                Span::styled(format!("  {label}"), row_style),
                Span::styled(summary, summary_style),
            ]));

            // Show individual channels for the selected agent
            if selected {
                if let Some(ch_list) = channels {
                    for ch in ch_list {
                        let ch_style = if selected {
                            Style::default().fg(t.bg).bg(t.highlight_bg)
                        } else {
                            Style::default().fg(t.muted)
                        };
                        lines.push(Line::from(Span::styled(
                            format!("    {ch}"),
                            ch_style,
                        )));
                    }
                }
            }
        }

        lines.push(Line::from(""));
        let key_style = Style::default().fg(t.text).add_modifier(Modifier::BOLD);
        let desc_style = Style::default().fg(t.muted);
        lines.push(Line::from(vec![
            Span::styled("\u{2191}/\u{2193}", key_style),
            Span::styled(" navigate   ", desc_style),
            Span::styled("a", key_style),
            Span::styled("/", desc_style),
            Span::styled("enter", key_style),
            Span::styled(" add   ", desc_style),
            Span::styled("x", key_style),
            Span::styled(" remove   ", desc_style),
            Span::styled("esc", key_style),
            Span::styled(" back", desc_style),
        ]));
    }

    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .style(Style::default().fg(t.text).bg(t.bg))
            .wrap(Wrap { trim: false }),
        area,
    );
}

pub fn draw_permissions_view(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
    let t = app.theme;

    let mut lines = vec![
        Line::from(Span::styled(
            "agent permissions",
            Style::default().fg(t.text).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "bypass permission prompts when launching agents",
            Style::default().fg(t.muted),
        )),
        Line::from(""),
    ];

    if !app.config.git_worktrees {
        lines.push(Line::from(Span::styled(
            "! git worktrees are off \u{2014} bypass is safer with isolated branches",
            Style::default().fg(t.yellow),
        )));
        lines.push(Line::from(""));
    }

    if app.available_agents.is_empty() {
        lines.push(Line::from(Span::styled("no agents detected", Style::default().fg(t.muted))));
    } else {
        for (i, agent) in app.available_agents.iter().enumerate() {
            let selected = i == app.permissions_selected;
            let bypassed = config::is_bypass_enabled(&app.config, &agent.id);
            let has_flag = agent.bypass_flag.is_some();

            let label = format!("{:<16}", agent.label);
            let status = if !has_flag {
                "no bypass flag"
            } else if bypassed {
                "bypass ON"
            } else {
                "restricted"
            };

            let row_style = if selected {
                Style::default().fg(t.bg).bg(t.highlight_bg).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(t.text)
            };

            let status_style = if selected {
                row_style
            } else if !has_flag {
                Style::default().fg(t.muted)
            } else if bypassed {
                Style::default().fg(t.yellow)
            } else {
                Style::default().fg(t.green)
            };

            lines.push(Line::from(vec![
                Span::styled(format!("  {label}"), row_style),
                Span::styled(status.to_owned(), status_style),
            ]));
        }
    }

    lines.push(Line::from(""));
    let key_style = Style::default().fg(t.text).add_modifier(Modifier::BOLD);
    let desc_style = Style::default().fg(t.muted);
    lines.push(Line::from(vec![
        Span::styled("\u{2191}/\u{2193}", key_style),
        Span::styled(" navigate   ", desc_style),
        Span::styled("enter", key_style),
        Span::styled(" toggle   ", desc_style),
        Span::styled("esc", key_style),
        Span::styled(" back", desc_style),
    ]));

    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .style(Style::default().fg(t.text).bg(t.bg))
            .wrap(Wrap { trim: false }),
        area,
    );
}

pub fn draw_startup_cmds_view(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
    let t = app.theme;

    let mut lines = vec![
        Line::from(Span::styled(
            "startup commands",
            Style::default().fg(t.text).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "commands to run before the agent launches in matching directories",
            Style::default().fg(t.muted),
        )),
        Line::from(""),
    ];

    if let Some(ref state) = app.startup_cmds_adding {
        match state.step {
            StartupCmdAddStep::BrowsePath => {
                lines.push(Line::from(Span::styled(
                    "select directory for startup commands",
                    Style::default().fg(t.accent),
                )));
                lines.push(Line::from(vec![
                    Span::styled("  cwd ", Style::default().fg(t.muted)),
                    Span::styled(
                        format!("{}", state.browser.cwd().display()),
                        Style::default().fg(t.text),
                    ),
                ]));
                lines.push(Line::from(""));

                let entries = state.browser.entries();
                let capacity = area.height.saturating_sub(12) as usize;
                let (start, end) =
                    visible_range(entries.len(), state.browser.selected(), capacity.max(1));

                if start > 0 {
                    lines.push(Line::from(Span::styled("  ...", Style::default().fg(t.muted))));
                }

                for (i, entry) in entries.iter().enumerate().skip(start).take(end - start) {
                    let icon = match entry.kind {
                        EntryKind::SelectCurrent => "\u{2192}",
                        EntryKind::TypePath => "/",
                        EntryKind::Parent => "\u{2190}",
                        EntryKind::Directory => " ",
                        _ => " ",
                    };

                    let style = if i == state.browser.selected() {
                        Style::default().fg(t.bg).bg(t.highlight_bg).add_modifier(Modifier::BOLD)
                    } else if matches!(entry.kind, EntryKind::TypePath) {
                        Style::default().fg(t.accent)
                    } else if matches!(entry.kind, EntryKind::SelectCurrent) {
                        Style::default().fg(t.green)
                    } else {
                        Style::default().fg(t.text)
                    };

                    lines.push(Line::from(Span::styled(
                        format!("  {} {}", icon, entry.label),
                        style,
                    )));
                }

                if end < entries.len() {
                    lines.push(Line::from(Span::styled("  ...", Style::default().fg(t.muted))));
                }

                lines.push(Line::from(""));
                let key_style = Style::default().fg(t.text).add_modifier(Modifier::BOLD);
                let desc_style = Style::default().fg(t.muted);
                lines.push(Line::from(vec![
                    Span::styled("enter", key_style),
                    Span::styled(" select   ", desc_style),
                    Span::styled("\u{2191}/\u{2193}", key_style),
                    Span::styled(" navigate   ", desc_style),
                    Span::styled("esc", key_style),
                    Span::styled(" cancel", desc_style),
                ]));
            }
            StartupCmdAddStep::TypePath => {
                lines.push(Line::from(Span::styled(
                    "enter path (~ supported)",
                    Style::default().fg(t.accent),
                )));
                lines.push(Line::from(Span::styled(
                    if state.current_input.is_empty() {
                        "  _".to_owned()
                    } else {
                        format!("  {}_", state.current_input)
                    },
                    Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
                )));
                lines.push(Line::from(""));
                let key_style = Style::default().fg(t.text).add_modifier(Modifier::BOLD);
                let desc_style = Style::default().fg(t.muted);
                lines.push(Line::from(vec![
                    Span::styled("enter", key_style),
                    Span::styled(" go to path   ", desc_style),
                    Span::styled("esc", key_style),
                    Span::styled(" back", desc_style),
                ]));
            }
            StartupCmdAddStep::Command => {
                lines.push(Line::from(Span::styled(
                    format!("path: {}", state.path),
                    Style::default().fg(t.muted),
                )));
                for cmd in &state.commands {
                    lines.push(Line::from(Span::styled(
                        format!("  + {cmd}"),
                        Style::default().fg(t.green),
                    )));
                }
                lines.push(Line::from(Span::styled(
                    "enter command (empty to finish):",
                    Style::default().fg(t.accent),
                )));
                lines.push(Line::from(Span::styled(
                    format!("  {}_", state.current_input),
                    Style::default().fg(t.text),
                )));
                lines.push(Line::from(""));
                let key_style = Style::default().fg(t.text).add_modifier(Modifier::BOLD);
                let desc_style = Style::default().fg(t.muted);
                lines.push(Line::from(vec![
                    Span::styled("enter", key_style),
                    Span::styled(" add command   ", desc_style),
                    Span::styled("enter", key_style),
                    Span::styled(" (empty) save   ", desc_style),
                    Span::styled("esc", key_style),
                    Span::styled(" cancel", desc_style),
                ]));
            }
        }
    } else if app.config.startup_commands.is_empty() {
        lines.push(Line::from(Span::styled(
            "no startup command rules configured",
            Style::default().fg(t.muted),
        )));
        lines.push(Line::from(""));
        let key_style = Style::default().fg(t.text).add_modifier(Modifier::BOLD);
        let desc_style = Style::default().fg(t.muted);
        lines.push(Line::from(vec![
            Span::styled("a", key_style),
            Span::styled(" add rule   ", desc_style),
            Span::styled("esc", key_style),
            Span::styled(" back", desc_style),
        ]));
    } else {
        for (i, entry) in app.config.startup_commands.iter().enumerate() {
            let selected = i == app.startup_cmds_selected;
            let path_style = if selected {
                Style::default().fg(t.bg).bg(t.highlight_bg).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(t.accent)
            };
            lines.push(Line::from(Span::styled(format!("  {}", entry.path), path_style)));
            for cmd in &entry.commands {
                let cmd_style = if selected {
                    Style::default().fg(t.bg).bg(t.highlight_bg)
                } else {
                    Style::default().fg(t.muted)
                };
                lines.push(Line::from(Span::styled(format!("    $ {cmd}"), cmd_style)));
            }
        }
        lines.push(Line::from(""));
        let key_style = Style::default().fg(t.text).add_modifier(Modifier::BOLD);
        let desc_style = Style::default().fg(t.muted);
        lines.push(Line::from(vec![
            Span::styled("\u{2191}/\u{2193}", key_style),
            Span::styled(" navigate   ", desc_style),
            Span::styled("a", key_style),
            Span::styled(" add   ", desc_style),
            Span::styled("x", key_style),
            Span::styled(" remove   ", desc_style),
            Span::styled("esc", key_style),
            Span::styled(" back", desc_style),
        ]));
    }

    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .style(Style::default().fg(t.text).bg(t.bg))
            .wrap(Wrap { trim: false }),
        area,
    );
}

pub fn draw_dev_servers_view(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
    let t = app.theme;

    let mut lines = vec![
        Line::from(Span::styled(
            "dev servers",
            Style::default().fg(t.text).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "background dev server to start in new worktrees for matching directories",
            Style::default().fg(t.muted),
        )),
        Line::from(""),
    ];

    if let Some(ref state) = app.dev_servers_adding {
        match state.step {
            DevServerAddStep::BrowsePath => {
                lines.push(Line::from(Span::styled(
                    "select directory for dev server",
                    Style::default().fg(t.accent),
                )));
                lines.push(Line::from(vec![
                    Span::styled("  cwd ", Style::default().fg(t.muted)),
                    Span::styled(
                        format!("{}", state.browser.cwd().display()),
                        Style::default().fg(t.text),
                    ),
                ]));
                lines.push(Line::from(""));

                let entries = state.browser.entries();
                let capacity = area.height.saturating_sub(12) as usize;
                let (start, end) =
                    visible_range(entries.len(), state.browser.selected(), capacity.max(1));

                if start > 0 {
                    lines.push(Line::from(Span::styled("  ...", Style::default().fg(t.muted))));
                }

                for (i, entry) in entries.iter().enumerate().skip(start).take(end - start) {
                    let icon = match entry.kind {
                        EntryKind::SelectCurrent => "\u{2192}",
                        EntryKind::TypePath => "/",
                        EntryKind::Parent => "\u{2190}",
                        EntryKind::Directory => " ",
                        _ => " ",
                    };

                    let style = if i == state.browser.selected() {
                        Style::default().fg(t.bg).bg(t.highlight_bg).add_modifier(Modifier::BOLD)
                    } else if matches!(entry.kind, EntryKind::TypePath) {
                        Style::default().fg(t.accent)
                    } else if matches!(entry.kind, EntryKind::SelectCurrent) {
                        Style::default().fg(t.green)
                    } else {
                        Style::default().fg(t.text)
                    };

                    lines.push(Line::from(Span::styled(
                        format!("  {} {}", icon, entry.label),
                        style,
                    )));
                }

                if end < entries.len() {
                    lines.push(Line::from(Span::styled("  ...", Style::default().fg(t.muted))));
                }

                lines.push(Line::from(""));
                let key_style = Style::default().fg(t.text).add_modifier(Modifier::BOLD);
                let desc_style = Style::default().fg(t.muted);
                lines.push(Line::from(vec![
                    Span::styled("enter", key_style),
                    Span::styled(" select   ", desc_style),
                    Span::styled("\u{2191}/\u{2193}", key_style),
                    Span::styled(" navigate   ", desc_style),
                    Span::styled("esc", key_style),
                    Span::styled(" cancel", desc_style),
                ]));
            }
            DevServerAddStep::TypePath => {
                lines.push(Line::from(Span::styled(
                    "enter path (~ supported)",
                    Style::default().fg(t.accent),
                )));
                lines.push(Line::from(Span::styled(
                    if state.current_input.is_empty() {
                        "  _".to_owned()
                    } else {
                        format!("  {}_", state.current_input)
                    },
                    Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
                )));
                lines.push(Line::from(""));
                let key_style = Style::default().fg(t.text).add_modifier(Modifier::BOLD);
                let desc_style = Style::default().fg(t.muted);
                lines.push(Line::from(vec![
                    Span::styled("enter", key_style),
                    Span::styled(" go to path   ", desc_style),
                    Span::styled("esc", key_style),
                    Span::styled(" back", desc_style),
                ]));
            }
            DevServerAddStep::Command => {
                lines.push(Line::from(Span::styled(
                    format!("path: {}", state.path),
                    Style::default().fg(t.muted),
                )));
                lines.push(Line::from(Span::styled(
                    "enter dev server command:",
                    Style::default().fg(t.accent),
                )));
                lines.push(Line::from(Span::styled(
                    format!("  {}_", state.current_input),
                    Style::default().fg(t.text),
                )));
                lines.push(Line::from(""));
                let key_style = Style::default().fg(t.text).add_modifier(Modifier::BOLD);
                let desc_style = Style::default().fg(t.muted);
                lines.push(Line::from(vec![
                    Span::styled("enter", key_style),
                    Span::styled(" save   ", desc_style),
                    Span::styled("esc", key_style),
                    Span::styled(" cancel", desc_style),
                ]));
            }
        }
    } else if app.config.dev_servers.is_empty() {
        lines.push(Line::from(Span::styled(
            "no dev server rules configured",
            Style::default().fg(t.muted),
        )));
        lines.push(Line::from(""));
        let key_style = Style::default().fg(t.text).add_modifier(Modifier::BOLD);
        let desc_style = Style::default().fg(t.muted);
        lines.push(Line::from(vec![
            Span::styled("a", key_style),
            Span::styled(" add rule   ", desc_style),
            Span::styled("esc", key_style),
            Span::styled(" back", desc_style),
        ]));
    } else {
        for (i, entry) in app.config.dev_servers.iter().enumerate() {
            let selected = i == app.dev_servers_selected;
            let path_style = if selected {
                Style::default().fg(t.bg).bg(t.highlight_bg).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(t.accent)
            };
            lines.push(Line::from(Span::styled(format!("  {}", entry.path), path_style)));
            let cmd_style = if selected {
                Style::default().fg(t.bg).bg(t.highlight_bg)
            } else {
                Style::default().fg(t.muted)
            };
            lines.push(Line::from(Span::styled(format!("    $ {}", entry.command), cmd_style)));
        }
        lines.push(Line::from(""));
        let key_style = Style::default().fg(t.text).add_modifier(Modifier::BOLD);
        let desc_style = Style::default().fg(t.muted);
        lines.push(Line::from(vec![
            Span::styled("\u{2191}/\u{2193}", key_style),
            Span::styled(" navigate   ", desc_style),
            Span::styled("a", key_style),
            Span::styled(" add   ", desc_style),
            Span::styled("x", key_style),
            Span::styled(" remove   ", desc_style),
            Span::styled("esc", key_style),
            Span::styled(" back", desc_style),
        ]));
    }

    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .style(Style::default().fg(t.text).bg(t.bg))
            .wrap(Wrap { trim: false }),
        area,
    );
}

pub fn draw_settings_view(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
    let t = app.theme;

    let mut lines = vec![
        Line::from(Span::styled(
            "settings",
            Style::default().fg(t.text).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];

    for i in 0..SETTINGS_COUNT {
        let label = setting_label(i);
        let selected = i == app.settings_selected;

        let is_editing = selected && app.settings_editing.is_some();

        let value_display = if is_editing {
            let buf = app.settings_editing.as_deref().unwrap_or("");
            format!("{}_", buf)
        } else {
            setting_value(&app.config, i)
        };

        let row_style = if selected {
            Style::default().fg(t.bg).bg(t.highlight_bg).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(t.text)
        };

        // For booleans, color the value green/muted when not selected
        let value_style = if selected {
            row_style
        } else if setting_is_bool(i) {
            let on = match i {
                2 => app.config.title_injection_enabled,
                4 => app.config.git_worktrees,
                5 => app.config.notifications.sound_on_completion,
                _ => false,
            };
            if on {
                Style::default().fg(t.green)
            } else {
                Style::default().fg(t.muted)
            }
        } else {
            Style::default().fg(t.muted)
        };

        let padded_label = format!("{:<24}", label);
        lines.push(Line::from(vec![
            Span::styled(padded_label, row_style),
            Span::styled(value_display, value_style),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Custom [[agents]] and theme entries are not editable here \u{2014} edit config.toml directly.",
        Style::default().fg(t.muted),
    )));

    // Footer hints
    lines.push(Line::from(""));
    let key_style = Style::default().fg(t.text).add_modifier(Modifier::BOLD);
    let desc_style = Style::default().fg(t.muted);

    if app.settings_editing.is_some() {
        lines.push(Line::from(vec![
            Span::styled("enter", key_style),
            Span::styled(" save   ", desc_style),
            Span::styled("esc", key_style),
            Span::styled(" discard", desc_style),
        ]));
    } else {
        lines.push(Line::from(vec![
            Span::styled("\u{2191}/\u{2193}", key_style),
            Span::styled(" navigate   ", desc_style),
            Span::styled("enter", key_style),
            Span::styled(" edit/toggle   ", desc_style),
            Span::styled("esc", key_style),
            Span::styled(" back", desc_style),
        ]));
    }

    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .style(Style::default().fg(t.text).bg(t.bg))
            .wrap(Wrap { trim: false }),
        area,
    );
}

pub fn draw_router_settings_view(frame: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
    let t = app.theme;

    let mut lines = vec![
        Line::from(Span::styled(
            "router",
            Style::default().fg(t.text).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "channel orchestrator \u{2014} routes messages to worker instances",
            Style::default().fg(t.muted),
        )),
        Line::from(""),
    ];

    for i in 0..4 {
        let label = router_setting_label(i);
        let selected = i == app.router_settings_selected;
        let is_editing = selected && app.router_settings_editing.is_some();

        let value_display = if is_editing {
            let buf = app.router_settings_editing.as_deref().unwrap_or("");
            format!("{}_", buf)
        } else {
            router_setting_value(&app.config.router, i)
        };

        let row_style = if selected {
            Style::default().fg(t.bg).bg(t.highlight_bg).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(t.text)
        };

        let value_style = if selected {
            row_style
        } else if router_setting_is_bool(i) {
            let on = match i {
                0 => app.config.router.as_ref().map(|r| r.enabled).unwrap_or(false),
                3 => app.config.router.as_ref().map(|r| r.auto_restart).unwrap_or(false),
                _ => false,
            };
            if on {
                Style::default().fg(t.green)
            } else {
                Style::default().fg(t.muted)
            }
        } else {
            Style::default().fg(t.muted)
        };

        let padded_label = format!("{:<16}", label);
        lines.push(Line::from(vec![
            Span::styled(padded_label, row_style),
            Span::styled(value_display, value_style),
        ]));
    }

    // Show configured channels
    let channel_count = app.config.router.as_ref().map(|r| r.channels.len()).unwrap_or(0);
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        format!(
            "channels: {}",
            if channel_count == 0 {
                "none (configure in channels settings)".to_owned()
            } else {
                format!("{channel_count} configured")
            }
        ),
        Style::default().fg(t.muted),
    )));
    if let Some(ref r) = app.config.router {
        for ch in &r.channels {
            lines.push(Line::from(Span::styled(
                format!("  {ch}"),
                Style::default().fg(t.muted),
            )));
        }
    }

    // Router status
    lines.push(Line::from(""));
    let status_line = if app.router_alive {
        Line::from(Span::styled("status: running", Style::default().fg(t.green)))
    } else if app.router_spawning {
        Line::from(Span::styled("status: starting...", Style::default().fg(t.yellow)))
    } else if app.is_router_enabled() {
        Line::from(Span::styled("status: offline", Style::default().fg(t.red)))
    } else {
        Line::from(Span::styled("status: disabled", Style::default().fg(t.muted)))
    };
    lines.push(status_line);

    // Footer hints
    lines.push(Line::from(""));
    let key_style = Style::default().fg(t.text).add_modifier(Modifier::BOLD);
    let desc_style = Style::default().fg(t.muted);

    if app.router_settings_editing.is_some() {
        lines.push(Line::from(vec![
            Span::styled("enter", key_style),
            Span::styled(" save   ", desc_style),
            Span::styled("esc", key_style),
            Span::styled(" discard", desc_style),
        ]));
    } else {
        lines.push(Line::from(vec![
            Span::styled("\u{2191}/\u{2193}", key_style),
            Span::styled(" navigate   ", desc_style),
            Span::styled("enter", key_style),
            Span::styled(" edit/toggle   ", desc_style),
            Span::styled("esc", key_style),
            Span::styled(" back", desc_style),
        ]));
    }

    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .style(Style::default().fg(t.text).bg(t.bg))
            .wrap(Wrap { trim: false }),
        area,
    );
}
