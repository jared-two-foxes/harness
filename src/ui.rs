use ratatui::{
    layout::{Constraint, Direction, Flex, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

use crate::app::{App, FilterKind, LoadState, Mode, FILTER_KINDS};

fn priority_label(p: f64) -> &'static str {
    match p as i64 {
        1 => "Urgent",
        2 => "High",
        3 => "Medium",
        4 => "Low",
        _ => "None",
    }
}

fn priority_color(p: f64) -> Color {
    match p as i64 {
        1 => Color::Red,
        2 => Color::Yellow,
        3 => Color::Cyan,
        4 => Color::Gray,
        _ => Color::DarkGray,
    }
}

pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(area);

    draw_body(frame, app, chunks[0]);
    draw_footer(frame, app, chunks[1]);

    match &app.mode {
        Mode::FilterMenu { selected } => draw_filter_menu(frame, area, app, *selected),
        Mode::Filter {
            kind,
            options,
            selected,
            checked,
        } => draw_filter_popup(frame, area, *kind, options, *selected, checked),
        Mode::Detail | Mode::Normal | Mode::ExtensionOutput { .. } => {}
    }
}

fn centered_popup(area: Rect, height: u16, width: u16) -> Rect {
    let height = height.min(area.height.saturating_sub(2)).max(3);
    let width = width.min(area.width.saturating_sub(2)).max(20);

    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .flex(Flex::Center)
        .constraints([Constraint::Length(height)])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .flex(Flex::Center)
        .constraints([Constraint::Length(width)])
        .split(vertical[0])[0]
}

fn draw_filter_menu(frame: &mut Frame, area: Rect, app: &App, selected: usize) {
    let popup_area = centered_popup(area, FILTER_KINDS.len() as u16 + 2, 50);

    let items: Vec<ListItem> = FILTER_KINDS
        .iter()
        .enumerate()
        .map(|(i, &kind)| {
            let style = if i == selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let active = app.filters.is_active(kind);
            let line = Line::from(vec![
                Span::raw(format!("{:<9}", kind.label())),
                Span::styled(
                    app.filters.summary(kind),
                    if active {
                        Style::default().fg(Color::Cyan)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    },
                ),
            ]);
            ListItem::new(line).style(style)
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Filters (enter: edit, c: clear all, esc: close)")
            .style(Style::default().bg(Color::Black)),
    );

    frame.render_widget(Clear, popup_area);
    frame.render_widget(list, popup_area);
}

fn draw_filter_popup(
    frame: &mut Frame,
    area: Rect,
    kind: FilterKind,
    options: &[String],
    selected: usize,
    checked: &[bool],
) {
    let title = if kind.is_multi() {
        format!("Filter by {} (space: toggle, enter: apply)", kind.label())
    } else {
        format!("Filter by {}", kind.label())
    };

    let popup_area = centered_popup(area, options.len() as u16 + 2, 50);

    let items: Vec<ListItem> = options
        .iter()
        .enumerate()
        .map(|(i, opt)| {
            let style = if i == selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let label = if kind.is_multi() {
                let mark = if checked.get(i).copied().unwrap_or(false) {
                    "[x] "
                } else {
                    "[ ] "
                };
                format!("{mark}{opt}")
            } else {
                opt.clone()
            };
            ListItem::new(label).style(style)
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(title)
            .style(Style::default().bg(Color::Black)),
    );

    frame.render_widget(Clear, popup_area);
    frame.render_widget(list, popup_area);
}

fn draw_body(frame: &mut Frame, app: &App, area: Rect) {
    match &app.load_state {
        LoadState::Loading => {
            let p = Paragraph::new("Loading issues from Linear...")
                .block(Block::default().borders(Borders::ALL).title("harness"));
            frame.render_widget(p, area);
        }
        LoadState::Error(msg) => {
            let p = Paragraph::new(msg.as_str())
                .style(Style::default().fg(Color::Red))
                .wrap(Wrap { trim: true })
                .block(Block::default().borders(Borders::ALL).title("Error"));
            frame.render_widget(p, area);
        }
        LoadState::Loaded => match &app.mode {
            Mode::Detail => draw_detail_view(frame, app, area),
            Mode::ExtensionOutput { scroll } => draw_extension_output(frame, area, app, *scroll),
            _ => draw_issue_list(frame, app, area),
        },
    }
}

fn draw_issue_list(frame: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .issues
        .iter()
        .enumerate()
        .map(|(i, issue)| {
            let style = if i == app.selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let blocked_marker = if issue.is_blocked() { "! " } else { "  " };
            let line = Line::from(vec![
                Span::styled(
                    blocked_marker,
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{:<8}", issue.identifier),
                    Style::default().fg(Color::Blue),
                ),
                Span::raw(issue.title.clone()),
            ]);
            ListItem::new(line).style(style)
        })
        .collect();

    let mut title = format!("Issues ({})", app.issues.len());
    title.push_str(&format!(" | sort: {}", app.sort_key.label()));
    for &kind in FILTER_KINDS.iter() {
        if app.filters.is_active(kind) {
            title.push_str(&format!(
                " | {}: {}",
                kind.label().to_lowercase(),
                app.filters.summary(kind)
            ));
        }
    }
    let block = Block::default().borders(Borders::ALL).title(title);

    if items.is_empty() {
        let p = Paragraph::new("No issues match the current filters.")
            .style(Style::default().fg(Color::DarkGray))
            .block(block);
        frame.render_widget(p, area);
        return;
    }

    let list = List::new(items).block(block);
    let mut state = ListState::default().with_selected(Some(app.selected));
    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_detail_view(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title("Details (j/k: scroll, esc: back to list)");

    let Some(issue) = app.selected_issue() else {
        frame.render_widget(Paragraph::new("No issue selected").block(block), area);
        return;
    };

    let mut lines = vec![
        Line::from(vec![
            Span::styled("Title: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(issue.title.clone()),
        ]),
        Line::from(vec![
            Span::styled("ID: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(issue.identifier.clone()),
        ]),
        Line::from(vec![
            Span::styled("Team: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(format!("{} ({})", issue.team.name, issue.team.key)),
        ]),
        Line::from(vec![
            Span::styled("Assignee: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(
                issue
                    .assignee
                    .as_ref()
                    .map(|a| a.name.clone())
                    .unwrap_or_else(|| "Unassigned".to_string()),
            ),
        ]),
        Line::from(vec![
            Span::styled("State: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(format!("{} [{}]", issue.state.name, issue.state.state_type)),
        ]),
        Line::from(vec![
            Span::styled("Priority: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(
                priority_label(issue.priority),
                Style::default().fg(priority_color(issue.priority)),
            ),
        ]),
        Line::from(vec![
            Span::styled("Blocked: ", Style::default().add_modifier(Modifier::BOLD)),
            if issue.is_blocked() {
                Span::styled("Yes", Style::default().fg(Color::Red))
            } else {
                Span::styled("No", Style::default().fg(Color::Green))
            },
        ]),
        Line::from(vec![
            Span::styled("Updated: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(issue.updated_at.clone()),
        ]),
        Line::from(vec![
            Span::styled("URL: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(issue.url.clone()),
        ]),
        Line::raw(""),
    ];

    if let Some(desc) = &issue.description {
        lines.push(Line::styled(
            "Description:",
            Style::default().add_modifier(Modifier::BOLD),
        ));
        lines.push(Line::raw(""));
        lines.extend(crate::markdown::render(desc));
    }

    let p = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((app.detail_scroll, 0));
    frame.render_widget(p, area);
}

fn draw_extension_output(frame: &mut Frame, area: Rect, app: &App, scroll: u16) {
    let Some(run) = &app.extension_run else {
        frame.render_widget(
            Paragraph::new("No extension output").block(Block::default().borders(Borders::ALL)),
            area,
        );
        return;
    };

    let title = if run.running {
        format!("Running: {}... (K: kill)", run.name)
    } else if run.success {
        format!("{} (done) — line {}", run.name, scroll + 1)
    } else {
        format!("{} (failed) — line {}", run.name, scroll + 1)
    };
    let title_style = if run.running {
        Style::default().fg(Color::Yellow)
    } else if run.success {
        Style::default().fg(Color::Green)
    } else {
        Style::default().fg(Color::Red)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(title, title_style.add_modifier(Modifier::BOLD)));

    let mut lines: Vec<Line> = run
        .lines
        .iter()
        .map(|(is_stderr, text)| {
            if *is_stderr {
                Line::styled(text.clone(), Style::default().fg(Color::Red))
            } else {
                Line::raw(text.clone())
            }
        })
        .collect();

    if lines.is_empty() {
        let text = if run.running {
            "Waiting for output..."
        } else {
            "(no output)"
        };
        lines.push(Line::styled(text, Style::default().fg(Color::DarkGray)));
    }

    let p = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    frame.render_widget(p, area);
}

fn extension_hints(app: &App) -> String {
    app.extensions
        .iter()
        .map(|e| {
            if e.description.is_empty() {
                format!("{}: {}", e.key, e.name)
            } else {
                format!("{}: {} ({})", e.key, e.name, e.description)
            }
        })
        .collect::<Vec<_>>()
        .join("   ")
}

fn draw_footer(frame: &mut Frame, app: &App, area: Rect) {
    let base = match &app.mode {
        Mode::Normal => {
            "j/k: navigate   enter: view details   f: filters   o: sort   r: refresh   q: quit"
                .to_string()
        }
        Mode::Detail => {
            "j/k: scroll   u/d: page up/down   g/G: top/bottom   esc: back to list".to_string()
        }
        Mode::ExtensionOutput { .. } => {
            let running = app.extension_run.as_ref().is_some_and(|r| r.running);
            if running {
                "j/k: scroll   u/d: page up/down   g/G: top/bottom   K: kill   esc: back to list"
                    .to_string()
            } else {
                "j/k: scroll   u/d: page up/down   g/G: top/bottom   esc: back to list"
                    .to_string()
            }
        }
        Mode::FilterMenu { .. } => {
            "j/k: navigate   enter: edit   c: clear all   esc: close".to_string()
        }
        Mode::Filter { kind, .. } if kind.is_multi() => {
            "j/k: navigate   space: toggle   enter: apply   esc: back".to_string()
        }
        Mode::Filter { .. } => "j/k: navigate   enter: select   esc: back".to_string(),
    };

    let mut text = base;
    if matches!(app.mode, Mode::Normal | Mode::Detail) {
        if let Some(run) = &app.extension_run {
            if run.running {
                text.push_str(&format!("   |   ● {} running...", run.name));
            }
        }
        let hints = extension_hints(app);
        if !hints.is_empty() {
            text.push_str(&format!("   |   {hints}"));
        }
    }

    let p = Paragraph::new(text).style(Style::default().fg(Color::DarkGray));
    frame.render_widget(p, area);
}
