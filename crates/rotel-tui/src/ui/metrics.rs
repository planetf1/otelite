use crate::api::models::Metric;
use crate::state::MetricsState;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Row, Table, Wrap},
    Frame,
};

/// Render the metrics view
pub fn render_metrics_view(frame: &mut Frame, area: Rect, state: &MetricsState) {
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
        render_metrics_with_detail(frame, chunks[0], state);
    } else {
        render_metrics_table(frame, chunks[0], state);
    }

    // Render status bar
    render_status_bar(frame, chunks[1], state);
}

/// Render metrics table only
fn render_metrics_table(frame: &mut Frame, area: Rect, state: &MetricsState) {
    let filtered_metrics = state.filtered_metrics();

    // Create table rows
    let rows: Vec<Row> = filtered_metrics
        .iter()
        .enumerate()
        .map(|(idx, metric)| {
            let style = if idx == state.selected_index {
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            use crate::api::models::MetricValue;

            let latest_value = match &metric.value {
                MetricValue::Gauge(v) => format!("{:.2}", v),
                MetricValue::Counter(v) => format!("{}", v),
                MetricValue::Histogram { sum, count, .. } => {
                    format!("sum={:.2}, count={}", sum, count)
                },
                MetricValue::Summary { sum, count, .. } => {
                    format!("sum={:.2}, count={}", sum, count)
                },
            };

            let unit = metric.unit.as_deref().unwrap_or("");
            let data_point_count = 1; // Single data point per metric now

            Row::new(vec![
                truncate_string(&metric.name, 40),
                metric.metric_type.clone(),
                format!("{} {}", latest_value, unit),
                data_point_count.to_string(),
                metric.description.as_deref().unwrap_or("").to_string(),
            ])
            .style(style)
            .height(1)
        })
        .collect();

    // Create table header
    let header = Row::new(vec![
        "Name",
        "Type",
        "Latest Value",
        "Points",
        "Description",
    ])
    .style(Style::default().add_modifier(Modifier::BOLD))
    .bottom_margin(1);

    // Create table widget
    let table = Table::new(
        rows,
        [
            Constraint::Min(30),    // Name
            Constraint::Length(12), // Type
            Constraint::Length(15), // Latest Value
            Constraint::Length(8),  // Points
            Constraint::Min(20),    // Description
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" Metrics ({}) ", filtered_metrics.len())),
    )
    .highlight_style(
        Style::default()
            .bg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
    );

    frame.render_widget(table, area);
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
        render_metric_chart(frame, chunks[1], metric);
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
        MetricValue::Histogram { sum, count, .. } => format!("sum={:.2}, count={}", sum, count),
        MetricValue::Summary { sum, count, .. } => format!("sum={:.2}, count={}", sum, count),
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
fn render_metric_chart(frame: &mut Frame, area: Rect, metric: &Metric) {
    use crate::api::models::MetricValue;

    // For single-value metrics, show a simple display instead of sparkline
    let display_text = match &metric.value {
        MetricValue::Gauge(v) => format!("Current: {:.2}", v),
        MetricValue::Counter(v) => format!("Total: {}", v),
        MetricValue::Histogram {
            count,
            sum,
            buckets,
        } => {
            format!(
                "Count: {}, Sum: {:.2}, Buckets: {}",
                count,
                sum,
                buckets.len()
            )
        },
        MetricValue::Summary {
            count,
            sum,
            quantiles,
        } => {
            format!(
                "Count: {}, Sum: {:.2}, Quantiles: {}",
                count,
                sum,
                quantiles.len()
            )
        },
    };

    let paragraph = Paragraph::new(display_text).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Current Value "),
    );
    frame.render_widget(paragraph, area);
}

/// Render status bar
fn render_status_bar(frame: &mut Frame, area: Rect, state: &MetricsState) {
    let mut status_parts = vec![];

    // View indicator
    status_parts.push(Span::styled(
        " METRICS ",
        Style::default()
            .fg(Color::Black)
            .bg(Color::Magenta)
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
