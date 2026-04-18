use crate::api::models::{Span, Trace, TraceSummary};
use crate::state::TracesState;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span as TextSpan, Text},
    widgets::{Block, Borders, Paragraph, Row, Table, Wrap},
    Frame,
};
use rotel_core::telemetry::GenAiSpanInfo;

/// A span node in the tree with calculated timing information
#[derive(Debug, Clone)]
struct SpanNode {
    span: Span,
    depth: usize,
    /// Start time relative to trace start (in nanoseconds)
    relative_start: i64,
    /// Percentage of trace duration this span represents
    duration_percent: f64,
}

/// Build a tree of spans with timing calculations
fn build_span_tree(trace: &Trace) -> Vec<SpanNode> {
    let mut nodes = Vec::new();
    let trace_start = trace.start_time;
    let trace_duration = trace.duration as f64;

    // Build a map of span_id -> children for quick lookup
    let mut children_map: std::collections::HashMap<String, Vec<&Span>> =
        std::collections::HashMap::new();
    let mut root_spans = Vec::new();

    for span in &trace.spans {
        if let Some(parent_id) = &span.parent_span_id {
            children_map
                .entry(parent_id.clone())
                .or_default()
                .push(span);
        } else {
            root_spans.push(span);
        }
    }

    // Sort root spans by start time
    root_spans.sort_by_key(|s| s.start_time);

    // Recursively build tree
    #[allow(clippy::too_many_arguments)]
    fn add_span_and_children(
        span: &Span,
        depth: usize,
        trace_start: i64,
        trace_duration: f64,
        children_map: &std::collections::HashMap<String, Vec<&Span>>,
        nodes: &mut Vec<SpanNode>,
    ) {
        let relative_start = span.start_time - trace_start;
        let duration_percent = if trace_duration > 0.0 {
            (span.duration as f64 / trace_duration) * 100.0
        } else {
            0.0
        };

        nodes.push(SpanNode {
            span: span.clone(),
            depth,
            relative_start,
            duration_percent,
        });

        // Add children sorted by start time
        if let Some(children) = children_map.get(&span.span_id) {
            let mut sorted_children = children.clone();
            sorted_children.sort_by_key(|s| s.start_time);

            for child in sorted_children {
                add_span_and_children(
                    child,
                    depth + 1,
                    trace_start,
                    trace_duration,
                    children_map,
                    nodes,
                );
            }
        }
    }

    for root in root_spans {
        add_span_and_children(
            root,
            0,
            trace_start,
            trace_duration,
            &children_map,
            &mut nodes,
        );
    }

    nodes
}

/// Render a timing bar for a span
/// Returns a string with Unicode block characters representing the span's timing
fn render_timing_bar(
    relative_start: i64,
    duration: i64,
    trace_duration: i64,
    bar_width: usize,
) -> String {
    if trace_duration == 0 || bar_width == 0 {
        return String::new();
    }

    let start_pos = ((relative_start as f64 / trace_duration as f64) * bar_width as f64) as usize;
    let span_width =
        ((duration as f64 / trace_duration as f64) * bar_width as f64).max(1.0) as usize;
    let end_pos = (start_pos + span_width).min(bar_width);

    let mut bar = vec![' '; bar_width];

    // Fill the span duration with blocks
    for item in bar.iter_mut().take(end_pos).skip(start_pos) {
        *item = '█';
    }

    // Add background dots for context
    for item in bar.iter_mut().take(bar_width) {
        if *item == ' ' {
            *item = '░';
        }
    }

    bar.into_iter().collect()
}

/// Get color for timing bar based on span status and duration percentage
fn get_timing_bar_color(status: &str, duration_percent: f64) -> Color {
    match status.to_uppercase().as_str() {
        "ERROR" | "FAILED" => Color::Red,
        _ if duration_percent > 50.0 => Color::Yellow,
        _ => Color::Green,
    }
}

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
    let content = if state.show_span_detail {
        // Show detailed span view
        if let Some(trace_details) = state.selected_trace_details() {
            let span_nodes = build_span_tree(trace_details);
            if let Some(node) = span_nodes.get(state.selected_span_index) {
                format_span_detail(&node.span, trace_details)
            } else {
                Text::from("No span selected")
            }
        } else {
            Text::from("Loading trace details...")
        }
    } else if let Some(trace_details) = state.selected_trace_details() {
        format_trace_detail(trace_details, state)
    } else if let Some(summary) = state.selected_trace() {
        format_trace_summary(summary)
    } else {
        Text::from("No trace selected")
    };

    let title = if state.show_span_detail {
        " Span Detail "
    } else {
        " Trace Detail "
    };

    let paragraph = Paragraph::new(content)
        .block(Block::default().borders(Borders::ALL).title(title))
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

/// Format detailed span information
fn format_span_detail(span: &Span, trace: &Trace) -> Text<'static> {
    let span_duration_ms = span.duration / 1_000_000;
    let relative_start_ms = (span.start_time - trace.start_time) / 1_000_000;

    let mut lines = vec![
        Line::from(vec![
            TextSpan::styled("Span: ", Style::default().add_modifier(Modifier::BOLD)),
            TextSpan::styled(
                span.name.clone(),
                get_span_status_color(
                    span.status
                        .as_ref()
                        .map(|s| s.code.as_str())
                        .unwrap_or("Ok"),
                ),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            TextSpan::styled("Span ID: ", Style::default().add_modifier(Modifier::BOLD)),
            TextSpan::raw(span.span_id.clone()),
        ]),
        Line::from(vec![
            TextSpan::styled("Trace ID: ", Style::default().add_modifier(Modifier::BOLD)),
            TextSpan::raw(span.trace_id.clone()),
        ]),
        Line::from(""),
        Line::from(vec![
            TextSpan::styled("Kind: ", Style::default().add_modifier(Modifier::BOLD)),
            TextSpan::raw(span.kind.clone()),
        ]),
        Line::from(vec![
            TextSpan::styled("Status: ", Style::default().add_modifier(Modifier::BOLD)),
            TextSpan::styled(
                span.status
                    .as_ref()
                    .map(|s| s.code.clone())
                    .unwrap_or_else(|| "Ok".to_string()),
                get_span_status_color(
                    span.status
                        .as_ref()
                        .map(|s| s.code.as_str())
                        .unwrap_or("Ok"),
                ),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            TextSpan::styled("Start: ", Style::default().add_modifier(Modifier::BOLD)),
            TextSpan::raw(format!("+{}ms from trace start", relative_start_ms)),
        ]),
        Line::from(vec![
            TextSpan::styled("Duration: ", Style::default().add_modifier(Modifier::BOLD)),
            TextSpan::raw(format!("{}ms", span_duration_ms)),
        ]),
        Line::from(""),
    ];

    // Parent/children info
    if let Some(parent_id) = &span.parent_span_id {
        if let Some(parent) = trace.spans.iter().find(|s| &s.span_id == parent_id) {
            lines.push(Line::from(vec![
                TextSpan::styled("Parent: ", Style::default().add_modifier(Modifier::BOLD)),
                TextSpan::raw(parent.name.clone()),
            ]));
        }
    } else {
        lines.push(Line::from(vec![
            TextSpan::styled("Parent: ", Style::default().add_modifier(Modifier::BOLD)),
            TextSpan::raw("(root span)"),
        ]));
    }

    let children: Vec<_> = trace
        .spans
        .iter()
        .filter(|s| s.parent_span_id.as_ref() == Some(&span.span_id))
        .collect();

    if !children.is_empty() {
        lines.push(Line::from(vec![
            TextSpan::styled("Children: ", Style::default().add_modifier(Modifier::BOLD)),
            TextSpan::raw(format!("{} spans", children.len())),
        ]));
        for child in children.iter().take(5) {
            lines.push(Line::from(format!("  • {}", child.name)));
        }
        if children.len() > 5 {
            lines.push(Line::from(format!("  ... and {} more", children.len() - 5)));
        }
    }

    lines.push(Line::from(""));

    // GenAI/LLM information (if present)
    let genai_info = GenAiSpanInfo::from_attributes(&span.attributes);
    if genai_info.is_genai {
        lines.push(Line::from(vec![TextSpan::styled(
            "GenAI/LLM Information:",
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::Magenta),
        )]));

        if let Some(system) = genai_info.system_display_name() {
            lines.push(Line::from(vec![
                TextSpan::styled("  System: ", Style::default().add_modifier(Modifier::BOLD)),
                TextSpan::styled(format!("[{}]", system), Style::default().fg(Color::Cyan)),
            ]));
        }

        if let Some(model) = &genai_info.model {
            lines.push(Line::from(vec![
                TextSpan::styled("  Model: ", Style::default().add_modifier(Modifier::BOLD)),
                TextSpan::raw(model.clone()),
            ]));
        }

        if let Some(operation) = &genai_info.operation {
            lines.push(Line::from(vec![
                TextSpan::styled(
                    "  Operation: ",
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                TextSpan::raw(operation.clone()),
            ]));
        }

        if let Some(token_usage) = genai_info.format_token_usage() {
            lines.push(Line::from(vec![
                TextSpan::styled("  Tokens: ", Style::default().add_modifier(Modifier::BOLD)),
                TextSpan::styled(token_usage, Style::default().fg(Color::Yellow)),
            ]));
        }

        if let Some(temp) = genai_info.temperature {
            lines.push(Line::from(vec![
                TextSpan::styled(
                    "  Temperature: ",
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                TextSpan::raw(format!("{:.2}", temp)),
            ]));
        }

        if let Some(max_tokens) = genai_info.max_tokens {
            lines.push(Line::from(vec![
                TextSpan::styled(
                    "  Max Tokens: ",
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                TextSpan::raw(max_tokens.to_string()),
            ]));
        }

        if !genai_info.finish_reasons.is_empty() {
            lines.push(Line::from(vec![
                TextSpan::styled(
                    "  Finish Reasons: ",
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                TextSpan::raw(genai_info.finish_reasons.join(", ")),
            ]));
        }

        lines.push(Line::from(""));
    }

    // Attributes
    if !span.attributes.is_empty() {
        lines.push(Line::from(vec![TextSpan::styled(
            "Attributes:",
            Style::default().add_modifier(Modifier::BOLD),
        )]));
        for (key, value) in &span.attributes {
            let indent = "  ";
            let preview = rotel_core::telemetry::format_attribute_preview(value, 60);
            lines.push(Line::from(format!("{}{}: {}", indent, key, preview)));

            let formatted = rotel_core::telemetry::format_attribute_value(value);
            if formatted != *value {
                for line in formatted.lines() {
                    lines.push(Line::from(format!("{}    {}", indent, line)));
                }
            }
        }
        lines.push(Line::from(""));
    }

    // Events
    if !span.events.is_empty() {
        lines.push(Line::from(vec![TextSpan::styled(
            "Events:",
            Style::default().add_modifier(Modifier::BOLD),
        )]));
        for event in &span.events {
            let event_time_ms = (event.timestamp - span.start_time) / 1_000_000;
            lines.push(Line::from(format!(
                "  • {} (+{}ms)",
                event.name, event_time_ms
            )));
            for (key, value) in &event.attributes {
                let indent = "    ";
                let preview = rotel_core::telemetry::format_attribute_preview(value, 60);
                lines.push(Line::from(format!("{}{}: {}", indent, key, preview)));

                let formatted = rotel_core::telemetry::format_attribute_value(value);
                if formatted != *value {
                    for line in formatted.lines() {
                        lines.push(Line::from(format!("{}    {}", indent, line)));
                    }
                }
            }
        }
        lines.push(Line::from(""));
    }

    // Links section removed - not in current API model

    lines.push(Line::from(vec![TextSpan::styled(
        "Press Esc to return to trace view",
        Style::default().fg(Color::DarkGray),
    )]));

    Text::from(lines)
}

/// Format full trace detail for display
fn format_trace_detail(trace: &Trace, state: &TracesState) -> Text<'static> {
    let duration_ms = trace.duration / 1_000_000;

    let mut lines = vec![
        Line::from(vec![
            TextSpan::styled("Trace ID: ", Style::default().add_modifier(Modifier::BOLD)),
            TextSpan::raw(trace.trace_id.clone()),
        ]),
        Line::from(vec![
            TextSpan::styled("Operation: ", Style::default().add_modifier(Modifier::BOLD)),
            TextSpan::raw(
                trace
                    .spans
                    .iter()
                    .find(|s| s.parent_span_id.is_none())
                    .map(|s| s.name.clone())
                    .unwrap_or_else(|| "Unknown".to_string()),
            ),
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

    // Build span tree with timing information
    let span_nodes = build_span_tree(trace);

    // Calculate bar width (assume 80 char width, reserve space for name and duration)
    let bar_width = 40;

    // Render each span with timing bar
    for (idx, node) in span_nodes.iter().enumerate() {
        let is_selected = idx == state.selected_span_index;
        let span_duration_ms = node.span.duration / 1_000_000;
        let indent = " ".repeat(node.depth * 2);
        let tree_char = if node.depth > 0 { "├─ " } else { "▶ " };

        // Truncate name to fit
        let max_name_len = 25_usize.saturating_sub(node.depth * 2);
        let span_name = truncate_string(&node.span.name, max_name_len);

        // Render timing bar
        let timing_bar = render_timing_bar(
            node.relative_start,
            node.span.duration,
            trace.duration,
            bar_width,
        );

        let status_str = node
            .span
            .status
            .as_ref()
            .map(|s| s.code.as_str())
            .unwrap_or("Ok");
        let bar_color = get_timing_bar_color(status_str, node.duration_percent);
        let mut status_color = get_span_status_color(status_str);

        // Highlight selected span
        let selection_marker = if is_selected { "▶ " } else { "  " };
        if is_selected {
            status_color = Color::Cyan;
        }

        lines.push(Line::from(vec![
            TextSpan::styled(
                selection_marker,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            TextSpan::raw(indent),
            TextSpan::styled(tree_char, Style::default().fg(Color::DarkGray)),
            TextSpan::styled(
                span_name,
                Style::default()
                    .fg(status_color)
                    .add_modifier(if is_selected {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    }),
            ),
            TextSpan::raw(" "),
            TextSpan::styled(timing_bar, Style::default().fg(bar_color)),
            TextSpan::raw(format!(" {}ms", span_duration_ms)),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![TextSpan::styled(
        "Press Enter on a span to view details",
        Style::default().fg(Color::DarkGray),
    )]));

    Text::from(lines)
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
    let help_text = if state.show_span_detail {
        "Esc: Back to trace"
    } else if state.show_detail {
        "↑↓/jk: Navigate spans | Enter: Span detail | Esc: Back | q: Quit"
    } else {
        "↑↓: Navigate | Enter: Detail | /: Search | f: Filter | q: Quit"
    };
    status_parts.push(TextSpan::styled(
        help_text,
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
