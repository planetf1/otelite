use crate::api::models::{Span, Trace, TraceSummary};
use crate::state::TracesState;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span as TextSpan, Text},
    widgets::{Block, Borders, Paragraph, Row, Table, Wrap},
    Frame,
};

/// Render the traces view
pub fn render_traces_view(frame: &mut Frame, area: Rect, state: &TracesState) {
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
        render_traces_with_detail(frame, chunks[0], state);
    } else {
        render_traces_table(frame, chunks[0], state);
    }

    // Render status bar
    render_status_bar(frame, chunks[1], state);
}

/// Render traces table only
fn render_traces_table(frame: &mut Frame, area: Rect, state: &TracesState) {
    let filtered_traces = state.filtered_traces();

    // Create table rows
    let rows: Vec<Row> = filtered_traces
        .iter()
        .enumerate()
        .map(|(idx, trace)| {
            let style = if idx == state.selected_index {
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            let error_indicator = if trace.has_errors { "⚠" } else { " " };
            let duration_ms = trace.duration / 1_000_000; // Convert nanoseconds to milliseconds

            Row::new(vec![
                format_timestamp(trace.start_time),
                error_indicator.to_string(),
                truncate_string(&trace.root_span_name, 40),
                format!("{}ms", duration_ms),
                trace.span_count.to_string(),
                trace.service_names.join(", "),
            ])
            .style(style)
            .height(1)
        })
        .collect();

    // Create table header
    let header = Row::new(vec![
        "Time",
        "E",
        "Operation",
        "Duration",
        "Spans",
        "Services",
    ])
    .style(Style::default().add_modifier(Modifier::BOLD))
    .bottom_margin(1);

    // Create table widget
    let table = Table::new(
        rows,
        [
            Constraint::Length(10), // Time
            Constraint::Length(2),  // Error indicator
            Constraint::Min(30),    // Operation
            Constraint::Length(10), // Duration
            Constraint::Length(6),  // Spans
            Constraint::Min(20),    // Services
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" Traces ({}) ", filtered_traces.len())),
    )
    .highlight_style(
        Style::default()
            .bg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
    );

    frame.render_widget(table, area);
}

/// Render traces table with detail panel
fn render_traces_with_detail(frame: &mut Frame, area: Rect, state: &TracesState) {
    // Split area horizontally
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50), // Table
            Constraint::Percentage(50), // Detail
        ])
        .split(area);

    // Render table
    render_traces_table(frame, chunks[0], state);

    // Render detail panel
    render_detail_panel(frame, chunks[1], state);
}

/// Render trace detail panel
fn render_detail_panel(frame: &mut Frame, area: Rect, state: &TracesState) {
    let content = if let Some(trace_details) = state.selected_trace_details() {
        format_trace_detail(trace_details)
    } else if let Some(summary) = state.selected_trace() {
        format_trace_summary(summary)
    } else {
        Text::from("No trace selected")
    };

    let paragraph = Paragraph::new(content)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Trace Detail "),
        )
        .wrap(Wrap { trim: true });

    frame.render_widget(paragraph, area);
}

/// Format trace summary for display (when full details not loaded)
fn format_trace_summary(summary: &TraceSummary) -> Text<'static> {
    let duration_ms = summary.duration / 1_000_000;

    let lines = vec![
        Line::from(vec![
            TextSpan::styled("Trace ID: ", Style::default().add_modifier(Modifier::BOLD)),
            TextSpan::raw(summary.trace_id.clone()),
        ]),
        Line::from(vec![
            TextSpan::styled("Operation: ", Style::default().add_modifier(Modifier::BOLD)),
            TextSpan::raw(summary.root_span_name.clone()),
        ]),
        Line::from(""),
        Line::from(vec![
            TextSpan::styled(
                "Start Time: ",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            TextSpan::raw(format_timestamp(summary.start_time)),
        ]),
        Line::from(vec![
            TextSpan::styled("Duration: ", Style::default().add_modifier(Modifier::BOLD)),
            TextSpan::raw(format!("{}ms", duration_ms)),
        ]),
        Line::from(vec![
            TextSpan::styled(
                "Span Count: ",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            TextSpan::raw(summary.span_count.to_string()),
        ]),
        Line::from(vec![
            TextSpan::styled(
                "Has Errors: ",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            TextSpan::styled(
                if summary.has_errors { "Yes" } else { "No" },
                if summary.has_errors {
                    Style::default().fg(Color::Red)
                } else {
                    Style::default().fg(Color::Green)
                },
            ),
        ]),
        Line::from(""),
        Line::from(vec![TextSpan::styled(
            "Services:",
            Style::default().add_modifier(Modifier::BOLD),
        )]),
    ];

    let mut all_lines = lines;
    for service in &summary.service_names {
        all_lines.push(Line::from(format!("  • {}", service)));
    }

    all_lines.push(Line::from(""));
    all_lines.push(Line::from(vec![TextSpan::styled(
        "Press Enter to load full trace details",
        Style::default().fg(Color::DarkGray),
    )]));

    Text::from(all_lines)
}

/// Format full trace detail for display
fn format_trace_detail(trace: &Trace) -> Text<'static> {
    let duration_ms = trace.duration / 1_000_000;

    let mut lines = vec![
        Line::from(vec![
            TextSpan::styled("Trace ID: ", Style::default().add_modifier(Modifier::BOLD)),
            TextSpan::raw(trace.trace_id.clone()),
        ]),
        Line::from(vec![
            TextSpan::styled("Operation: ", Style::default().add_modifier(Modifier::BOLD)),
            TextSpan::raw(trace.root_span_name.clone()),
        ]),
        Line::from(""),
        Line::from(vec![
            TextSpan::styled("Duration: ", Style::default().add_modifier(Modifier::BOLD)),
            TextSpan::raw(format!("{}ms", duration_ms)),
        ]),
        Line::from(vec![
            TextSpan::styled("Spans: ", Style::default().add_modifier(Modifier::BOLD)),
            TextSpan::raw(trace.span_count.to_string()),
        ]),
        Line::from(""),
        Line::from(vec![TextSpan::styled(
            "Span Waterfall:",
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
    ];

    // Add waterfall view of spans
    for span in &trace.spans {
        let span_duration_ms = span.duration / 1_000_000;
        let indent = calculate_span_indent(span, &trace.spans);
        let status_color = get_span_status_color(&span.status);

        lines.push(Line::from(vec![
            TextSpan::raw(" ".repeat(indent * 2)),
            TextSpan::styled("├─ ", Style::default().fg(Color::DarkGray)),
            TextSpan::styled(
                truncate_string(&span.name, 30),
                Style::default().fg(status_color),
            ),
            TextSpan::raw(format!(" ({}ms)", span_duration_ms)),
        ]));

        for (key, value) in &span.attributes {
            append_formatted_key_value_lines(&mut lines, key, value, indent + 1, 60);
        }
    }

    Text::from(lines)
}

fn append_formatted_key_value_lines(
    lines: &mut Vec<Line<'static>>,
    key: &str,
    value: &str,
    indent_level: usize,
    preview_width: usize,
) {
    let indent = " ".repeat(indent_level * 2);
    let preview = rotel_core::telemetry::format_attribute_preview(value, preview_width);
    lines.push(Line::from(format!("{indent}  {key}: {preview}")));

    let formatted = rotel_core::telemetry::format_attribute_value(value);
    if formatted != value {
        for line in formatted.lines() {
            lines.push(Line::from(format!("{indent}      {line}")));
        }
    }
}

/// Calculate indentation level for span based on parent relationships
fn calculate_span_indent(span: &Span, all_spans: &[Span]) -> usize {
    let mut indent = 0;
    let mut current_parent = span.parent_span_id.clone();

    while let Some(parent_id) = current_parent {
        indent += 1;
        current_parent = all_spans
            .iter()
            .find(|s| s.span_id == parent_id)
            .and_then(|s| s.parent_span_id.clone());

        // Prevent infinite loops
        if indent > 10 {
            break;
        }
    }

    indent
}

/// Get color for span status
fn get_span_status_color(status: &str) -> Color {
    match status.to_uppercase().as_str() {
        "OK" | "SUCCESS" => Color::Green,
        "ERROR" | "FAILED" => Color::Red,
        "UNSET" | "UNKNOWN" => Color::DarkGray,
        _ => Color::White,
    }
}

/// Render status bar
fn render_status_bar(frame: &mut Frame, area: Rect, state: &TracesState) {
    let mut status_parts = vec![];

    // View indicator
    status_parts.push(TextSpan::styled(
        " TRACES ",
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    ));

    // Search indicator
    if !state.search_query.is_empty() {
        status_parts.push(TextSpan::raw(" "));
        status_parts.push(TextSpan::styled(
            format!(" 🔍 {} ", state.search_query),
            Style::default().fg(Color::Yellow),
        ));
    }

    // Filter indicator
    if !state.filters.is_empty() {
        status_parts.push(TextSpan::raw(" "));
        let filter_text = state
            .filters
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join(", ");
        status_parts.push(TextSpan::styled(
            format!(" 🔧 {} ", filter_text),
            Style::default().fg(Color::Cyan),
        ));
    }

    // Error indicator
    if let Some(error) = &state.error {
        status_parts.push(TextSpan::raw(" "));
        status_parts.push(TextSpan::styled(
            format!(" ⚠ {} ", error),
            Style::default().fg(Color::Red),
        ));
    }

    // Help text
    status_parts.push(TextSpan::raw(" | "));
    status_parts.push(TextSpan::styled(
        "↑↓: Navigate | Enter: Detail | /: Search | f: Filter | q: Quit",
        Style::default().fg(Color::DarkGray),
    ));

    let status_line = Line::from(status_parts);
    let paragraph = Paragraph::new(status_line);
    frame.render_widget(paragraph, area);
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
    fn test_get_span_status_color() {
        assert_eq!(get_span_status_color("OK"), Color::Green);
        assert_eq!(get_span_status_color("ERROR"), Color::Red);
        assert_eq!(get_span_status_color("UNSET"), Color::DarkGray);
    }

    #[test]
    fn test_format_timestamp() {
        let timestamp_ms = 1713360896789;
        let formatted = format_timestamp(timestamp_ms);
        assert!(formatted.contains(':'));
        assert_eq!(formatted.len(), 8);
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
