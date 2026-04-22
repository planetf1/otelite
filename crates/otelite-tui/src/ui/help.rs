use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

/// Render the help screen
pub fn render_help_view(frame: &mut Frame, area: Rect, version: &str) {
    let help_text = create_help_text(version);

    let paragraph = Paragraph::new(help_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Otelite TUI - Help ")
                .style(Style::default().fg(Color::White)),
        )
        .wrap(Wrap { trim: true });

    frame.render_widget(paragraph, area);
}

/// Create help text content
fn create_help_text(version: &str) -> Text<'static> {
    let version_line = format!("Version: {version}");
    let lines = vec![
        Line::from(vec![Span::styled(
            "Otelite TUI - OpenTelemetry Receiver & Dashboard",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![Span::styled(
            version_line,
            Style::default().fg(Color::DarkGray),
        )]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "NAVIGATION",
            Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        )]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  l", Style::default().fg(Color::Yellow)),
            Span::raw("  Switch to Logs view"),
        ]),
        Line::from(vec![
            Span::styled("  t", Style::default().fg(Color::Yellow)),
            Span::raw("  Switch to Traces view"),
        ]),
        Line::from(vec![
            Span::styled("  m", Style::default().fg(Color::Yellow)),
            Span::raw("  Switch to Metrics view"),
        ]),
        Line::from(vec![
            Span::styled("  Tab / Shift+Tab", Style::default().fg(Color::Yellow)),
            Span::raw("  Next / previous view"),
        ]),
        Line::from(vec![
            Span::styled("  ?", Style::default().fg(Color::Yellow)),
            Span::raw("  Show this help screen"),
        ]),
        Line::from(vec![
            Span::styled("  q", Style::default().fg(Color::Yellow)),
            Span::raw("  Quit application"),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "VIEW CONTROLS",
            Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        )]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  ↑/k", Style::default().fg(Color::Yellow)),
            Span::raw("  Move selection up"),
        ]),
        Line::from(vec![
            Span::styled("  ↓/j", Style::default().fg(Color::Yellow)),
            Span::raw("  Move selection down"),
        ]),
        Line::from(vec![
            Span::styled("  Enter", Style::default().fg(Color::Yellow)),
            Span::raw("  Show detail panel for selected item"),
        ]),
        Line::from(vec![
            Span::styled("  Esc", Style::default().fg(Color::Yellow)),
            Span::raw("  Close detail panel / Go back"),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "SEARCH & FILTER",
            Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        )]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  /", Style::default().fg(Color::Yellow)),
            Span::raw("  Start search (type to filter results)"),
        ]),
        Line::from(vec![
            Span::styled("  f", Style::default().fg(Color::Yellow)),
            Span::raw("  Add filter (e.g., severity=ERROR)"),
        ]),
        Line::from(vec![
            Span::styled("  c", Style::default().fg(Color::Yellow)),
            Span::raw("  Clear all filters and search"),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "LOGS VIEW",
            Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        )]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  a", Style::default().fg(Color::Yellow)),
            Span::raw("  Toggle auto-scroll (automatically scroll to newest logs)"),
        ]),
        Line::from(vec![Span::raw(
            "  • Logs are color-coded by severity (TRACE → ERROR)",
        )]),
        Line::from(vec![Span::raw(
            "  • Press Enter to view full log details with attributes",
        )]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "TRACES VIEW",
            Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        )]),
        Line::from(""),
        Line::from(vec![Span::raw(
            "  • View distributed traces with span waterfall",
        )]),
        Line::from(vec![Span::raw(
            "  • Error indicator (⚠) shows traces with errors",
        )]),
        Line::from(vec![Span::raw(
            "  • Press Enter to view span hierarchy and timing",
        )]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "METRICS VIEW",
            Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        )]),
        Line::from(""),
        Line::from(vec![Span::raw(
            "  • View metrics with latest values and sparkline charts",
        )]),
        Line::from(vec![Span::raw(
            "  • Press Enter to see detailed metric info and value history",
        )]),
        Line::from(vec![Span::raw(
            "  • Charts show min, max, and average values",
        )]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "TIPS",
            Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        )]),
        Line::from(""),
        Line::from(vec![Span::raw(
            "  • Data refreshes automatically based on configured interval",
        )]),
        Line::from(vec![Span::raw(
            "  • Use filters to focus on specific data (e.g., severity=ERROR)",
        )]),
        Line::from(vec![Span::raw(
            "  • Search works across all visible fields in each view",
        )]),
        Line::from(vec![Span::raw(
            "  • Status bar shows active filters and search queries",
        )]),
        Line::from(""),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Press Esc to return to the previous view",
            Style::default().fg(Color::DarkGray),
        )]),
    ];

    Text::from(lines)
}
