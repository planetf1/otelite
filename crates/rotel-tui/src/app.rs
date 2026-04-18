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
}

impl App {
    /// Create a new application instance
    pub fn new(config: Config) -> Self {
        let current_view = match config.initial_view.as_str() {
            "traces" => View::Traces,
            "metrics" => View::Metrics,
            _ => View::Logs,
        };

        let api_client = ApiClient::new(config.api_url.clone());

        Self {
            config,
            current_view,
            should_quit: false,
            api_client,
            logs_state: LogsState::new(),
            traces_state: TracesState::new(),
            metrics_state: MetricsState::new(),
            last_refresh: Instant::now(),
        }
    }

    /// Handle an application event
    pub fn handle_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::Quit => self.should_quit = true,
            AppEvent::SwitchToLogs => self.current_view = View::Logs,
            AppEvent::SwitchToTraces => self.current_view = View::Traces,
            AppEvent::SwitchToMetrics => self.current_view = View::Metrics,
            AppEvent::ShowHelp => self.current_view = View::Help,
            AppEvent::Back => {
                if self.current_view == View::Help {
                    self.current_view = View::Logs;
                } else {
                    // Close detail panels in each view
                    match self.current_view {
                        View::Logs if self.logs_state.show_detail => {
                            self.logs_state.hide_detail_panel();
                        },
                        View::Traces if self.traces_state.show_detail => {
                            self.traces_state.hide_detail_panel();
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
                self.traces_state.select_previous();
            },
            AppEvent::Down if self.current_view == View::Traces => {
                self.traces_state.select_next();
            },
            AppEvent::Select if self.current_view == View::Traces => {
                self.traces_state.show_detail_panel();
                // TODO: Load full trace details from API
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
            _ => {},
        }
    }

    /// Check if the application should quit
    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    /// Render the current view
    pub fn render<B: ratatui::backend::Backend>(&self, terminal: &mut Terminal<B>) -> Result<()> {
        terminal.draw(|f| {
            let area = f.size();

            // Render based on current view
            match self.current_view {
                View::Logs => {
                    ui::render_logs_view(f, area, &self.logs_state);
                },
                View::Traces => {
                    ui::render_traces_view(f, area, &self.traces_state);
                },
                View::Metrics => {
                    ui::render_metrics_view(f, area, &self.metrics_state);
                },
                View::Help => {
                    ui::render_help_view(f, area);
                },
            }
        })?;

        Ok(())
    }

    /// Refresh data from API if needed
    pub async fn refresh_if_needed(&mut self) -> Result<()> {
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
                    },
                    Err(e) => {
                        self.logs_state
                            .set_error(format!("Failed to fetch logs: {}", e));
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
                    },
                    Err(e) => {
                        self.traces_state
                            .set_error(format!("Failed to fetch traces: {}", e));
                    },
                }
            },
            View::Metrics => {
                // Note: Metrics API doesn't have query parameters in current implementation
                match self.api_client.get_metrics().await {
                    Ok(response) => {
                        self.metrics_state.update_metrics(response.metrics);
                        self.metrics_state.clear_error();
                    },
                    Err(e) => {
                        self.metrics_state
                            .set_error(format!("Failed to fetch metrics: {}", e));
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

// Made with Bob
