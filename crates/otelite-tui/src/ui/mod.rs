pub mod help;
pub mod logs;
pub mod metrics;
pub mod traces;

pub use help::render_help_view;
pub use logs::render_logs_view;
pub use metrics::render_metrics_view;
pub use traces::render_traces_view;

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

/// Render a tab bar showing available views with the active one highlighted.
/// Height = 1 line. Caller allocates a 1-line Rect at the top.
pub fn render_tab_bar(frame: &mut Frame, area: Rect, active: &str) {
    let tabs: &[(&str, &str)] = &[("l", "Logs"), ("t", "Traces"), ("m", "Metrics")];

    let mut spans: Vec<Span> = Vec::new();
    for (key, name) in tabs {
        if name.to_lowercase() == active.to_lowercase() {
            spans.push(Span::styled(
                format!("[{key}:{name}]"),
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            spans.push(Span::styled(
                format!(" {key}:{name} "),
                Style::default().fg(Color::White),
            ));
        }
        spans.push(Span::raw("  "));
    }
    spans.push(Span::styled("?:Help", Style::default().fg(Color::Yellow)));
    spans.push(Span::raw("  "));
    spans.push(Span::styled("q:Quit", Style::default().fg(Color::Red)));

    let line = Line::from(spans);
    frame.render_widget(Paragraph::new(line), area);
}
