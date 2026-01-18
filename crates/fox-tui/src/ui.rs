//! TUI rendering with Ratatui

use crate::app::App;
use crate::vim::VimMode;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Tabs},
    Frame,
};

/// Draw the complete UI
pub fn draw(frame: &mut Frame, app: &mut App) {
    let size = frame.size();
    app.set_viewport_size(size.width, size.height);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Tab bar
            Constraint::Min(1),    // Content
            Constraint::Length(1), // Status bar
            Constraint::Length(1), // Command/input line
        ])
        .split(size);

    draw_tab_bar(frame, app, chunks[0]);
    draw_content(frame, app, chunks[1]);
    draw_status_bar(frame, app, chunks[2]);
    draw_command_line(frame, app, chunks[3]);
}

fn draw_tab_bar(frame: &mut Frame, app: &App, area: Rect) {
    let titles: Vec<Line> = app
        .tabs
        .tabs()
        .iter()
        .enumerate()
        .map(|(i, tab)| {
            let title = tab.title().unwrap_or("New Tab");
            let truncated = if title.len() > 20 {
                format!("{}...", &title[..17])
            } else {
                title.to_string()
            };
            let num = format!("{}: ", i + 1);
            Line::from(vec![
                Span::styled(num, Style::default().fg(Color::DarkGray)),
                Span::raw(truncated),
            ])
        })
        .collect();

    let tabs = Tabs::new(titles)
        .select(app.tabs.current_index())
        .style(Style::default().fg(Color::White))
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .divider("|");

    frame.render_widget(tabs, area);
}

fn draw_content(frame: &mut Frame, app: &App, area: Rect) {
    let tab = app.tabs.current();

    // Split content area for scrollbar
    let content_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(area);

    // Get link hints if in hint mode
    let hint_chars: std::collections::HashMap<usize, char> = app
        .link_hints
        .iter()
        .filter_map(|(c, link)| {
            // Find which line contains this link
            let content = tab.content()?;
            let line_num = content[..link.position.min(content.len())]
                .lines()
                .count()
                .saturating_sub(1);
            Some((line_num, *c))
        })
        .collect();

    // Render content
    let lines: Vec<Line> = tab
        .visible_lines()
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let line_idx = tab.scroll_offset() + i;
            let mut spans = vec![];

            // Add hint character if in hint mode
            if app.vim.mode == VimMode::Hint {
                if let Some(&hint) = hint_chars.get(&line_idx) {
                    spans.push(Span::styled(
                        format!("[{}]", hint),
                        Style::default()
                            .fg(Color::Black)
                            .bg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ));
                    spans.push(Span::raw(" "));
                }
            }

            // Style the content
            let styled_line = style_markdown_line(line);
            spans.extend(styled_line);

            Line::from(spans)
        })
        .collect();

    let content_block = Block::default()
        .borders(Borders::NONE);

    let paragraph = Paragraph::new(lines).block(content_block);

    frame.render_widget(paragraph, content_chunks[0]);

    // Render scrollbar
    let scrollbar = Scrollbar::default()
        .orientation(ScrollbarOrientation::VerticalRight)
        .begin_symbol(Some("↑"))
        .end_symbol(Some("↓"));

    let mut scrollbar_state = ScrollbarState::new(tab.total_lines())
        .position(tab.scroll_offset());

    frame.render_stateful_widget(scrollbar, content_chunks[1], &mut scrollbar_state);
}

fn draw_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let tab = app.tabs.current();

    let mode_style = match app.vim.mode {
        VimMode::Normal => Style::default().fg(Color::Green),
        VimMode::Command => Style::default().fg(Color::Yellow),
        VimMode::Search => Style::default().fg(Color::Cyan),
        VimMode::Insert => Style::default().fg(Color::Magenta),
        VimMode::Hint => Style::default().fg(Color::Red),
    };

    let mode_span = Span::styled(
        format!(" {} ", app.vim.mode.indicator()),
        mode_style.add_modifier(Modifier::BOLD),
    );

    let url = tab.url().unwrap_or_else(|| "about:blank".to_string());
    let url_span = Span::styled(format!(" {} ", url), Style::default().fg(Color::Blue));

    let progress = format!(
        " {}% ",
        if tab.total_lines() > 0 {
            ((tab.scroll_offset() + tab.viewport_height) * 100 / tab.total_lines().max(1)).min(100)
        } else {
            100
        }
    );
    let progress_span = Span::styled(progress, Style::default().fg(Color::DarkGray));

    let loading_span = if app.loading {
        Span::styled(" Loading... ", Style::default().fg(Color::Yellow))
    } else {
        Span::raw("")
    };

    // Build status line
    let mut spans = vec![mode_span, url_span, loading_span];

    // Add link info if selected
    if let Some(link) = tab.selected_link() {
        let link_info = format!(" → {} ", link.url);
        spans.push(Span::styled(link_info, Style::default().fg(Color::Cyan)));
    }

    spans.push(progress_span);

    let status = Paragraph::new(Line::from(spans))
        .style(Style::default().bg(Color::DarkGray));

    frame.render_widget(status, area);
}

fn draw_command_line(frame: &mut Frame, app: &App, area: Rect) {
    let content = match app.vim.mode {
        VimMode::Command => format!(":{}", app.input),
        VimMode::Search => format!("/{}", app.input),
        _ => app.status.clone().unwrap_or_default(),
    };

    let style = match app.vim.mode {
        VimMode::Command | VimMode::Search => Style::default().fg(Color::White),
        _ => Style::default().fg(Color::DarkGray),
    };

    let paragraph = Paragraph::new(content).style(style);
    frame.render_widget(paragraph, area);

    // Show cursor in command/search mode
    if matches!(app.vim.mode, VimMode::Command | VimMode::Search) {
        let cursor_x = area.x + app.input.len() as u16 + 1; // +1 for : or /
        frame.set_cursor(cursor_x, area.y);
    }
}

/// Style a markdown line with colors
fn style_markdown_line(line: &str) -> Vec<Span<'static>> {
    let mut spans = Vec::new();

    // Headers
    if line.starts_with("# ") {
        spans.push(Span::styled(
            line.to_string(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
        return spans;
    }
    if line.starts_with("## ") || line.starts_with("### ") {
        spans.push(Span::styled(
            line.to_string(),
            Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::BOLD),
        ));
        return spans;
    }
    if line.starts_with("#### ") || line.starts_with("##### ") || line.starts_with("###### ") {
        spans.push(Span::styled(
            line.to_string(),
            Style::default().fg(Color::Blue),
        ));
        return spans;
    }

    // Code blocks
    if line.starts_with("```") || line.starts_with("    ") {
        spans.push(Span::styled(
            line.to_string(),
            Style::default().fg(Color::Green),
        ));
        return spans;
    }

    // Lists
    if line.trim_start().starts_with("- ") || line.trim_start().starts_with("* ") {
        let indent = line.len() - line.trim_start().len();
        if indent > 0 {
            spans.push(Span::raw(" ".repeat(indent)));
        }
        spans.push(Span::styled(
            "•".to_string(),
            Style::default().fg(Color::Yellow),
        ));
        let rest = line.trim_start();
        let content = rest.strip_prefix("- ")
            .or_else(|| rest.strip_prefix("* "))
            .unwrap_or("")
            .to_string();
        spans.push(Span::raw(content));
        return spans;
    }

    // Blockquotes
    if line.starts_with("> ") {
        spans.push(Span::styled(
            line.to_string(),
            Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
        ));
        return spans;
    }

    // Links - simple detection
    if line.contains("](") {
        // Basic link highlighting
        let line_owned = line.to_string();
        let mut remaining = line_owned.as_str();

        while let Some(link_start) = remaining.find('[') {
            // Add text before link
            if link_start > 0 {
                spans.push(Span::raw(remaining[..link_start].to_string()));
            }

            if let Some(link_end) = remaining[link_start..].find(')') {
                let link_text = &remaining[link_start..link_start + link_end + 1];
                spans.push(Span::styled(
                    link_text.to_string(),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::UNDERLINED),
                ));
                let new_pos = link_start + link_end + 1;
                remaining = &remaining[new_pos..];
            } else {
                spans.push(Span::raw(remaining[link_start..].to_string()));
                return spans;
            }
        }
        if !remaining.is_empty() {
            spans.push(Span::raw(remaining.to_string()));
        }
        return spans;
    }

    // Bold text
    if line.contains("**") {
        let parts: Vec<&str> = line.split("**").collect();
        for (i, part) in parts.iter().enumerate() {
            if i % 2 == 1 {
                spans.push(Span::styled(
                    part.to_string(),
                    Style::default().add_modifier(Modifier::BOLD),
                ));
            } else {
                spans.push(Span::raw(part.to_string()));
            }
        }
        return spans;
    }

    // Default
    spans.push(Span::raw(line.to_string()));
    spans
}
