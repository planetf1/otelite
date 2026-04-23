use anyhow::Result;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::time::{Duration, Instant};

use crate::api::ApiClient;
use crate::config::Config;
use crate::events::{poll_event, AppEvent};
use crate::state::{LogsState, MetricsState, TracesState};
use crate::ui;

/// Current view in the TUI
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum View {
    Logs,
    Traces,
    Metrics,
    Help,
}

/// Main application state
///
/// Performance optimizations:
/// - Lazy initialization: Views are created upfront but data is loaded on-demand
/// - Pagination: State modules use PaginatedList to limit memory (max 1000 items)
/// - Debouncing: UpdateTracker prevents excessive API calls (min 100ms between updates)
/// - Efficient rendering: Ratatui handles double-buffering and minimal redraws
/// - Response caching: Trace details are cached for 5 minutes to reduce API calls
pub struct App {
    config: Config,
    current_view: View,
    should_quit: bool,
    api_client: ApiClient,
    logs_state: LogsState,
    traces_state: TracesState,
    metrics_state: MetricsState,
    last_refresh: Instant,
    /// Whether filter input mode is active
    pub filter_input_active: bool,
    /// The string being typed in filter input mode
    pub filter_input_buffer: String,
    /// Last API error, cleared on success
    pub api_error: Option<String>,
}

impl App {
    /// Create a new application instance
    pub fn new(config: Config) -> Self {
        let current_view = match config.initial_view.as_str() {
            "traces" => View::Traces,
            "metrics" => View::Metrics,
            _ => View::Logs,
        };

        let api_client = ApiClient::new(config.api_url.clone(), Duration::from_secs(30))
            .expect("Failed to create HTTP client");

        Self {
            config,
            current_view,
            should_quit: false,
            api_client,
            logs_state: LogsState::new(),
            traces_state: TracesState::new(),
            metrics_state: MetricsState::new(),
            last_refresh: Instant::now(),
            filter_input_active: false,
            filter_input_buffer: String::new(),
            api_error: None,
        }
    }

    /// Handle an application event
    pub fn handle_event(&mut self, event: AppEvent) {
        // Filter input mode: intercept most keys for text entry
        if self.filter_input_active {
            match event {
                AppEvent::Select => {
                    // Enter: apply the filter and return to normal mode
                    self.apply_filter_input();
                    self.filter_input_active = false;
                    self.filter_input_buffer.clear();
                },
                AppEvent::Back => {
                    // Esc: cancel without applying
                    self.filter_input_active = false;
                    self.filter_input_buffer.clear();
                },
                AppEvent::Backspace => {
                    self.filter_input_buffer.pop();
                },
                AppEvent::Char(c) => {
                    self.filter_input_buffer.push(c);
                },
                _ => {},
            }
            return;
        }

        match event {
            AppEvent::Quit => self.should_quit = true,
            AppEvent::SwitchToLogs => self.current_view = View::Logs,
            AppEvent::SwitchToTraces => self.current_view = View::Traces,
            AppEvent::SwitchToMetrics => self.current_view = View::Metrics,
            AppEvent::ShowHelp => self.current_view = View::Help,
            AppEvent::NextView => {
                self.current_view = match self.current_view {
                    View::Logs => View::Traces,
                    View::Traces => View::Metrics,
                    View::Metrics | View::Help => View::Logs,
                };
            },
            AppEvent::PrevView => {
                self.current_view = match self.current_view {
                    View::Logs | View::Help => View::Metrics,
                    View::Traces => View::Logs,
                    View::Metrics => View::Traces,
                };
            },
            AppEvent::Filter if matches!(self.current_view, View::Logs | View::Traces) => {
                self.filter_input_active = true;
                self.filter_input_buffer.clear();
            },
            AppEvent::Back => {
                if self.current_view == View::Help {
                    self.current_view = View::Logs;
                } else {
                    // If filters are active, Esc clears them first
                    match self.current_view {
                        View::Logs if !self.logs_state.filters.is_empty() => {
                            self.logs_state.clear_filters();
                        },
                        View::Traces if !self.traces_state.filters.is_empty() => {
                            self.traces_state.clear_filters();
                        },
                        // Close detail panels in each view
                        View::Logs if self.logs_state.show_detail => {
                            self.logs_state.hide_detail_panel();
                        },
                        View::Traces if self.traces_state.show_span_detail => {
                            // Exit span detail back to trace detail
                            self.traces_state.toggle_span_detail();
                        },
                        View::Traces if self.traces_state.show_detail => {
                            // Exit trace detail back to trace list
                            self.traces_state.hide_detail_panel();
                            self.traces_state.reset_span_selection();
                        },
                        View::Metrics if self.metrics_state.show_detail => {
                            self.metrics_state.hide_detail_panel();
                        },
                        _ => {},
                    }
                }
            },
            // Logs view events
            AppEvent::Up if self.current_view == View::Logs => {
                self.logs_state.select_previous();
            },
            AppEvent::Down if self.current_view == View::Logs => {
                self.logs_state.select_next();
            },
            AppEvent::Select if self.current_view == View::Logs => {
                self.logs_state.show_detail_panel();
            },
            AppEvent::ToggleAutoScroll if self.current_view == View::Logs => {
                self.logs_state.toggle_auto_scroll();
            },
            // Traces view events
            AppEvent::Up if self.current_view == View::Traces => {
                if self.traces_state.show_span_detail {
                    // In span detail view, do nothing (Esc to go back)
                } else if self.traces_state.show_detail {
                    // In trace detail view, navigate spans
                    if let Some(_trace) = self.traces_state.selected_trace_details() {
                        self.traces_state.select_previous_span();
                    }
                } else {
                    // In trace list view, navigate traces
                    self.traces_state.select_previous();
                }
            },
            AppEvent::Down if self.current_view == View::Traces => {
                if self.traces_state.show_span_detail {
                    // In span detail view, do nothing (Esc to go back)
                } else if self.traces_state.show_detail {
                    // In trace detail view, navigate spans
                    if let Some(trace) = self.traces_state.selected_trace_details() {
                        self.traces_state.select_next_span(trace.spans.len());
                    }
                } else {
                    // In trace list view, navigate traces
                    self.traces_state.select_next();
                }
            },
            AppEvent::Select if self.current_view == View::Traces => {
                if self.traces_state.show_span_detail {
                    // Already in span detail, do nothing
                } else if self.traces_state.show_detail {
                    // In trace detail view, show span detail
                    self.traces_state.toggle_span_detail();
                } else {
                    // In trace list view, show trace detail (triggers API load via pending_detail_load)
                    self.traces_state.show_detail_panel();
                    self.traces_state.reset_span_selection();
                }
            },
            // Metrics view events
            AppEvent::Up if self.current_view == View::Metrics => {
                self.metrics_state.select_previous();
            },
            AppEvent::Down if self.current_view == View::Metrics => {
                self.metrics_state.select_next();
            },
            AppEvent::Select if self.current_view == View::Metrics => {
                self.metrics_state.show_detail_panel();
            },
            // Page navigation — most-specific guards first so the right arm matches
            // Logs: PageDown/PageUp scrolls detail text when detail panel is open,
            //       otherwise pages through the list.
            AppEvent::PageDown
                if self.current_view == View::Logs && self.logs_state.show_detail =>
            {
                self.logs_state.scroll_detail_down(10);
            },
            AppEvent::PageUp if self.current_view == View::Logs && self.logs_state.show_detail => {
                self.logs_state.scroll_detail_up(10);
            },
            AppEvent::PageDown if self.current_view == View::Logs => {
                self.logs_state.select_page_down(10);
            },
            AppEvent::PageUp if self.current_view == View::Logs => {
                self.logs_state.select_page_up(10);
            },
            // Traces: scroll span detail text → page waterfall spans → page trace list
            AppEvent::PageDown
                if self.current_view == View::Traces && self.traces_state.show_span_detail =>
            {
                self.traces_state.scroll_span_detail_down(10);
            },
            AppEvent::PageUp
                if self.current_view == View::Traces && self.traces_state.show_span_detail =>
            {
                self.traces_state.scroll_span_detail_up(10);
            },
            AppEvent::PageDown
                if self.current_view == View::Traces && self.traces_state.show_detail =>
            {
                let max = self
                    .traces_state
                    .selected_trace_details()
                    .map(|t| t.spans.len())
                    .unwrap_or(0);
                self.traces_state.select_next_span_page(max, 10);
            },
            AppEvent::PageUp
                if self.current_view == View::Traces && self.traces_state.show_detail =>
            {
                self.traces_state.select_previous_span_page(10);
            },
            AppEvent::PageDown if self.current_view == View::Traces => {
                self.traces_state.select_page_down(10);
            },
            AppEvent::PageUp if self.current_view == View::Traces => {
                self.traces_state.select_page_up(10);
            },
            AppEvent::PageDown if self.current_view == View::Metrics => {
                self.metrics_state.select_page_down(10);
            },
            AppEvent::PageUp if self.current_view == View::Metrics => {
                self.metrics_state.select_page_up(10);
            },
            // Vim-style j/k navigation. filter_input_active returns early above so these
            // only fire in normal mode, preserving j/k as text input inside filters.
            AppEvent::Char('j') if self.current_view == View::Logs => {
                self.logs_state.select_next();
            },
            AppEvent::Char('k') if self.current_view == View::Logs => {
                self.logs_state.select_previous();
            },
            AppEvent::Char('j')
                if self.current_view == View::Traces && !self.traces_state.show_span_detail =>
            {
                if self.traces_state.show_detail {
                    if let Some(trace) = self.traces_state.selected_trace_details() {
                        self.traces_state.select_next_span(trace.spans.len());
                    }
                } else {
                    self.traces_state.select_next();
                }
            },
            AppEvent::Char('k')
                if self.current_view == View::Traces && !self.traces_state.show_span_detail =>
            {
                if self.traces_state.show_detail {
                    if self.traces_state.selected_trace_details().is_some() {
                        self.traces_state.select_previous_span();
                    }
                } else {
                    self.traces_state.select_previous();
                }
            },
            AppEvent::Char('j') if self.current_view == View::Metrics => {
                self.metrics_state.select_next();
            },
            AppEvent::Char('k') if self.current_view == View::Metrics => {
                self.metrics_state.select_previous();
            },
            _ => {},
        }
    }

    /// Parse the filter input buffer and apply the filter to the current view's state.
    ///
    /// Format: `key=value` — splits on first `=`. If no `=`, treats the whole
    /// string as a body search query.  Supported keys:
    ///   Logs:   `severity`, `service` (matches resource attr `service.name`)
    ///   Traces: `service`, `error` (maps to `has_errors`)
    fn apply_filter_input(&mut self) {
        let input = self.filter_input_buffer.trim().to_string();
        if input.is_empty() {
            return;
        }

        if let Some(eq_pos) = input.find('=') {
            let key = input[..eq_pos].trim().to_string();
            let value = input[eq_pos + 1..].trim().to_string();

            // Normalise "error" -> "has_errors" for traces
            let (key, value) = match (self.current_view.clone(), key.as_str()) {
                (View::Traces, "error") => ("has_errors".to_string(), value),
                (View::Logs, "service") => ("service.name".to_string(), value),
                (View::Traces, "service") => ("service".to_string(), value),
                _ => (key, value),
            };

            match self.current_view {
                View::Logs => self.logs_state.set_filter(key, value),
                View::Traces => self.traces_state.set_filter(key, value),
                _ => {},
            }
        } else {
            // No `=`: treat as body/name search query
            match self.current_view {
                View::Logs => self.logs_state.set_search_query(input),
                View::Traces => self.traces_state.set_search_query(input),
                _ => {},
            }
        }
    }

    /// Check if the application should quit
    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    /// Get current view
    #[cfg(test)]
    pub fn current_view(&self) -> &View {
        &self.current_view
    }

    /// Render the current view
    pub fn render<B: ratatui::backend::Backend>(&self, terminal: &mut Terminal<B>) -> Result<()> {
        terminal.draw(|f| {
            let area = f.area();

            // Render based on current view
            match self.current_view {
                View::Logs => {
                    ui::render_logs_view(
                        f,
                        area,
                        &self.logs_state,
                        self.filter_input_active,
                        &self.filter_input_buffer,
                        self.api_error.as_deref(),
                    );
                },
                View::Traces => {
                    ui::render_traces_view(
                        f,
                        area,
                        &self.traces_state,
                        self.filter_input_active,
                        &self.filter_input_buffer,
                        self.api_error.as_deref(),
                    );
                },
                View::Metrics => {
                    ui::render_metrics_view(
                        f,
                        area,
                        &self.metrics_state,
                        self.api_error.as_deref(),
                    );
                },
                View::Help => {
                    ui::render_help_view(f, area, &self.config.version);
                },
            }
        })?;

        Ok(())
    }

    /// Refresh data from API if needed
    pub async fn refresh_if_needed(&mut self) -> Result<()> {
        // Handle pending trace detail load immediately (don't wait for refresh interval)
        if let Some(trace_id) = self.traces_state.pending_detail_load.take() {
            match self.api_client.fetch_trace_by_id(&trace_id).await {
                Ok(trace) => {
                    self.traces_state.set_trace_details(trace);
                    self.traces_state.clear_error();
                    self.api_error = None;
                },
                Err(e) => {
                    let msg = e.to_string();
                    self.traces_state
                        .set_error(format!("Failed to load trace: {}", msg));
                    self.api_error = Some(msg);
                },
            }
        }

        let elapsed = self.last_refresh.elapsed();
        if elapsed >= self.config.refresh_interval {
            self.refresh_data().await?;
            self.last_refresh = Instant::now();
        }
        Ok(())
    }

    /// Refresh data from API
    async fn refresh_data(&mut self) -> Result<()> {
        match self.current_view {
            View::Logs => {
                use crate::api::models::LogsQuery;
                let query = LogsQuery::default();
                match self.api_client.get_logs(&query).await {
                    Ok(response) => {
                        self.logs_state.update_logs(response.logs);
                        self.logs_state.clear_error();
                        self.api_error = None;
                    },
                    Err(e) => {
                        let msg = e.to_string();
                        self.logs_state
                            .set_error(format!("Failed to fetch logs: {}", msg));
                        self.api_error = Some(msg);
                    },
                }
            },
            View::Traces => {
                use crate::api::models::TracesQuery;
                let query = TracesQuery::default();
                match self.api_client.get_traces(&query).await {
                    Ok(response) => {
                        self.traces_state.update_traces(response.traces);
                        self.traces_state.clear_error();
                        self.api_error = None;
                    },
                    Err(e) => {
                        let msg = e.to_string();
                        self.traces_state
                            .set_error(format!("Failed to fetch traces: {}", msg));
                        self.api_error = Some(msg);
                    },
                }
            },
            View::Metrics => {
                // Note: Metrics API doesn't have query parameters in current implementation
                match self.api_client.fetch_metrics(vec![]).await {
                    Ok(response) => {
                        self.metrics_state.update_metrics(response);
                        self.metrics_state.clear_error();
                        self.api_error = None;
                    },
                    Err(e) => {
                        let msg = e.to_string();
                        self.metrics_state
                            .set_error(format!("Failed to fetch metrics: {}", msg));
                        self.api_error = Some(msg);
                    },
                }
            },
            View::Help => {
                // No refresh needed for help view
            },
        }
        Ok(())
    }
}

/// Run the TUI application
pub async fn run(config: Config) -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app
    let mut app = App::new(config);

    // Main event loop
    let result = run_event_loop(&mut app, &mut terminal).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

/// Run the main event loop
async fn run_event_loop<B: ratatui::backend::Backend>(
    app: &mut App,
    terminal: &mut Terminal<B>,
) -> Result<()> {
    // Initial data fetch
    app.refresh_data().await?;

    loop {
        // Render
        app.render(terminal)?;

        // Poll for events with timeout
        let event = poll_event(Duration::from_millis(100))?;

        // Handle event
        app.handle_event(event);

        // Refresh data if needed
        app.refresh_if_needed().await?;

        // Check if should quit
        if app.should_quit() {
            break;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_config() -> Config {
        Config {
            api_url: "http://localhost:8080".to_string(),
            refresh_interval: Duration::from_secs(5),
            initial_view: "logs".to_string(),
            debug: false,
            version: "0.0.0-test".to_string(),
        }
    }

    #[test]
    fn test_app_new_with_default_view() {
        let config = create_test_config();
        let app = App::new(config);

        assert_eq!(app.current_view, View::Logs);
        assert!(!app.should_quit);
    }

    #[test]
    fn test_app_new_with_traces_view() {
        let mut config = create_test_config();
        config.initial_view = "traces".to_string();
        let app = App::new(config);

        assert_eq!(app.current_view, View::Traces);
    }

    #[test]
    fn test_app_new_with_metrics_view() {
        let mut config = create_test_config();
        config.initial_view = "metrics".to_string();
        let app = App::new(config);

        assert_eq!(app.current_view, View::Metrics);
    }

    #[test]
    fn test_quit_event() {
        let config = create_test_config();
        let mut app = App::new(config);

        assert!(!app.should_quit());
        app.handle_event(AppEvent::Quit);
        assert!(app.should_quit());
    }

    #[test]
    fn test_view_switching() {
        let config = create_test_config();
        let mut app = App::new(config);

        assert_eq!(app.current_view(), &View::Logs);

        app.handle_event(AppEvent::SwitchToTraces);
        assert_eq!(app.current_view(), &View::Traces);

        app.handle_event(AppEvent::SwitchToMetrics);
        assert_eq!(app.current_view(), &View::Metrics);

        app.handle_event(AppEvent::SwitchToLogs);
        assert_eq!(app.current_view(), &View::Logs);
    }

    #[test]
    fn test_help_view_switching() {
        let config = create_test_config();
        let mut app = App::new(config);

        app.handle_event(AppEvent::ShowHelp);
        assert_eq!(app.current_view(), &View::Help);

        // Back from help should go to logs
        app.handle_event(AppEvent::Back);
        assert_eq!(app.current_view(), &View::Logs);
    }

    #[test]
    fn test_logs_view_navigation() {
        let config = create_test_config();
        let mut app = App::new(config);

        // Should be in logs view by default
        assert_eq!(app.current_view(), &View::Logs);

        // Navigation events should be handled
        app.handle_event(AppEvent::Down);
        app.handle_event(AppEvent::Up);
        app.handle_event(AppEvent::Select);

        // Should still be in logs view
        assert_eq!(app.current_view(), &View::Logs);
    }

    #[test]
    fn test_logs_view_auto_scroll_toggle() {
        let config = create_test_config();
        let mut app = App::new(config);

        let initial_auto_scroll = app.logs_state.auto_scroll;
        app.handle_event(AppEvent::ToggleAutoScroll);
        assert_ne!(app.logs_state.auto_scroll, initial_auto_scroll);
    }

    #[test]
    fn test_traces_view_navigation() {
        let config = create_test_config();
        let mut app = App::new(config);

        app.handle_event(AppEvent::SwitchToTraces);
        assert_eq!(app.current_view(), &View::Traces);

        // Navigation events should be handled
        app.handle_event(AppEvent::Down);
        app.handle_event(AppEvent::Up);
        app.handle_event(AppEvent::Select);
    }

    #[test]
    fn test_metrics_view_navigation() {
        let config = create_test_config();
        let mut app = App::new(config);

        app.handle_event(AppEvent::SwitchToMetrics);
        assert_eq!(app.current_view(), &View::Metrics);

        // Navigation events should be handled
        app.handle_event(AppEvent::Down);
        app.handle_event(AppEvent::Up);
        app.handle_event(AppEvent::Select);
    }

    #[test]
    fn test_back_closes_detail_panels() {
        let config = create_test_config();
        let mut app = App::new(config);

        // Open logs detail
        app.logs_state.show_detail_panel();
        assert!(app.logs_state.show_detail);

        // Back should close detail
        app.handle_event(AppEvent::Back);
        assert!(!app.logs_state.show_detail);
    }

    #[test]
    fn test_unhandled_events_in_wrong_view() {
        let config = create_test_config();
        let mut app = App::new(config);

        // Switch to metrics view
        app.handle_event(AppEvent::SwitchToMetrics);

        // Logs-specific event should be ignored
        app.handle_event(AppEvent::ToggleAutoScroll);

        // Should still be in metrics view
        assert_eq!(app.current_view(), &View::Metrics);
    }

    #[test]
    fn test_view_enum_equality() {
        assert_eq!(View::Logs, View::Logs);
        assert_ne!(View::Logs, View::Traces);
        assert_ne!(View::Traces, View::Metrics);
        assert_ne!(View::Metrics, View::Help);
    }

    #[test]
    fn test_view_enum_clone() {
        let view1 = View::Logs;
        let view2 = view1.clone();
        assert_eq!(view1, view2);
    }

    #[test]
    fn test_view_enum_debug() {
        let view = View::Logs;
        let debug_str = format!("{:?}", view);
        assert_eq!(debug_str, "Logs");
    }

    #[test]
    fn test_next_view_cycles_forward() {
        let config = create_test_config();
        let mut app = App::new(config);

        assert_eq!(app.current_view(), &View::Logs);
        app.handle_event(AppEvent::NextView);
        assert_eq!(app.current_view(), &View::Traces);
        app.handle_event(AppEvent::NextView);
        assert_eq!(app.current_view(), &View::Metrics);
        app.handle_event(AppEvent::NextView);
        assert_eq!(app.current_view(), &View::Logs);
    }

    #[test]
    fn test_prev_view_cycles_backward() {
        let config = create_test_config();
        let mut app = App::new(config);

        assert_eq!(app.current_view(), &View::Logs);
        app.handle_event(AppEvent::PrevView);
        assert_eq!(app.current_view(), &View::Metrics);
        app.handle_event(AppEvent::PrevView);
        assert_eq!(app.current_view(), &View::Traces);
        app.handle_event(AppEvent::PrevView);
        assert_eq!(app.current_view(), &View::Logs);
    }

    #[test]
    fn test_next_prev_view_ignored_when_filter_active() {
        let config = create_test_config();
        let mut app = App::new(config);

        app.filter_input_active = true;
        app.handle_event(AppEvent::NextView);
        assert_eq!(app.current_view(), &View::Logs);
        app.handle_event(AppEvent::PrevView);
        assert_eq!(app.current_view(), &View::Logs);
    }

    #[test]
    fn test_none_event_does_nothing() {
        let config = create_test_config();
        let mut app = App::new(config);

        let initial_view = app.current_view().clone();
        let initial_quit = app.should_quit();

        app.handle_event(AppEvent::None);

        assert_eq!(app.current_view(), &initial_view);
        assert_eq!(app.should_quit(), initial_quit);
    }
}
