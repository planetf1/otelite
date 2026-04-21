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
    /// Page down (move selection by page)
    PageDown,
    /// Page up (move selection by page)
    PageUp,
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
        KeyCode::PageDown => AppEvent::PageDown,
        KeyCode::PageUp => AppEvent::PageUp,

        _ => AppEvent::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quit_events() {
        let key = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);
        assert_eq!(handle_key_event(key), AppEvent::Quit);

        let key = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(handle_key_event(key), AppEvent::Quit);
    }

    #[test]
    fn test_view_switching_events() {
        let key = KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE);
        assert_eq!(handle_key_event(key), AppEvent::SwitchToLogs);

        let key = KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE);
        assert_eq!(handle_key_event(key), AppEvent::SwitchToTraces);

        let key = KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE);
        assert_eq!(handle_key_event(key), AppEvent::SwitchToMetrics);
    }

    #[test]
    fn test_help_events() {
        let key = KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE);
        assert_eq!(handle_key_event(key), AppEvent::ShowHelp);

        let key = KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE);
        assert_eq!(handle_key_event(key), AppEvent::ShowHelp);
    }

    #[test]
    fn test_navigation_events() {
        let key = KeyEvent::new(KeyCode::Up, KeyModifiers::NONE);
        assert_eq!(handle_key_event(key), AppEvent::Up);

        let key = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        assert_eq!(handle_key_event(key), AppEvent::Down);

        let key = KeyEvent::new(KeyCode::Left, KeyModifiers::NONE);
        assert_eq!(handle_key_event(key), AppEvent::Left);

        let key = KeyEvent::new(KeyCode::Right, KeyModifiers::NONE);
        assert_eq!(handle_key_event(key), AppEvent::Right);

        let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(handle_key_event(key), AppEvent::Select);

        let key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(handle_key_event(key), AppEvent::Back);
    }

    #[test]
    fn test_action_events() {
        let key = KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE);
        assert_eq!(handle_key_event(key), AppEvent::Search);

        let key = KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE);
        assert_eq!(handle_key_event(key), AppEvent::Filter);

        let key = KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE);
        assert_eq!(handle_key_event(key), AppEvent::ToggleAutoScroll);

        let key = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE);
        assert_eq!(handle_key_event(key), AppEvent::HighlightCriticalPath);

        let key = KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE);
        assert_eq!(handle_key_event(key), AppEvent::Refresh);
    }

    #[test]
    fn test_zoom_events() {
        let key = KeyEvent::new(KeyCode::Char('+'), KeyModifiers::NONE);
        assert_eq!(handle_key_event(key), AppEvent::ZoomIn);

        let key = KeyEvent::new(KeyCode::Char('='), KeyModifiers::NONE);
        assert_eq!(handle_key_event(key), AppEvent::ZoomIn);

        let key = KeyEvent::new(KeyCode::Char('-'), KeyModifiers::NONE);
        assert_eq!(handle_key_event(key), AppEvent::ZoomOut);
    }

    #[test]
    fn test_page_navigation_events() {
        let key = KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE);
        assert_eq!(handle_key_event(key), AppEvent::PageDown);

        let key = KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE);
        assert_eq!(handle_key_event(key), AppEvent::PageUp);
    }

    #[test]
    fn test_unknown_key_returns_none() {
        let key = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE);
        assert_eq!(handle_key_event(key), AppEvent::None);

        let key = KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE);
        assert_eq!(handle_key_event(key), AppEvent::None);
    }

    #[test]
    fn test_ctrl_c_with_other_modifiers() {
        // Ctrl+C should work even with other modifiers
        let key = KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        );
        assert_eq!(handle_key_event(key), AppEvent::Quit);
    }

    #[test]
    fn test_app_event_clone_and_eq() {
        let event1 = AppEvent::Quit;
        let event2 = event1.clone();
        assert_eq!(event1, event2);

        let event3 = AppEvent::SwitchToLogs;
        assert_ne!(event1, event3);
    }

    #[test]
    fn test_app_event_debug() {
        let event = AppEvent::Quit;
        let debug_str = format!("{:?}", event);
        assert_eq!(debug_str, "Quit");
    }
}
