use crate::api::models::LogEntry;
use crate::state::LogsState;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, Row, Table, Wrap},
    Frame,
};

/// Render the logs view
pub fn render_logs_view(frame: &mut Frame, area: Rect, state: &LogsState) {
    // Split the area into main content and status bar
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),    // Main content
            Constraint::Length(1), // Status bar
        ])
        .split(area);

    // Render main content (table or table + detail)
    if state.show_detail {
        render_logs_with_detail(frame, chunks[0], state);
    } else {
        render_logs_table(frame, chunks[0], state);
    }

    // Render status bar
    render_status_bar(frame, chunks[1], state);
}

/// Render logs table only
fn render_logs_table(frame: &mut Frame, area: Rect, state: &LogsState) {
    let filtered_logs = state.filtered_logs();

    // Create table rows
    let rows: Vec<Row> = filtered_logs
        .iter()
        .enumerate()
        .map(|(idx, log)| {
            let style = if idx == state.selected_index {
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            let _severity_style = get_severity_style(&log.severity);

            Row::new(vec![
                format_timestamp(log.timestamp),
                log.severity.clone(),
                truncate_string(&log.body, 80),
            ])
            .style(style)
            .height(1)
        })
        .collect();

    // Create table header
    let header = Row::new(vec!["Timestamp", "Severity", "Message"])
        .style(Style::default().add_modifier(Modifier::BOLD))
        .bottom_margin(1);

    // Create table widget
    let table = Table::new(
        rows,
        [
            Constraint::Length(20), // Timestamp
            Constraint::Length(10), // Severity
            Constraint::Min(50),    // Message
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" Logs ({}) ", filtered_logs.len())),
    )
    .row_highlight_style(
        Style::default()
            .bg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
    );

    frame.render_widget(table, area);
}

/// Render logs table with detail panel
fn render_logs_with_detail(frame: &mut Frame, area: Rect, state: &LogsState) {
    // Split area horizontally
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(60), // Table
            Constraint::Percentage(40), // Detail
        ])
        .split(area);

    // Render table
    render_logs_table(frame, chunks[0], state);

    // Render detail panel
    render_detail_panel(frame, chunks[1], state);
}

/// Render log detail panel
fn render_detail_panel(frame: &mut Frame, area: Rect, state: &LogsState) {
    let content = if let Some(log) = state.selected_log() {
        format_log_detail(log)
    } else {
        Text::from("No log selected")
    };

    let paragraph = Paragraph::new(content)
        .block(Block::default().borders(Borders::ALL).title(" Log Detail "))
        .wrap(Wrap { trim: true });

    frame.render_widget(paragraph, area);
}

/// Format log detail for display
fn format_log_detail(log: &LogEntry) -> Text<'static> {
    let mut lines = vec![
        Line::from(vec![
            Span::styled("Timestamp: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(format_timestamp(log.timestamp)),
        ]),
        Line::from(vec![
            Span::styled("Severity: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(log.severity.clone(), get_severity_style(&log.severity)),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Message:",
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        Line::from(log.body.clone()),
        Line::from(""),
    ];

    // Add attributes
    if !log.attributes.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![Span::styled(
            "Attributes:",
            Style::default().add_modifier(Modifier::BOLD),
        )]));
        for (key, value) in &log.attributes {
            append_formatted_key_value_lines(&mut lines, key, value, 60);
        }
    }

    // Add resource attributes
    if let Some(resource) = &log.resource {
        if !resource.attributes.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![Span::styled(
                "Resource:",
                Style::default().add_modifier(Modifier::BOLD),
            )]));
            for (key, value) in &resource.attributes {
                append_formatted_key_value_lines(&mut lines, key, value, 60);
            }
        }
    }

    Text::from(lines)
}

fn append_formatted_key_value_lines(
    lines: &mut Vec<Line<'static>>,
    key: &str,
    value: &str,
    preview_width: usize,
) {
    let preview = rotel_core::telemetry::format_attribute_preview(value, preview_width);
    lines.push(Line::from(format!("  {key}: {preview}")));

    let formatted = rotel_core::telemetry::format_attribute_value(value);
    if formatted != value {
        for line in formatted.lines() {
            lines.push(Line::from(format!("      {line}")));
        }
    }
}

/// Render status bar
fn render_status_bar(frame: &mut Frame, area: Rect, state: &LogsState) {
    let mut status_parts = vec![];

    // View indicator
    status_parts.push(Span::styled(
        " LOGS ",
        Style::default()
            .fg(Color::Black)
            .bg(Color::Blue)
            .add_modifier(Modifier::BOLD),
    ));

    // Search indicator
    if !state.search_query.is_empty() {
        status_parts.push(Span::raw(" "));
        status_parts.push(Span::styled(
            format!(" 🔍 {} ", state.search_query),
            Style::default().fg(Color::Yellow),
        ));
    }

    // Filter indicator
    if !state.filters.is_empty() {
        status_parts.push(Span::raw(" "));
        let filter_text = state
            .filters
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join(", ");
        status_parts.push(Span::styled(
            format!(" 🔧 {} ", filter_text),
            Style::default().fg(Color::Cyan),
        ));
    }

    // Auto-scroll indicator
    if state.auto_scroll {
        status_parts.push(Span::raw(" "));
        status_parts.push(Span::styled(" ⬇ AUTO ", Style::default().fg(Color::Green)));
    }

    // Error indicator
    if let Some(error) = &state.error {
        status_parts.push(Span::raw(" "));
        status_parts.push(Span::styled(
            format!(" ⚠ {} ", error),
            Style::default().fg(Color::Red),
        ));
    }

    // Help text
    status_parts.push(Span::raw(" | "));
    status_parts.push(Span::styled(
        "↑↓: Navigate | Enter: Detail | /: Search | f: Filter | a: Auto-scroll | q: Quit",
        Style::default().fg(Color::DarkGray),
    ));

    let status_line = Line::from(status_parts);
    let paragraph = Paragraph::new(status_line);
    frame.render_widget(paragraph, area);
}

/// Get color style for severity level
fn get_severity_style(severity: &str) -> Style {
    match severity.to_uppercase().as_str() {
        "TRACE" => Style::default().fg(Color::DarkGray),
        "DEBUG" => Style::default().fg(Color::Blue),
        "INFO" => Style::default().fg(Color::Green),
        "WARN" | "WARNING" => Style::default().fg(Color::Yellow),
        "ERROR" => Style::default().fg(Color::Red),
        "FATAL" | "CRITICAL" => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        _ => Style::default(),
    }
}

/// Format timestamp for display (converts milliseconds to HH:MM:SS)
fn format_timestamp(timestamp_ms: i64) -> String {
    use chrono::DateTime;

    // Convert milliseconds to DateTime
    if let Some(dt) = DateTime::from_timestamp_millis(timestamp_ms) {
        dt.format("%H:%M:%S").to_string()
    } else {
        format!("{}", timestamp_ms)
    }
}

/// Truncate string to max length with ellipsis
fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_severity_style() {
        let trace_style = get_severity_style("TRACE");
        assert_eq!(trace_style.fg, Some(Color::DarkGray));

        let error_style = get_severity_style("ERROR");
        assert_eq!(error_style.fg, Some(Color::Red));
    }

    #[test]
    fn test_format_timestamp() {
        // 2026-04-17T12:34:56.789Z in milliseconds
        let timestamp_ms = 1713360896789;
        let formatted = format_timestamp(timestamp_ms);
        // Just check it's in HH:MM:SS format
        assert!(formatted.contains(':'));
        assert_eq!(formatted.len(), 8); // HH:MM:SS
    }

    #[test]
    fn test_truncate_string() {
        let short = "Hello";
        assert_eq!(truncate_string(short, 10), "Hello");

        let long = "This is a very long string that needs truncation";
        let truncated = truncate_string(long, 20);
        assert_eq!(truncated.len(), 20);
        assert!(truncated.ends_with("..."));
    }
}

// Made with Bob
