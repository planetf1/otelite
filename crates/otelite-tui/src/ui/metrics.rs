use crate::api::models::Metric;
use crate::state::MetricsState;
use crate::ui::{logs::render_api_error_banner, render_tab_bar};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Sparkline, Table, TableState, Wrap},
    Frame,
};

/// Render the metrics view
pub fn render_metrics_view(
    frame: &mut Frame,
    area: Rect,
    state: &MetricsState,
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

    // Split the area into tab bar, main content and status bar
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Tab bar
            Constraint::Min(3),    // Main content
            Constraint::Length(1), // Status bar
        ])
        .split(content_area);

    render_tab_bar(frame, chunks[0], "Metrics");

    // Render main content (table or table + detail)
    if state.show_detail {
        render_metrics_with_detail(frame, chunks[1], state);
    } else {
        render_metrics_table(frame, chunks[1], state);
    }

    // Render status bar
    render_status_bar(frame, chunks[2], state, api_error);
}

/// Render metrics table only
fn render_metrics_table(frame: &mut Frame, area: Rect, state: &MetricsState) {
    use crate::api::models::MetricValue;

    // Deduplicated list: one row per unique metric name
    let unique_metrics = state.unique_filtered_metrics();

    // Empty state: no data and no error
    if unique_metrics.is_empty() && state.error.is_none() {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Metrics (0) ");
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let paragraph =
            Paragraph::new("No metrics yet — send OTLP data to :4317 (gRPC) or :4318 (HTTP)")
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

    // Create rows — no manual selection; row_highlight_style handles it
    let rows: Vec<Row> = unique_metrics
        .iter()
        .map(|(metric, count)| {
            let latest_value = match &metric.value {
                MetricValue::Gauge(v) => format!("{:.2}", v),
                MetricValue::Counter(v) => format!("{}", v),
                MetricValue::Histogram(h) if h.count > 0 => {
                    format!("avg {:.1}", h.sum / h.count as f64)
                },
                MetricValue::Histogram(h) => format!("count={}", h.count),
                MetricValue::Summary(s) if s.count > 0 => {
                    format!("avg {:.1}", s.sum / s.count as f64)
                },
                MetricValue::Summary(s) => format!("count={}", s.count),
            };

            let unit = metric.unit.as_deref().unwrap_or("");
            let value_with_unit = if unit.is_empty() {
                latest_value
            } else {
                format!("{} {}", latest_value, unit)
            };

            let type_cell = Cell::from(metric.metric_type.clone()).style(
                Style::default()
                    .fg(get_metric_type_color(&metric.metric_type))
                    .add_modifier(Modifier::BOLD),
            );

            Row::new(vec![
                Cell::from(truncate_string(&metric.name, 38)),
                type_cell,
                Cell::from(value_with_unit),
                Cell::from(count.to_string()),
                Cell::from(truncate_string(
                    metric.description.as_deref().unwrap_or(""),
                    40,
                )),
            ])
            .height(1)
        })
        .collect();

    let header = Row::new(vec!["Name", "Type", "Latest Value", "Pts", "Description"])
        .style(Style::default().add_modifier(Modifier::BOLD))
        .bottom_margin(1);

    let table = Table::new(
        rows,
        [
            Constraint::Min(30),    // Name
            Constraint::Length(10), // Type
            Constraint::Length(16), // Latest Value
            Constraint::Length(5),  // Points
            Constraint::Min(20),    // Description
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" Metrics ({}) ", unique_metrics.len())),
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

/// Render metrics table with detail panel
fn render_metrics_with_detail(frame: &mut Frame, area: Rect, state: &MetricsState) {
    // Split area horizontally
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50), // Table
            Constraint::Percentage(50), // Detail
        ])
        .split(area);

    // Render table
    render_metrics_table(frame, chunks[0], state);

    // Render detail panel
    render_detail_panel(frame, chunks[1], state);
}

/// Render metric detail panel
fn render_detail_panel(frame: &mut Frame, area: Rect, state: &MetricsState) {
    if let Some(metric) = state.selected_metric() {
        // Split detail area into info and chart
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(12), // Info
                Constraint::Min(5),     // Chart
            ])
            .split(area);

        // Render metric info
        render_metric_info(frame, chunks[0], metric);

        // Render sparkline chart
        render_metric_chart(frame, chunks[1], metric, state);
    } else {
        let paragraph = Paragraph::new("No metric selected").block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Metric Detail "),
        );
        frame.render_widget(paragraph, area);
    }
}

/// Render metric information
fn render_metric_info(frame: &mut Frame, area: Rect, metric: &Metric) {
    use crate::api::models::MetricValue;

    let latest_value = match &metric.value {
        MetricValue::Gauge(v) => format!("{:.2}", v),
        MetricValue::Counter(v) => format!("{}", v),
        MetricValue::Histogram(h) => format!("sum={:.2}, count={}", h.sum, h.count),
        MetricValue::Summary(s) => format!("sum={:.2}, count={}", s.sum, s.count),
    };

    let unit = metric.unit.as_deref().unwrap_or("none");

    let lines = vec![
        Line::from(vec![
            Span::styled("Name: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(&metric.name),
        ]),
        Line::from(vec![
            Span::styled("Type: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(&metric.metric_type),
        ]),
        Line::from(vec![
            Span::styled("Unit: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(unit),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Value: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(latest_value, Style::default().fg(Color::Green)),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Description:",
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        Line::from(metric.description.as_deref().unwrap_or("No description")),
    ];

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Metric Info "),
        )
        .wrap(Wrap { trim: true });

    frame.render_widget(paragraph, area);
}

/// Render metric chart (sparkline)
fn render_metric_chart(frame: &mut Frame, area: Rect, metric: &Metric, state: &MetricsState) {
    use crate::api::models::MetricValue;

    // Get metric history for sparkline
    if let Some(history) = state.get_metric_history(&metric.name) {
        if history.len() > 1 {
            // Split area for sparkline and stats
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(3),    // Sparkline
                    Constraint::Length(3), // Stats
                ])
                .split(area);

            // Calculate min, max, current
            let min = history.iter().copied().fold(f64::INFINITY, f64::min);
            let max = history.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let current = history.last().copied().unwrap_or(0.0);

            // Determine trend and color
            let (trend_color, trend_indicator) = if history.len() >= 2 {
                let prev = history[history.len() - 2];
                let change = current - prev;
                let change_pct = if prev != 0.0 {
                    (change / prev) * 100.0
                } else {
                    0.0
                };

                // Color logic based on metric type and trend
                let color = match &metric.metric_type.to_lowercase().as_str() {
                    &"counter" if metric.name.contains("error") || metric.name.contains("fail") => {
                        if current > 0.0 {
                            Color::Red
                        } else {
                            Color::Green
                        }
                    },
                    _ => {
                        if change_pct.abs() < 1.0 {
                            Color::Green // Stable
                        } else if change > 0.0 {
                            Color::Yellow // Increasing
                        } else {
                            Color::Cyan // Decreasing
                        }
                    },
                };

                let indicator = if change > 0.0 {
                    "↑"
                } else if change < 0.0 {
                    "↓"
                } else {
                    "→"
                };
                (color, indicator)
            } else {
                (Color::Green, "→")
            };

            // Convert f64 history to u64 for Sparkline widget
            let sparkline_data: Vec<u64> = history.iter().map(|&v| v.max(0.0) as u64).collect();
            let max_u64 = max.max(0.0) as u64;

            // Render sparkline
            let sparkline = Sparkline::default()
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(format!(" Trend (last {} points) ", history.len())),
                )
                .data(&sparkline_data)
                .style(Style::default().fg(trend_color))
                .max(max_u64)
                .direction(ratatui::widgets::RenderDirection::LeftToRight);

            frame.render_widget(sparkline, chunks[0]);

            // Render stats
            let stats_text = format!(
                "Min: {:.2}  Max: {:.2}  Current: {:.2} {}",
                min, max, current, trend_indicator
            );
            let stats = Paragraph::new(stats_text)
                .style(Style::default().fg(trend_color))
                .block(Block::default().borders(Borders::ALL).title(" Stats "));

            frame.render_widget(stats, chunks[1]);
            return;
        }
    }

    // Fallback: show current value only (no history yet)
    let display_text = match &metric.value {
        MetricValue::Gauge(v) => format!("Current: {:.2}", v),
        MetricValue::Counter(v) => format!("Total: {}", v),
        MetricValue::Histogram(h) => {
            format!(
                "Count: {}, Sum: {:.2}, Buckets: {}",
                h.count,
                h.sum,
                h.buckets.len()
            )
        },
        MetricValue::Summary(s) => {
            format!(
                "Count: {}, Sum: {:.2}, Quantiles: {}",
                s.count,
                s.sum,
                s.quantiles.len()
            )
        },
    };

    let paragraph = Paragraph::new(vec![
        Line::from(display_text),
        Line::from(""),
        Line::from(Span::styled(
            "Waiting for more data points...",
            Style::default().fg(Color::DarkGray),
        )),
    ])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Current Value "),
    );
    frame.render_widget(paragraph, area);
}

/// Render status bar
fn render_status_bar(frame: &mut Frame, area: Rect, state: &MetricsState, api_error: Option<&str>) {
    let mut status_parts = vec![];

    // View indicator
    status_parts.push(Span::styled(
        " METRICS ",
        Style::default()
            .fg(Color::Black)
            .bg(Color::Magenta)
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
        format!(" | Metrics: {} ", state.unique_filtered_metrics().len()),
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
        "↑↓: Navigate | Enter: Detail | /: Search | f: Filter | q: Quit",
        Style::default().fg(Color::DarkGray),
    ));

    let status_line = Line::from(status_parts);
    let paragraph = Paragraph::new(status_line);
    frame.render_widget(paragraph, area);
}

/// Color coding for metric types
fn get_metric_type_color(metric_type: &str) -> Color {
    match metric_type.to_lowercase().as_str() {
        "counter" => Color::Green,
        "gauge" => Color::Blue,
        "histogram" => Color::Magenta,
        "summary" => Color::Yellow,
        _ => Color::White,
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
    use crate::api::models::{MetricValue, Resource};
    use std::collections::HashMap;

    fn create_test_metric(name: &str, value: MetricValue) -> Metric {
        Metric {
            name: name.to_string(),
            description: Some("Test metric".to_string()),
            unit: Some("ms".to_string()),
            metric_type: "gauge".to_string(),
            value,
            timestamp: 1713360896789,
            resource: Some(Resource {
                attributes: HashMap::new(),
            }),
            attributes: HashMap::new(),
        }
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
    fn test_truncate_string_exact_length() {
        let exact = "Exactly20Characters!";
        assert_eq!(truncate_string(exact, 20), exact);
    }

    #[test]
    fn test_truncate_string_empty() {
        assert_eq!(truncate_string("", 10), "");
    }

    #[test]
    fn test_gauge_metric_formatting() {
        let metric = create_test_metric("test.gauge", MetricValue::Gauge(42.5));

        // Verify metric structure
        assert_eq!(metric.name, "test.gauge");
        assert_eq!(metric.unit, Some("ms".to_string()));

        // Test value formatting
        if let MetricValue::Gauge(v) = metric.value {
            let formatted = format!("{:.2}", v);
            assert_eq!(formatted, "42.50");
        } else {
            panic!("Expected Gauge value");
        }
    }

    #[test]
    fn test_counter_metric_formatting() {
        let metric = create_test_metric("test.counter", MetricValue::Counter(100));

        if let MetricValue::Counter(v) = metric.value {
            let formatted = format!("{}", v);
            assert_eq!(formatted, "100");
        } else {
            panic!("Expected Counter value");
        }
    }

    #[test]
    fn test_histogram_metric_formatting() {
        use otelite_core::api::HistogramValue;

        let metric = create_test_metric(
            "test.histogram",
            MetricValue::Histogram(HistogramValue {
                count: 10,
                sum: 123.45,
                buckets: vec![],
            }),
        );

        if let MetricValue::Histogram(h) = metric.value {
            let formatted = format!("sum={:.2}, count={}", h.sum, h.count);
            assert_eq!(formatted, "sum=123.45, count=10");
        } else {
            panic!("Expected Histogram value");
        }
    }

    #[test]
    fn test_summary_metric_formatting() {
        use otelite_core::api::SummaryValue;

        let metric = create_test_metric(
            "test.summary",
            MetricValue::Summary(SummaryValue {
                count: 5,
                sum: 67.89,
                quantiles: vec![],
            }),
        );

        if let MetricValue::Summary(s) = metric.value {
            let formatted = format!("sum={:.2}, count={}", s.sum, s.count);
            assert_eq!(formatted, "sum=67.89, count=5");
        } else {
            panic!("Expected Summary value");
        }
    }

    #[test]
    fn test_metric_with_no_unit() {
        let mut metric = create_test_metric("test.no_unit", MetricValue::Gauge(1.0));
        metric.unit = None;

        let unit = metric.unit.as_deref().unwrap_or("none");
        assert_eq!(unit, "none");
    }

    #[test]
    fn test_metric_with_no_description() {
        let mut metric = create_test_metric("test.no_desc", MetricValue::Gauge(1.0));
        metric.description = None;

        let desc = metric.description.as_deref().unwrap_or("No description");
        assert_eq!(desc, "No description");
    }

    #[test]
    fn test_histogram_with_buckets() {
        use crate::api::models::{HistogramBucket, HistogramValue};
        let metric = create_test_metric(
            "test.histogram",
            MetricValue::Histogram(HistogramValue {
                count: 100,
                sum: 500.0,
                buckets: vec![
                    HistogramBucket {
                        upper_bound: 10.0,
                        count: 10,
                    },
                    HistogramBucket {
                        upper_bound: 20.0,
                        count: 20,
                    },
                    HistogramBucket {
                        upper_bound: 30.0,
                        count: 30,
                    },
                    HistogramBucket {
                        upper_bound: 40.0,
                        count: 40,
                    },
                ],
            }),
        );

        if let MetricValue::Histogram(h) = metric.value {
            assert_eq!(h.buckets.len(), 4);
        } else {
            panic!("Expected Histogram value");
        }
    }

    #[test]
    fn test_summary_with_quantiles() {
        use crate::api::models::{Quantile, SummaryValue};
        let metric = create_test_metric(
            "test.summary",
            MetricValue::Summary(SummaryValue {
                count: 50,
                sum: 250.0,
                quantiles: vec![
                    Quantile {
                        quantile: 0.5,
                        value: 100.0,
                    },
                    Quantile {
                        quantile: 0.9,
                        value: 200.0,
                    },
                    Quantile {
                        quantile: 0.99,
                        value: 250.0,
                    },
                ],
            }),
        );

        if let MetricValue::Summary(s) = metric.value {
            assert_eq!(s.quantiles.len(), 3);
        } else {
            panic!("Expected Summary value");
        }
    }
}
