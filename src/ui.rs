use ratatui::{
    layout::{Constraint, Direction, Flex, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
    Frame,
};

use crate::app::{App, FilterKind, LoadState, Mode};

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

    if let Mode::Filter {
        kind,
        options,
        selected,
        checked,
    } = &app.mode
    {
        draw_filter_popup(frame, area, *kind, options, *selected, checked);
    }
}

fn draw_filter_popup(
    frame: &mut Frame,
    area: Rect,
    kind: FilterKind,
    options: &[String],
    selected: usize,
    checked: &[bool],
) {
    let title = match kind {
        FilterKind::Team => "Filter by Team",
        FilterKind::Project => "Filter by Project",
        FilterKind::Status => "Filter by Status (space: toggle, enter: apply)",
    };

    let height = (options.len() as u16 + 2).min(area.height.saturating_sub(2)).max(3);
    let width = 50.min(area.width.saturating_sub(2)).max(20);

    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .flex(Flex::Center)
        .constraints([Constraint::Length(height)])
        .split(area);
    let popup_area = Layout::default()
        .direction(Direction::Horizontal)
        .flex(Flex::Center)
        .constraints([Constraint::Length(width)])
        .split(vertical[0])[0];

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
        LoadState::Loaded => {
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
                .split(area);

            draw_issue_list(frame, app, chunks[0]);
            draw_issue_detail(frame, app, chunks[1]);
        }
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
            let line = Line::from(vec![
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
    if let Some(t) = &app.team_filter {
        title.push_str(&format!(" | team: {t}"));
    }
    if let Some(p) = &app.project_filter {
        title.push_str(&format!(" | project: {p}"));
    }
    if !app.status_filter.is_empty() {
        title.push_str(&format!(" | status: {}", app.status_filter.join(", ")));
    }
    let list = List::new(items).block(Block::default().borders(Borders::ALL).title(title));
    frame.render_widget(list, area);
}

fn draw_issue_detail(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default().borders(Borders::ALL).title("Details");

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

    let p = Paragraph::new(lines).block(block).wrap(Wrap { trim: false });
    frame.render_widget(p, area);
}

fn draw_footer(frame: &mut Frame, app: &App, area: Rect) {
    let text = match &app.mode {
        Mode::Normal => {
            "j/k: navigate   t: filter team   p: filter project   s: filter status   c: clear filters   r: refresh   q: quit"
        }
        Mode::Filter { kind, .. } if kind.is_multi() => {
            "j/k: navigate   space: toggle   enter: apply   esc: cancel"
        }
        Mode::Filter { .. } => "j/k: navigate   enter: select   esc: cancel",
    };
    let p = Paragraph::new(text).style(Style::default().fg(Color::DarkGray));
    frame.render_widget(p, area);
}
