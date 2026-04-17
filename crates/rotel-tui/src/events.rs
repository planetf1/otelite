use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use std::time::Duration;

/// Keyboard events that the application handles
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppEvent {
    /// Quit the application
    Quit,
    /// Switch to logs view
    SwitchToLogs,
    /// Switch to traces view
    SwitchToTraces,
    /// Switch to metrics view
    SwitchToMetrics,
    /// Show help screen
    ShowHelp,
    /// Navigate up
    Up,
    /// Navigate down
    Down,
    /// Navigate left
    Left,
    /// Navigate right
    Right,
    /// Select/expand item
    Select,
    /// Go back/cancel
    Back,
    /// Start search
    Search,
    /// Start filter
    Filter,
    /// Toggle auto-scroll
    ToggleAutoScroll,
    /// Highlight critical path (traces)
    HighlightCriticalPath,
    /// Zoom in (metrics)
    ZoomIn,
    /// Zoom out (metrics)
    ZoomOut,
    /// Refresh data
    Refresh,
    /// No event
    None,
}

/// Poll for keyboard events with timeout
pub fn poll_event(timeout: Duration) -> Result<AppEvent> {
    if event::poll(timeout)? {
        if let Event::Key(key) = event::read()? {
            return Ok(handle_key_event(key));
        }
    }
    Ok(AppEvent::None)
}

/// Convert keyboard event to application event
fn handle_key_event(key: KeyEvent) -> AppEvent {
    match key.code {
        // Quit
        KeyCode::Char('q') => AppEvent::Quit,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => AppEvent::Quit,

        // View switching
        KeyCode::Char('l') => AppEvent::SwitchToLogs,
        KeyCode::Char('t') => AppEvent::SwitchToTraces,
        KeyCode::Char('m') => AppEvent::SwitchToMetrics,

        // Help
        KeyCode::Char('?') | KeyCode::Char('h') => AppEvent::ShowHelp,

        // Navigation
        KeyCode::Up => AppEvent::Up,
        KeyCode::Down => AppEvent::Down,
        KeyCode::Left => AppEvent::Left,
        KeyCode::Right => AppEvent::Right,
        KeyCode::Enter => AppEvent::Select,
        KeyCode::Esc => AppEvent::Back,

        // Actions
        KeyCode::Char('/') => AppEvent::Search,
        KeyCode::Char('f') => AppEvent::Filter,
        KeyCode::Char('s') => AppEvent::ToggleAutoScroll,
        KeyCode::Char('c') => AppEvent::HighlightCriticalPath,
        KeyCode::Char('+') | KeyCode::Char('=') => AppEvent::ZoomIn,
        KeyCode::Char('-') => AppEvent::ZoomOut,
        KeyCode::Char('r') => AppEvent::Refresh,

        _ => AppEvent::None,
    }
}

// Made with Bob
