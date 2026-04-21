use crate::api::models::LogEntry;
use crate::state::LogsState;
use crate::ui::render_tab_bar;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState, Wrap},
    Frame,
};

/// Render the logs view
#[allow(clippy::too_many_arguments)]
pub fn render_logs_view(
    frame: &mut Frame,
    area: Rect,
    state: &LogsState,
    filter_input_active: bool,
    filter_input_buffer: &str,
    api_error: Option<&str>,
) {
    // Optionally prepend a 1-line error banner
    let content_area = if let Some(err) = api_error {
        let splits = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(0)])
            .split(area);
        render_api_error_banner(frame, splits[0], err);
        splits[1]
    } else {
        area
    };

    // Split the area: tab bar | main content | [filter bar] | status bar
    let constraints = if filter_input_active {
        vec![
            Constraint::Length(1), // Tab bar
            Constraint::Min(3),    // Main content
            Constraint::Length(1), // Filter input bar
            Constraint::Length(1), // Status bar
        ]
    } else {
        vec![
            Constraint::Length(1), // Tab bar
            Constraint::Min(3),    // Main content
            Constraint::Length(1), // Status bar
        ]
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(content_area);

    render_tab_bar(frame, chunks[0], "Logs");

    // Render main content (table or table + detail)
    if state.show_detail {
        render_logs_with_detail(frame, chunks[1], state);
    } else {
        render_logs_table(frame, chunks[1], state);
    }

    if filter_input_active {
        render_filter_input_bar(frame, chunks[2], filter_input_buffer);
        render_status_bar(frame, chunks[3], state, api_error);
    } else {
        render_status_bar(frame, chunks[2], state, api_error);
    }
}

/// Render logs table only
fn render_logs_table(frame: &mut Frame, area: Rect, state: &LogsState) {
    let filtered_logs = state.filtered_logs();

    // Empty state: no data and no error
    if filtered_logs.is_empty() && state.error.is_none() {
        let block = Block::default().borders(Borders::ALL).title(" Logs (0) ");
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let paragraph =
            Paragraph::new("No logs yet — send OTLP data to :4317 (gRPC) or :4318 (HTTP)")
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::DarkGray));
        // Vertically centre in the inner area
        if inner.height > 2 {
            let v_splits = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(inner.height / 2),
                    Constraint::Length(1),
                    Constraint::Min(0),
                ])
                .split(inner);
            frame.render_widget(paragraph, v_splits[1]);
        } else {
            frame.render_widget(paragraph, inner);
        }
        return;
    }

    // Create table rows — no manual selection; row_highlight_style handles it
    let rows: Vec<Row> = filtered_logs
        .iter()
        .map(|log| {
            Row::new(vec![
                Cell::from(format_timestamp(log.timestamp)),
                Cell::from(log.severity.clone()).style(get_severity_style(&log.severity)),
                Cell::from(truncate_string(&log.body, 80)),
            ])
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
            Constraint::Length(16), // Timestamp (YYYY-MM-DD HH:MM)
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
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    );

    // Use stateful render so the table scrolls to keep the selected row visible
    let mut table_state = TableState::default();
    table_state.select(Some(state.selected_index));
    frame.render_stateful_widget(table, area, &mut table_state);
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
    let content = if let Some(log) = state.selected_log_detail() {
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
            Span::raw(format_timestamp_full(log.timestamp)),
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
fn render_status_bar(frame: &mut Frame, area: Rect, state: &LogsState, api_error: Option<&str>) {
    let mut status_parts = vec![];

    // View indicator
    status_parts.push(Span::styled(
        " LOGS ",
        Style::default()
            .fg(Color::Black)
            .bg(Color::Blue)
            .add_modifier(Modifier::BOLD),
    ));

    // Connection status
    status_parts.push(Span::raw(" "));
    if api_error.is_some() {
        status_parts.push(Span::styled(
            "Disconnected",
            Style::default().fg(Color::Red),
        ));
    } else {
        status_parts.push(Span::styled("Connected", Style::default().fg(Color::Green)));
    }

    // Item count
    status_parts.push(Span::styled(
        format!(" | Logs: {} ", state.filtered_logs().len()),
        Style::default().fg(Color::DarkGray),
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
        "↑↓:Navigate  Enter:Detail  /:Search  f:Filter  a:AutoScroll  r:Refresh",
        Style::default().fg(Color::DarkGray),
    ));

    let status_line = Line::from(status_parts);
    let paragraph = Paragraph::new(status_line);
    frame.render_widget(paragraph, area);
}

/// Render a full-width red error banner (1 line)
pub(crate) fn render_api_error_banner(frame: &mut Frame, area: Rect, err: &str) {
    let paragraph = Paragraph::new(format!("⚠ Cannot connect to API: {}", err))
        .style(Style::default().fg(Color::Red).bg(Color::Black));
    frame.render_widget(paragraph, area);
}

/// Render a single-line filter input bar (vim-style prompt at the bottom of the pane)
fn render_filter_input_bar(frame: &mut Frame, area: Rect, buffer: &str) {
    let line = Line::from(vec![
        Span::styled(
            "Filter: ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(buffer.to_string()),
        Span::styled("█", Style::default().fg(Color::Yellow)),
        Span::styled(
            "  (Enter to apply, Esc to cancel, key=value or text)",
            Style::default().fg(Color::DarkGray),
        ),
    ]);
    frame.render_widget(Paragraph::new(line), area);
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

/// Format timestamp for display in the list view (16 chars: YYYY-MM-DD HH:MM).
/// Timestamps are stored as nanoseconds since Unix epoch; displayed in local time.
fn format_timestamp(timestamp_ns: i64) -> String {
    use chrono::{DateTime, Local, Utc};

    DateTime::<Utc>::from_timestamp_millis(timestamp_ns / 1_000_000)
        .map(|dt| {
            dt.with_timezone(&Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| "?".to_string())
}

/// Format timestamp for detail panels: full ISO 8601 with seconds and UTC offset.
pub(crate) fn format_timestamp_full(timestamp_ns: i64) -> String {
    use chrono::{DateTime, Local, Utc};

    DateTime::<Utc>::from_timestamp_millis(timestamp_ns / 1_000_000)
        .map(|dt| {
            dt.with_timezone(&Local)
                .format("%Y-%m-%d %H:%M:%S %z")
                .to_string()
        })
        .unwrap_or_else(|| "?".to_string())
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
        let timestamp_ns: i64 = 1713360896789 * 1_000_000;
        let formatted = format_timestamp(timestamp_ns);
        // YYYY-MM-DD HH:MM — 16 chars, year-first ISO ordering
        assert_eq!(formatted.len(), 16);
        assert!(formatted.starts_with("20")); // year starts with 20xx
        assert!(formatted.contains('-'));
        assert!(formatted.contains(':'));
    }

    #[test]
    fn test_format_timestamp_full() {
        let timestamp_ns: i64 = 1713360896789 * 1_000_000;
        let formatted = format_timestamp_full(timestamp_ns);
        // YYYY-MM-DD HH:MM:SS +HHMM — at least 24 chars
        assert!(formatted.len() >= 24);
        assert!(formatted.starts_with("20"));
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
