use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Parser, Tag, TagEnd};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

pub fn render(source: &str) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut current: Vec<Span<'static>> = Vec::new();

    let mut style_stack: Vec<Style> = vec![Style::default()];
    let mut in_code_block = false;
    let mut list_stack: Vec<Option<u64>> = Vec::new();

    macro_rules! flush_line {
        () => {{
            lines.push(Line::from(std::mem::take(&mut current)));
        }};
    }

    let style = |stack: &Vec<Style>| -> Style {
        stack.last().copied().unwrap_or_default()
    };

    for event in Parser::new(source) {
        match event {
            Event::Start(tag) => match tag {
                Tag::Heading { level, .. } => {
                    if !current.is_empty() {
                        flush_line!();
                    }
                    let prefix = match level {
                        HeadingLevel::H1 => "# ",
                        HeadingLevel::H2 => "## ",
                        HeadingLevel::H3 => "### ",
                        _ => "#### ",
                    };
                    let s = Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::BOLD);
                    style_stack.push(s);
                    current.push(Span::styled(prefix, s));
                }
                Tag::Emphasis => {
                    let s = style(&style_stack).add_modifier(Modifier::ITALIC);
                    style_stack.push(s);
                }
                Tag::Strong => {
                    let s = style(&style_stack).add_modifier(Modifier::BOLD);
                    style_stack.push(s);
                }
                Tag::Strikethrough => {
                    let s = style(&style_stack).add_modifier(Modifier::CROSSED_OUT);
                    style_stack.push(s);
                }
                Tag::CodeBlock(kind) => {
                    if !current.is_empty() {
                        flush_line!();
                    }
                    in_code_block = true;
                    let lang = match kind {
                        CodeBlockKind::Fenced(l) if !l.is_empty() => format!("[{l}]"),
                        _ => String::new(),
                    };
                    lines.push(Line::styled(
                        format!("```{lang}"),
                        Style::default().fg(Color::DarkGray),
                    ));
                }
                Tag::Item => {
                    if !current.is_empty() {
                        flush_line!();
                    }
                    let depth = list_stack.len().saturating_sub(1);
                    let indent = "  ".repeat(depth);
                    match list_stack.last_mut() {
                        Some(Some(n)) => {
                            current.push(Span::raw(format!("{indent}{n}. ")));
                            *n += 1;
                        }
                        _ => {
                            current.push(Span::raw(format!("{indent}- ")));
                        }
                    }
                }
                Tag::List(start) => {
                    list_stack.push(start);
                }
                Tag::Link { .. } => {
                    let s = style(&style_stack)
                        .fg(Color::Blue)
                        .add_modifier(Modifier::UNDERLINED);
                    style_stack.push(s);
                }
                Tag::BlockQuote(_) => {
                    if !current.is_empty() {
                        flush_line!();
                    }
                    current.push(Span::styled(
                        "> ",
                        Style::default().fg(Color::DarkGray),
                    ));
                }
                Tag::Paragraph => {}
                _ => {}
            },
            Event::End(tag_end) => match tag_end {
                TagEnd::Heading(_) => {
                    flush_line!();
                    style_stack.pop();
                }
                TagEnd::Emphasis
                | TagEnd::Strong
                | TagEnd::Strikethrough
                | TagEnd::Link => {
                    style_stack.pop();
                }
                TagEnd::CodeBlock => {
                    in_code_block = false;
                    lines.push(Line::styled(
                        "```",
                        Style::default().fg(Color::DarkGray),
                    ));
                }
                TagEnd::Item => {
                    flush_line!();
                }
                TagEnd::List(_) => {
                    list_stack.pop();
                }
                TagEnd::Paragraph | TagEnd::BlockQuote(_) => {
                    flush_line!();
                    lines.push(Line::raw(""));
                }
                _ => {}
            },
            Event::Text(text) => {
                if in_code_block {
                    for (i, l) in text.split('\n').enumerate() {
                        if i > 0 {
                            flush_line!();
                        }
                        if !l.is_empty() {
                            current.push(Span::styled(
                                l.to_string(),
                                Style::default().fg(Color::Green),
                            ));
                        }
                    }
                } else {
                    current.push(Span::styled(text.to_string(), style(&style_stack)));
                }
            }
            Event::Code(text) => {
                current.push(Span::styled(
                    text.to_string(),
                    Style::default().fg(Color::Green).bg(Color::Black),
                ));
            }
            Event::SoftBreak => current.push(Span::raw(" ")),
            Event::HardBreak => flush_line!(),
            Event::Rule => {
                if !current.is_empty() {
                    flush_line!();
                }
                lines.push(Line::styled(
                    "─".repeat(40),
                    Style::default().fg(Color::DarkGray),
                ));
            }
            _ => {}
        }
    }

    if !current.is_empty() {
        flush_line!();
    }

    while lines.last().is_some_and(|l| l.spans.is_empty()) {
        lines.pop();
    }

    lines
}
