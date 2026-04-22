use crate::api::models::{Span, Trace, TraceSummary};
use crate::state::TracesState;
use crate::ui::{logs::render_api_error_banner, render_tab_bar};
use otelite_core::telemetry::GenAiSpanInfo;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span as TextSpan, Text},
    widgets::{Block, Borders, Paragraph, Row, Table, TableState, Wrap},
    Frame,
};

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
#[allow(clippy::too_many_arguments)]
pub fn render_traces_view(
    frame: &mut Frame,
    area: Rect,
    state: &TracesState,
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

    render_tab_bar(frame, chunks[0], "Traces");

    // Render main content (table or table + detail)
    if state.show_detail {
        render_traces_with_detail(frame, chunks[1], state);
    } else {
        render_traces_table(frame, chunks[1], state);
    }

    if filter_input_active {
        render_filter_input_bar(frame, chunks[2], filter_input_buffer);
        render_status_bar(frame, chunks[3], state, api_error);
    } else {
        render_status_bar(frame, chunks[2], state, api_error);
    }
}

/// Render traces table only
fn render_traces_table(frame: &mut Frame, area: Rect, state: &TracesState) {
    let filtered_traces = state.filtered_traces();

    // Empty state: no data and no error
    if filtered_traces.is_empty() && state.error.is_none() {
        let block = Block::default().borders(Borders::ALL).title(" Traces (0) ");
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let paragraph =
            Paragraph::new("No traces yet — send OTLP data to :4317 (gRPC) or :4318 (HTTP)")
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::DarkGray));
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
    let rows: Vec<Row> = filtered_traces
        .iter()
        .map(|trace| {
            let error_indicator = if trace.has_errors { "⚠" } else { " " };
            let row_style = if trace.has_errors {
                Style::default().fg(Color::LightRed)
            } else {
                Style::default()
            };

            Row::new(vec![
                format_timestamp(trace.start_time),
                error_indicator.to_string(),
                truncate_string(&trace.root_span_name, 40),
                format_duration(trace.duration),
                trace.span_count.to_string(),
                trace.service_names.join(", "),
            ])
            .height(1)
            .style(row_style)
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
            Constraint::Length(8),  // Time (HH:MM:SS)
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
            .title(format!(" Traces ({}) ", filtered_traces.len()))
            .border_style(Style::default().fg(Color::Cyan)),
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
    } else if state.pending_detail_load.is_some() {
        Text::from(vec![
            Line::from(""),
            Line::from(TextSpan::styled(
                "  Loading trace details...",
                Style::default().fg(Color::Yellow),
            )),
        ])
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

    // Auto-scroll to keep the selected span visible in the waterfall.
    // 8 header lines (trace_id, op, blank, duration, spans, blank, label, blank) precede spans.
    let scroll_y = if !state.show_span_detail
        && state.selected_trace_details().is_some_and(|t| !t.spans.is_empty())
    {
        const HEADER_LINES: u16 = 8;
        let target_line = HEADER_LINES + state.selected_span_index as u16;
        let visible_height = area.height.saturating_sub(2);
        target_line.saturating_sub(visible_height / 2)
    } else {
        0
    };

    // Use trim: false so leading indent spaces (span depth) are preserved in the waterfall.
    let paragraph = Paragraph::new(content)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .wrap(Wrap { trim: false })
        .scroll((scroll_y, 0));
    frame.render_widget(paragraph, area);
}

/// Format trace summary for display (when full details not loaded)
fn format_trace_summary(summary: &TraceSummary) -> Text<'static> {
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
            TextSpan::raw(format_timestamp_full(summary.start_time)),
        ]),
        Line::from(vec![
            TextSpan::styled("Duration: ", Style::default().add_modifier(Modifier::BOLD)),
            TextSpan::raw(format_duration(summary.duration)),
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
        Style::default().fg(Color::Yellow),
    )]));

    Text::from(all_lines)
}

/// Format detailed span information
fn format_span_detail(span: &Span, trace: &Trace) -> Text<'static> {
    let relative_start_ms = (span.start_time - trace.start_time) / 1_000_000;

    let mut lines = vec![
        Line::from(vec![
            TextSpan::styled("Span: ", Style::default().add_modifier(Modifier::BOLD)),
            TextSpan::styled(span.name.clone(), get_span_status_color(&span.status.code)),
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
                span.status.code.clone(),
                get_span_status_color(&span.status.code),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            TextSpan::styled("Start: ", Style::default().add_modifier(Modifier::BOLD)),
            TextSpan::raw(format!("+{}ms from trace start", relative_start_ms)),
        ]),
        Line::from(vec![
            TextSpan::styled("Duration: ", Style::default().add_modifier(Modifier::BOLD)),
            TextSpan::raw(format_duration(span.duration)),
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

        if let Some(response_model) = &genai_info.response_model {
            // Only show if it differs from the request model
            if genai_info.model.as_deref() != Some(response_model.as_str()) {
                lines.push(Line::from(vec![
                    TextSpan::styled(
                        "  Response model: ",
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    TextSpan::raw(response_model.clone()),
                ]));
            }
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

        if let Some(n) = genai_info.cache_creation_tokens {
            if n > 0 {
                lines.push(Line::from(vec![
                    TextSpan::styled(
                        "  Cache creation: ",
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    TextSpan::styled(n.to_string(), Style::default().fg(Color::Yellow)),
                ]));
            }
        }

        if let Some(n) = genai_info.cache_read_tokens {
            if n > 0 {
                lines.push(Line::from(vec![
                    TextSpan::styled(
                        "  Cache read: ",
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    TextSpan::styled(n.to_string(), Style::default().fg(Color::Yellow)),
                ]));
            }
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

        if let Some(response_id) = &genai_info.response_id {
            lines.push(Line::from(vec![
                TextSpan::styled(
                    "  Response ID: ",
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                TextSpan::styled(response_id.clone(), Style::default().fg(Color::DarkGray)),
            ]));
        }

        if let Some(tool_name) = &genai_info.tool_name {
            lines.push(Line::from(vec![
                TextSpan::styled("  Tool: ", Style::default().add_modifier(Modifier::BOLD)),
                TextSpan::styled(
                    tool_name.clone(),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        }

        if let Some(tool_call_id) = &genai_info.tool_call_id {
            lines.push(Line::from(vec![
                TextSpan::styled(
                    "  Tool call ID: ",
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                TextSpan::styled(tool_call_id.clone(), Style::default().fg(Color::DarkGray)),
            ]));
        }

        if let Some(top_p) = genai_info.top_p {
            lines.push(Line::from(vec![
                TextSpan::styled("  Top-p: ", Style::default().add_modifier(Modifier::BOLD)),
                TextSpan::raw(format!("{:.2}", top_p)),
            ]));
        }

        if let Some(seed) = genai_info.seed {
            lines.push(Line::from(vec![
                TextSpan::styled("  Seed: ", Style::default().add_modifier(Modifier::BOLD)),
                TextSpan::raw(seed.to_string()),
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
            let preview = otelite_core::telemetry::format_attribute_preview(value, 60);
            lines.push(Line::from(format!("{}{}: {}", indent, key, preview)));

            let formatted = otelite_core::telemetry::format_attribute_value(value);
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
                let preview = otelite_core::telemetry::format_attribute_preview(value, 60);
                lines.push(Line::from(format!("{}{}: {}", indent, key, preview)));

                let formatted = otelite_core::telemetry::format_attribute_value(value);
                if formatted != *value {
                    for line in formatted.lines() {
                        lines.push(Line::from(format!("{}    {}", indent, line)));
                    }
                }
            }
        }
        lines.push(Line::from(""));
    }

    lines.push(Line::from(vec![TextSpan::styled(
        "Press Esc to return to trace view",
        Style::default().fg(Color::Yellow),
    )]));

    Text::from(lines)
}

/// Format full trace detail for display
fn format_trace_detail(trace: &Trace, state: &TracesState) -> Text<'static> {
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
            TextSpan::raw(format_duration(trace.duration)),
        ]),
        Line::from(vec![
            TextSpan::styled("Spans: ", Style::default().add_modifier(Modifier::BOLD)),
            TextSpan::raw(trace.span_count.to_string()),
        ]),
        Line::from(""),
        Line::from(vec![TextSpan::styled(
            "Span Waterfall:",
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::Cyan),
        )]),
        Line::from(""),
    ];

    // Build span tree with timing information
    let span_nodes = build_span_tree(trace);

    // Bar width sized to keep lines within ~78 cols (50% split panel minus borders).
    // Fixed cost per line: 2 (selector) + 3 (tree) + 25 (name) + 1 + 1 + 6 (duration) = 38.
    // 78 - 38 = 40, but GenAI badges add ~20 chars, so use 20 to avoid wrapping.
    let bar_width = 20;

    // Render each span with timing bar
    for (idx, node) in span_nodes.iter().enumerate() {
        let is_selected = idx == state.selected_span_index;
        let span_duration = format_duration(node.span.duration);
        let indent = " ".repeat(node.depth * 2);
        // Both markers are exactly 2 display columns — critical for bar alignment
        let tree_char = if node.depth > 0 { "└ " } else { "  " };

        // Truncate name to fit, then pad to fixed width so the bar column is constant
        let max_name_len = 25_usize.saturating_sub(node.depth * 2);
        let truncated = truncate_string(&node.span.name, max_name_len);
        let span_name = format!("{:<width$}", truncated, width = max_name_len);

        // Render timing bar
        let timing_bar = render_timing_bar(
            node.relative_start,
            node.span.duration,
            trace.duration,
            bar_width,
        );

        let status_str = &node.span.status.code;
        let bar_color = get_timing_bar_color(status_str, node.duration_percent);
        let mut status_color = get_span_status_color(status_str);

        // Depth-based color for tree connector so the hierarchy is visually clear
        let tree_color = match node.depth {
            0 => Color::White,
            1 => Color::Cyan,
            2 => Color::Blue,
            _ => Color::Gray,
        };

        // Highlight selected span
        let selection_marker = if is_selected { "▶ " } else { "  " };
        if is_selected {
            status_color = Color::Cyan;
        }

        // Build optional GenAI inline badge — placed AFTER duration so it never shifts the bar
        let genai_info = GenAiSpanInfo::from_attributes(&node.span.attributes);
        let genai_badge: Option<String> = if genai_info.is_genai {
            if genai_info.is_tool_call() {
                let tool = genai_info.tool_name.as_deref().unwrap_or("tool");
                Some(format!(" [\u{1f527} {}]", tool))
            } else {
                let model = genai_info
                    .response_model
                    .as_deref()
                    .or(genai_info.model.as_deref());
                match (model, genai_info.input_tokens, genai_info.output_tokens) {
                    (Some(m), Some(input), Some(output)) => {
                        Some(format!(" [{} \u{00b7} {}\u{2192}{}]", m, input, output))
                    },
                    (Some(m), _, _) => Some(format!(" [{}]", m)),
                    (None, Some(input), Some(output)) => {
                        Some(format!(" [{}\u{2192}{}]", input, output))
                    },
                    _ => None,
                }
            }
        } else {
            None
        };

        let mut row_spans = vec![
            TextSpan::styled(
                selection_marker,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            TextSpan::raw(indent),
            TextSpan::styled(tree_char, Style::default().fg(tree_color)),
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
            TextSpan::raw(format!(" {:>6}", span_duration)),
        ];

        // GenAI badge after duration — doesn't disturb bar column alignment
        if let Some(badge) = genai_badge {
            row_spans.push(TextSpan::styled(
                badge,
                Style::default().fg(Color::Magenta),
            ));
        }

        lines.push(Line::from(row_spans));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![TextSpan::styled(
        "Press Enter on a span to view details",
        Style::default().fg(Color::Yellow),
    )]));

    Text::from(lines)
}

/// Get color for span status
fn get_span_status_color(status: &str) -> Color {
    match status.to_uppercase().as_str() {
        "OK" | "SUCCESS" => Color::Green,
        "ERROR" | "FAILED" => Color::Red,
        "UNSET" | "UNKNOWN" => Color::Gray,
        _ => Color::White,
    }
}

/// Render a single-line filter input bar (vim-style prompt at the bottom of the pane)
fn render_filter_input_bar(frame: &mut Frame, area: Rect, buffer: &str) {
    let line = Line::from(vec![
        TextSpan::styled(
            "Filter: ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        TextSpan::raw(buffer.to_string()),
        TextSpan::styled("█", Style::default().fg(Color::Yellow)),
        TextSpan::styled(
            "  (Enter to apply, Esc to cancel, key=value or text)",
            Style::default().fg(Color::DarkGray),
        ),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

/// Render status bar
fn render_status_bar(frame: &mut Frame, area: Rect, state: &TracesState, api_error: Option<&str>) {
    let mut status_parts = vec![];

    // View indicator
    status_parts.push(TextSpan::styled(
        " TRACES ",
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    ));

    // Connection status
    status_parts.push(TextSpan::raw(" "));
    if api_error.is_some() {
        status_parts.push(TextSpan::styled(
            "Disconnected",
            Style::default().fg(Color::Red),
        ));
    } else {
        status_parts.push(TextSpan::styled(
            "Connected",
            Style::default().fg(Color::Green),
        ));
    }

    // Item count
    status_parts.push(TextSpan::styled(
        format!(" | Traces: {} ", state.filtered_traces().len()),
        Style::default().fg(Color::Gray),
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
        Style::default().fg(Color::Gray),
    ));

    let status_line = Line::from(status_parts);
    let paragraph = Paragraph::new(status_line);
    frame.render_widget(paragraph, area);
}

/// Format timestamp for the traces list view (8 chars: HH:MM:SS, local time).
fn format_timestamp(timestamp_ns: i64) -> String {
    use chrono::{DateTime, Local, Utc};

    DateTime::<Utc>::from_timestamp_millis(timestamp_ns / 1_000_000)
        .map(|dt| dt.with_timezone(&Local).format("%H:%M:%S").to_string())
        .unwrap_or_else(|| "?".to_string())
}

/// Format timestamp for detail panels: full ISO 8601 with seconds and UTC offset.
fn format_timestamp_full(timestamp_ns: i64) -> String {
    use chrono::{DateTime, Local, Utc};

    DateTime::<Utc>::from_timestamp_millis(timestamp_ns / 1_000_000)
        .map(|dt| {
            dt.with_timezone(&Local)
                .format("%Y-%m-%d %H:%M:%S %z")
                .to_string()
        })
        .unwrap_or_else(|| "?".to_string())
}

/// Format a duration in nanoseconds as a human-readable string.
/// Shows "<1ms" for sub-millisecond durations to avoid misleading "0ms".
fn format_duration(ns: i64) -> String {
    let ms = ns / 1_000_000;
    if ms == 0 && ns > 0 {
        "<1ms".to_string()
    } else {
        format!("{}ms", ms)
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
    use crate::api::models::{Resource, SpanStatus};
    use std::collections::HashMap;

    fn create_test_span(
        span_id: &str,
        name: &str,
        start_time: i64,
        duration: i64,
        parent_span_id: Option<String>,
    ) -> Span {
        Span {
            trace_id: "test-trace-123".to_string(),
            span_id: span_id.to_string(),
            parent_span_id,
            name: name.to_string(),
            kind: "INTERNAL".to_string(),
            start_time,
            end_time: start_time + duration,
            duration,
            status: SpanStatus {
                code: "Ok".to_string(),
                message: None,
            },
            attributes: HashMap::new(),
            events: vec![],
            resource: Some(Resource {
                attributes: HashMap::new(),
            }),
        }
    }

    #[test]
    fn test_get_span_status_color() {
        assert_eq!(get_span_status_color("OK"), Color::Green);
        assert_eq!(get_span_status_color("ERROR"), Color::Red);
        assert_eq!(get_span_status_color("UNSET"), Color::Gray);
        assert_eq!(get_span_status_color("SUCCESS"), Color::Green);
        assert_eq!(get_span_status_color("FAILED"), Color::Red);
    }

    #[test]
    fn test_get_timing_bar_color() {
        assert_eq!(get_timing_bar_color("ERROR", 30.0), Color::Red);
        assert_eq!(get_timing_bar_color("Ok", 30.0), Color::Green);
        assert_eq!(get_timing_bar_color("Ok", 60.0), Color::Yellow);
        assert_eq!(get_timing_bar_color("FAILED", 60.0), Color::Red);
    }

    #[test]
    fn test_format_timestamp() {
        let timestamp_ns: i64 = 1713360896789 * 1_000_000;
        let formatted = format_timestamp(timestamp_ns);
        // HH:MM:SS — 8 chars
        assert_eq!(formatted.len(), 8);
        assert!(formatted.contains(':'));
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

    #[test]
    fn test_render_timing_bar_empty() {
        let bar = render_timing_bar(0, 100, 0, 40);
        assert_eq!(bar, "");

        let bar = render_timing_bar(0, 100, 1000, 0);
        assert_eq!(bar, "");
    }

    #[test]
    fn test_render_timing_bar_full_width() {
        let bar = render_timing_bar(0, 1000, 1000, 10);
        // Bar uses Unicode characters (3 bytes each for █ and ░)
        assert!(!bar.is_empty());
        assert!(bar.contains('█'));
        // Count actual characters
        assert_eq!(bar.chars().count(), 10);
    }

    #[test]
    fn test_render_timing_bar_partial() {
        let bar = render_timing_bar(0, 500, 1000, 10);
        assert!(!bar.is_empty());
        assert!(bar.contains('█'));
        assert!(bar.contains('░'));
        // Count actual characters
        assert_eq!(bar.chars().count(), 10);
    }

    #[test]
    fn test_build_span_tree_single_span() {
        let span = create_test_span("span1", "test-span", 1000, 100, None);
        let trace = Trace {
            trace_id: "test-trace-123".to_string(),
            start_time: 1000,
            end_time: 1100,
            duration: 100,
            span_count: 1,
            service_names: vec!["test-service".to_string()],
            spans: vec![span],
        };

        let nodes = build_span_tree(&trace);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].depth, 0);
        assert_eq!(nodes[0].relative_start, 0);
    }

    #[test]
    fn test_build_span_tree_parent_child() {
        let parent = create_test_span("span1", "parent", 1000, 200, None);
        let child = create_test_span("span2", "child", 1050, 100, Some("span1".to_string()));

        let trace = Trace {
            trace_id: "test-trace-123".to_string(),
            start_time: 1000,
            end_time: 1200,
            duration: 200,
            span_count: 2,
            service_names: vec!["test-service".to_string()],
            spans: vec![parent, child],
        };

        let nodes = build_span_tree(&trace);
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].depth, 0);
        assert_eq!(nodes[1].depth, 1);
        assert_eq!(nodes[1].relative_start, 50);
    }

    #[test]
    fn test_span_node_duration_percent() {
        let span = create_test_span("span1", "test", 1000, 500, None);
        let trace = Trace {
            trace_id: "test-trace-123".to_string(),
            start_time: 1000,
            end_time: 2000,
            duration: 1000,
            span_count: 1,
            service_names: vec!["test-service".to_string()],
            spans: vec![span],
        };

        let nodes = build_span_tree(&trace);
        assert_eq!(nodes[0].duration_percent, 50.0);
    }
}
