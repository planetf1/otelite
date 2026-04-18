#![allow(dead_code)]

use crate::api::models::LogEntry;
use std::collections::HashMap;

use super::{PaginatedList, UpdateTracker, MAX_ITEMS_IN_MEMORY, MIN_REFRESH_INTERVAL};

/// State management for the logs view
#[derive(Debug, Clone)]
pub struct LogsState {
    /// All logs fetched from the API (with pagination)
    logs: PaginatedList<LogEntry>,
    /// Currently selected log index
    pub selected_index: usize,
    /// Whether detail panel is shown
    pub show_detail: bool,
    /// Search query
    pub search_query: String,
    /// Active filters (field -> value)
    pub filters: HashMap<String, String>,
    /// Whether auto-scroll is enabled
    pub auto_scroll: bool,
    /// Scroll offset for the logs table
    #[allow(dead_code)]
    pub scroll_offset: usize,
    /// Last error message
    pub error: Option<String>,
    /// Update tracker for debouncing
    update_tracker: UpdateTracker,
}

impl Default for LogsState {
    fn default() -> Self {
        Self {
            logs: PaginatedList::new(MAX_ITEMS_IN_MEMORY),
            selected_index: 0,
            show_detail: false,
            search_query: String::new(),
            filters: HashMap::new(),
            auto_scroll: true,
            scroll_offset: 0,
            error: None,
            update_tracker: UpdateTracker::new(MIN_REFRESH_INTERVAL),
        }
    }
}

impl LogsState {
    /// Create a new logs state
    pub fn new() -> Self {
        Self::default()
    }

    /// Update logs from API response (with debouncing)
    pub fn update_logs(&mut self, new_logs: Vec<LogEntry>) {
        // Check if we should update (debouncing)
        if !self.update_tracker.should_update() {
            return;
        }

        self.logs.replace(new_logs);
        self.update_tracker.mark_updated();

        // Auto-scroll to bottom if enabled
        if self.auto_scroll && !self.logs.is_empty() {
            self.selected_index = self.logs.len() - 1;
        }

        // Ensure selected index is valid
        if self.selected_index >= self.logs.len() && !self.logs.is_empty() {
            self.selected_index = self.logs.len() - 1;
        }
    }

    /// Get filtered logs based on search query and filters
    pub fn filtered_logs(&self) -> Vec<&LogEntry> {
        self.logs
            .items()
            .iter()
            .filter(|log| {
                // Apply search query
                if !self.search_query.is_empty() {
                    let query = self.search_query.to_lowercase();
                    let matches = log.body.to_lowercase().contains(&query)
                        || log.severity.to_lowercase().contains(&query)
                        || log
                            .attributes
                            .values()
                            .any(|v: &String| v.to_lowercase().contains(&query));
                    if !matches {
                        return false;
                    }
                }

                // Apply filters
                for (field, value) in &self.filters {
                    match field.as_str() {
                        "severity" => {
                            if !log.severity.eq_ignore_ascii_case(value) {
                                return false;
                            }
                        },
                        _ => {
                            if let Some(attr_value) = log.attributes.get(field) {
                                if !attr_value.eq_ignore_ascii_case(value.as_str()) {
                                    return false;
                                }
                            } else {
                                return false;
                            }
                        },
                    }
                }

                true
            })
            .collect()
    }

    /// Get currently selected log
    pub fn selected_log(&self) -> Option<&LogEntry> {
        let filtered = self.filtered_logs();
        filtered.get(self.selected_index).copied()
    }

    /// Move selection up
    pub fn select_previous(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
            self.auto_scroll = false;
        }
    }

    /// Move selection down
    pub fn select_next(&mut self) {
        let filtered_count = self.filtered_logs().len();
        if filtered_count > 0 && self.selected_index < filtered_count - 1 {
            self.selected_index += 1;
            self.auto_scroll = false;
        }
    }

    #[allow(dead_code)]
    /// Toggle detail panel
    pub fn toggle_detail(&mut self) {
        self.show_detail = !self.show_detail;
    }

    /// Show detail panel
    pub fn show_detail_panel(&mut self) {
        self.show_detail = true;
    }

    /// Hide detail panel
    pub fn hide_detail_panel(&mut self) {
        self.show_detail = false;
    }

    /// Set search query
    pub fn set_search_query(&mut self, query: String) {
        self.search_query = query;
        self.selected_index = 0;
    }

    /// Clear search query
    pub fn clear_search(&mut self) {
        self.search_query.clear();
        self.selected_index = 0;
    }

    /// Add or update a filter
    pub fn set_filter(&mut self, field: String, value: String) {
        self.filters.insert(field, value);
        self.selected_index = 0;
    }

    /// Remove a filter
    pub fn remove_filter(&mut self, field: &str) {
        self.filters.remove(field);
        self.selected_index = 0;
    }

    /// Clear all filters
    pub fn clear_filters(&mut self) {
        self.filters.clear();
        self.selected_index = 0;
    }

    /// Toggle auto-scroll
    pub fn toggle_auto_scroll(&mut self) {
        self.auto_scroll = !self.auto_scroll;
        if self.auto_scroll && !self.logs.is_empty() {
            self.selected_index = self.logs.len() - 1;
        }
    }

    /// Set error message
    pub fn set_error(&mut self, error: String) {
        self.error = Some(error);
    }

    /// Clear error message
    pub fn clear_error(&mut self) {
        self.error = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::models::Resource;
    use crate::state::StateManager;

    fn create_test_log(body: &str, severity: &str) -> LogEntry {
        LogEntry {
            id: "test-id".to_string(),
            timestamp: 1713360000000, // 2026-04-17T12:00:00Z in milliseconds
            severity: severity.to_string(),
            body: body.to_string(),
            attributes: HashMap::new(),
            resource: Resource {
                attributes: HashMap::new(),
            },
        }
    }
    impl StateManager for LogsState {
        fn apply_pagination(&mut self) {
            // Pagination is automatically handled by PaginatedList
            // This method is here for trait compliance
        }

        fn cleanup_old_data(&mut self) {
            // For logs, we keep the most recent items based on pagination
            // No time-based cleanup needed as PaginatedList handles it
        }

        fn item_count(&self) -> usize {
            self.logs.len()
        }
    }

    #[test]
    fn test_logs_state_default() {
        let state = LogsState::default();
        assert_eq!(state.logs.len(), 0);
        assert_eq!(state.selected_index, 0);
        assert!(!state.show_detail);
        assert!(state.auto_scroll);
    }

    #[test]
    fn test_update_logs() {
        let mut state = LogsState::new();
        let logs = vec![
            create_test_log("Log 1", "INFO"),
            create_test_log("Log 2", "ERROR"),
        ];

        state.update_logs(logs);
        assert_eq!(state.logs.len(), 2);
        assert_eq!(state.selected_index, 1); // Auto-scrolled to bottom
    }

    #[test]
    fn test_navigation() {
        let mut state = LogsState::new();
        let logs = vec![
            create_test_log("Log 1", "INFO"),
            create_test_log("Log 2", "ERROR"),
            create_test_log("Log 3", "WARN"),
        ];
        state.update_logs(logs);

        state.selected_index = 1;
        state.select_next();
        assert_eq!(state.selected_index, 2);

        state.select_previous();
        assert_eq!(state.selected_index, 1);
    }

    #[test]
    fn test_search_filtering() {
        let mut state = LogsState::new();
        let logs = vec![
            create_test_log("User logged in", "INFO"),
            create_test_log("Database error", "ERROR"),
            create_test_log("User logged out", "INFO"),
        ];
        state.update_logs(logs);

        state.set_search_query("user".to_string());
        let filtered = state.filtered_logs();
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn test_severity_filtering() {
        let mut state = LogsState::new();
        let logs = vec![
            create_test_log("Log 1", "INFO"),
            create_test_log("Log 2", "ERROR"),
            create_test_log("Log 3", "INFO"),
        ];
        state.update_logs(logs);

        state.set_filter("severity".to_string(), "ERROR".to_string());
        let filtered = state.filtered_logs();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].severity, "ERROR");
    }

    #[test]
    fn test_auto_scroll_toggle() {
        let mut state = LogsState::new();
        assert!(state.auto_scroll);

        state.toggle_auto_scroll();
        assert!(!state.auto_scroll);

        state.toggle_auto_scroll();
        assert!(state.auto_scroll);
    }
}

// Made with Bob
