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

    // Calculate suggestion height
    let suggestion_height = if app.vim.mode == VimMode::Command && !app.url_suggestions.is_empty() {
        (app.url_suggestions.len() as u16).min(10).min(size.height.saturating_sub(4))
    } else {
        0
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),                    // Tab bar
            Constraint::Min(1),                       // Content
            Constraint::Length(1),                    // Status bar
            Constraint::Length(suggestion_height),   // URL suggestions
            Constraint::Length(1),                    // Command/input line
        ])
        .split(size);

    draw_tab_bar(frame, app, chunks[0]);
    draw_content(frame, app, chunks[1]);
    draw_status_bar(frame, app, chunks[2]);

    if suggestion_height > 0 {
        draw_suggestions(frame, app, chunks[3]);
    }

    draw_command_line(frame, app, chunks[4]);
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

    // Get link hints if in hint mode - now supports multi-letter hints
    // Use Vec to store multiple hints per line
    let mut hint_strings: std::collections::HashMap<usize, Vec<String>> = std::collections::HashMap::new();
    for (hint, link) in app.link_hints.iter() {
        if let Some(content) = tab.content() {
            let line_num = content[..link.position.min(content.len())]
                .lines()
                .count()
                .saturating_sub(1);
            hint_strings.entry(line_num).or_default().push(hint.clone());
        }
    }

    // Current hint input for highlighting matched prefix
    let hint_input = &app.hint_input;

    // Render content
    let lines: Vec<Line> = tab
        .visible_lines()
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let line_idx = tab.scroll_offset() + i;
            let mut spans = vec![];

            // Add hint labels if in hint mode (multiple hints per line)
            if app.vim.mode == VimMode::Hint {
                if let Some(hints) = hint_strings.get(&line_idx) {
                    for hint in hints {
                        // Only show hints that match the current input prefix
                        if hint.starts_with(hint_input) {
                            // Split hint into matched and unmatched parts
                            let matched = &hint[..hint_input.len()];
                            let remaining = &hint[hint_input.len()..];

                            // Show matched part in dim style (already typed)
                            if !matched.is_empty() {
                                spans.push(Span::styled(
                                    matched.to_string(),
                                    Style::default()
                                        .fg(Color::DarkGray)
                                        .bg(Color::Yellow),
                                ));
                            }

                            // Show remaining part in bold (still to type)
                            if !remaining.is_empty() {
                                spans.push(Span::styled(
                                    remaining.to_string(),
                                    Style::default()
                                        .fg(Color::Black)
                                        .bg(Color::Yellow)
                                        .add_modifier(Modifier::BOLD),
                                ));
                            }

                            spans.push(Span::raw(" "));
                        }
                    }
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
        VimMode::Hint => {
            if app.hint_input.is_empty() {
                "Follow hint...".to_string()
            } else {
                format!("Follow hint: {}", app.hint_input)
            }
        }
        _ => app.status.clone().unwrap_or_default(),
    };

    let style = match app.vim.mode {
        VimMode::Command | VimMode::Search => Style::default().fg(Color::White),
        VimMode::Hint => Style::default().fg(Color::Yellow),
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

/// Draw URL suggestions popup for :o command
fn draw_suggestions(frame: &mut Frame, app: &App, area: Rect) {
    let lines: Vec<Line> = app.url_suggestions
        .iter()
        .enumerate()
        .map(|(i, suggestion)| {
            let is_selected = i == app.suggestion_index;

            // Build display text with URL and optional title
            let display = if let Some(title) = &suggestion.title {
                format!("{} - {}", suggestion.url, title)
            } else {
                suggestion.url.clone()
            };

            // Truncate if too long
            let max_width = area.width.saturating_sub(2) as usize;
            let display = if display.len() > max_width {
                format!("{}...", &display[..max_width.saturating_sub(3)])
            } else {
                display
            };

            let style = if is_selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White).bg(Color::DarkGray)
            };

            Line::from(Span::styled(display, style))
        })
        .collect();

    let suggestions = Paragraph::new(lines)
        .style(Style::default().bg(Color::DarkGray));

    frame.render_widget(suggestions, area);
}

/// Style a line with colors (content is already plain text with markdown stripped)
fn style_markdown_line(line: &str) -> Vec<Span<'static>> {
    let mut spans = Vec::new();

    // Code blocks (indented lines)
    if line.starts_with("    ") {
        spans.push(Span::styled(
            line.to_string(),
            Style::default().fg(Color::Green),
        ));
        return spans;
    }

    // Lists with bullet points
    if line.trim_start().starts_with("- ") || line.trim_start().starts_with("* ") {
        let indent = line.len() - line.trim_start().len();
        if indent > 0 {
            spans.push(Span::raw(" ".repeat(indent)));
        }
        spans.push(Span::styled(
            "• ".to_string(),
            Style::default().fg(Color::Yellow),
        ));
        let rest = line.trim_start();
        let content = rest.strip_prefix("- ")
            .or_else(|| rest.strip_prefix("* "))
            .unwrap_or("");
        spans.push(Span::raw(content.to_string()));
        return spans;
    }

    // Numbered lists (like HN stories: "1.", "2.", etc.)
    let trimmed = line.trim_start();
    if let Some(dot_pos) = trimmed.find('.') {
        if dot_pos > 0 && dot_pos <= 3 && trimmed[..dot_pos].chars().all(|c| c.is_ascii_digit()) {
            let indent = line.len() - trimmed.len();
            if indent > 0 {
                spans.push(Span::raw(" ".repeat(indent)));
            }
            spans.push(Span::styled(
                trimmed[..dot_pos + 1].to_string(),
                Style::default().fg(Color::Yellow),
            ));
            spans.push(Span::raw(trimmed[dot_pos + 1..].to_string()));
            return spans;
        }
    }

    // Blockquotes (lines starting with │)
    if line.starts_with("│ ") {
        spans.push(Span::styled(
            line.to_string(),
            Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
        ));
        return spans;
    }

    // Image placeholders [alt text]
    if line.contains("[") && line.contains("]") && !line.contains("](") {
        let line_owned = line.to_string();
        let mut remaining = line_owned.as_str();

        while let Some(start) = remaining.find('[') {
            if start > 0 {
                spans.push(Span::raw(remaining[..start].to_string()));
            }
            if let Some(end) = remaining[start..].find(']') {
                let bracket_content = &remaining[start..start + end + 1];
                spans.push(Span::styled(
                    bracket_content.to_string(),
                    Style::default().fg(Color::DarkGray),
                ));
                remaining = &remaining[start + end + 1..];
            } else {
                spans.push(Span::raw(remaining[start..].to_string()));
                return spans;
            }
        }
        if !remaining.is_empty() {
            spans.push(Span::raw(remaining.to_string()));
        }
        return spans;
    }

    // Separator lines
    if line.trim() == "---" || line.trim() == "***" || line.trim() == "___" {
        spans.push(Span::styled(
            "─".repeat(40),
            Style::default().fg(Color::DarkGray),
        ));
        return spans;
    }

    // Default - just show the text
    spans.push(Span::raw(line.to_string()));
    spans
}
