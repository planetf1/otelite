// Traces state implementation - waiting for UI integration
#![allow(dead_code)]

use crate::api::models::{Trace, TraceSummary};
use std::collections::HashMap;

use super::{
    PaginatedList, ResponseCache, StateManager, UpdateTracker, MAX_ITEMS_IN_MEMORY,
    MIN_REFRESH_INTERVAL,
};
use std::time::Duration;

/// State management for the traces view
#[derive(Debug, Clone)]
pub struct TracesState {
    /// All trace summaries fetched from the API (with pagination)
    traces: PaginatedList<TraceSummary>,
    /// Currently selected trace index
    pub selected_index: usize,
    /// Full trace details (loaded on demand with caching)
    trace_details: HashMap<String, ResponseCache<Trace>>,
    /// Whether detail panel is shown
    pub show_detail: bool,
    /// Trace ID that needs full details fetched from API (None when not needed)
    pub pending_detail_load: Option<String>,
    /// Selected span index within the trace detail view
    pub selected_span_index: usize,
    /// Scroll offset for span list in detail view
    pub span_scroll_offset: usize,
    /// Whether to show span detail (nested detail within trace detail)
    pub show_span_detail: bool,
    /// Search query
    pub search_query: String,
    /// Active filters (field -> value)
    pub filters: HashMap<String, String>,
    /// Scroll offset for the traces table (will be used when UI implements scrolling)
    pub scroll_offset: usize,
    /// Last error message
    pub error: Option<String>,
    /// Update tracker for debouncing
    update_tracker: UpdateTracker,
}

impl Default for TracesState {
    fn default() -> Self {
        Self {
            traces: PaginatedList::new(MAX_ITEMS_IN_MEMORY),
            selected_index: 0,
            trace_details: HashMap::new(),
            show_detail: false,
            pending_detail_load: None,
            selected_span_index: 0,
            span_scroll_offset: 0,
            show_span_detail: false,
            search_query: String::new(),
            filters: HashMap::new(),
            scroll_offset: 0,
            error: None,
            update_tracker: UpdateTracker::new(MIN_REFRESH_INTERVAL),
        }
    }
}

impl TracesState {
    /// Create a new traces state
    pub fn new() -> Self {
        Self::default()
    }

    /// Update traces from API response (with debouncing)
    pub fn update_traces(&mut self, new_traces: Vec<TraceSummary>) {
        // Check if we should update (debouncing)
        if !self.update_tracker.should_update() {
            return;
        }

        self.traces.replace(new_traces);
        self.update_tracker.mark_updated();

        // Ensure selected index is valid
        if self.selected_index >= self.traces.len() && !self.traces.is_empty() {
            self.selected_index = self.traces.len() - 1;
        }
    }

    /// Get all traces (will be used by UI components)
    pub fn traces(&self) -> &[TraceSummary] {
        self.traces.items()
    }

    /// Store trace details (with caching)
    pub fn store_trace_details(&mut self, trace_id: String, trace: Trace) {
        let mut cache = ResponseCache::new(Duration::from_secs(300)); // 5 minute cache
        cache.set(trace);
        self.trace_details.insert(trace_id, cache);
    }

    /// Get trace details (from cache if available)
    pub fn get_trace_details(&self, trace_id: &str) -> Option<&Trace> {
        self.trace_details
            .get(trace_id)
            .and_then(|cache| cache.get())
    }

    /// Check if trace details are cached
    pub fn has_cached_trace(&self, trace_id: &str) -> bool {
        self.trace_details
            .get(trace_id)
            .is_some_and(|cache| cache.is_valid())
    }

    /// Check if update is needed (for debouncing)
    pub fn should_update(&self) -> bool {
        self.update_tracker.should_update()
    }

    /// Get filtered traces based on search query and filters
    pub fn filtered_traces(&self) -> Vec<&TraceSummary> {
        self.traces
            .items()
            .iter()
            .filter(|trace| {
                // Apply search query
                if !self.search_query.is_empty() {
                    let query = self.search_query.to_lowercase();
                    let matches = trace.trace_id.to_lowercase().contains(&query)
                        || trace.root_span_name.to_lowercase().contains(&query)
                        || trace
                            .service_names
                            .iter()
                            .any(|s: &String| s.to_lowercase().contains(&query));
                    if !matches {
                        return false;
                    }
                }

                // Apply filters
                for (field, value) in &self.filters {
                    match field.as_str() {
                        "has_errors" => {
                            let filter_value = value.eq_ignore_ascii_case("true");
                            if trace.has_errors != filter_value {
                                return false;
                            }
                        },
                        "service"
                            if !trace
                                .service_names
                                .iter()
                                .any(|s: &String| s.eq_ignore_ascii_case(value.as_str())) =>
                        {
                            return false;
                        },
                        _ => {},
                    }
                }

                true
            })
            .collect()
    }

    /// Get currently selected trace
    pub fn selected_trace(&self) -> Option<&TraceSummary> {
        let filtered = self.filtered_traces();
        filtered.get(self.selected_index).copied()
    }

    /// Get trace details for the selected trace
    pub fn selected_trace_details(&self) -> Option<&Trace> {
        self.selected_trace()
            .and_then(|summary| self.get_trace_details(&summary.trace_id))
    }

    /// Store trace details (legacy method for compatibility)
    pub fn set_trace_details(&mut self, trace: Trace) {
        self.store_trace_details(trace.trace_id.clone(), trace);
    }

    /// Move selection up
    pub fn select_previous(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    /// Move selection down
    pub fn select_next(&mut self) {
        let filtered_count = self.filtered_traces().len();
        if filtered_count > 0 && self.selected_index < filtered_count - 1 {
            self.selected_index += 1;
        }
    }

    /// Move selection up by `n` items (page up)
    pub fn select_page_up(&mut self, n: usize) {
        self.selected_index = self.selected_index.saturating_sub(n);
    }

    /// Move selection down by `n` items (page down)
    pub fn select_page_down(&mut self, n: usize) {
        let filtered_count = self.filtered_traces().len();
        if filtered_count > 0 {
            self.selected_index = (self.selected_index + n).min(filtered_count - 1);
        }
    }

    /// Toggle detail panel
    pub fn toggle_detail(&mut self) {
        self.show_detail = !self.show_detail;
    }

    /// Show detail panel and trigger API load if details not cached
    pub fn show_detail_panel(&mut self) {
        self.show_detail = true;
        // If we don't have cached details, request a load
        if let Some(summary) = self.selected_trace() {
            if !self.has_cached_trace(&summary.trace_id) {
                self.pending_detail_load = Some(summary.trace_id.clone());
            }
        }
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

    /// Set error message
    pub fn set_error(&mut self, error: String) {
        self.error = Some(error);
    }

    /// Clear error message
    pub fn clear_error(&mut self) {
        self.error = None;
    }

    /// Move span selection up
    pub fn select_previous_span(&mut self) {
        if self.selected_span_index > 0 {
            self.selected_span_index -= 1;
        }
    }

    /// Move span selection down
    pub fn select_next_span(&mut self, max_spans: usize) {
        if max_spans > 0 && self.selected_span_index < max_spans - 1 {
            self.selected_span_index += 1;
        }
    }

    /// Toggle span detail panel
    pub fn toggle_span_detail(&mut self) {
        self.show_span_detail = !self.show_span_detail;
    }

    /// Reset span selection when switching traces
    pub fn reset_span_selection(&mut self) {
        self.selected_span_index = 0;
        self.span_scroll_offset = 0;
        self.show_span_detail = false;
    }
}

impl StateManager for TracesState {
    fn apply_pagination(&mut self) {
        // Pagination is automatically handled by PaginatedList
    }

    fn cleanup_old_data(&mut self) {
        // Clean up old cached trace details
        self.trace_details.retain(|_, cache| cache.is_valid());
    }

    fn item_count(&self) -> usize {
        self.traces.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_trace(trace_id: &str, name: &str, has_errors: bool) -> TraceSummary {
        TraceSummary {
            trace_id: trace_id.to_string(),
            root_span_name: name.to_string(),
            start_time: 1713360000000,
            duration: 1000000,
            span_count: 5,
            has_errors,
            service_names: vec!["test-service".to_string()],
        }
    }

    #[test]
    fn test_traces_state_default() {
        let state = TracesState::default();
        assert_eq!(state.traces.len(), 0);
        assert_eq!(state.selected_index, 0);
        assert!(!state.show_detail);
    }

    #[test]
    fn test_update_traces() {
        let mut state = TracesState::new();
        let traces = vec![
            create_test_trace("trace1", "GET /api/users", false),
            create_test_trace("trace2", "POST /api/orders", true),
        ];

        state.update_traces(traces);
        assert_eq!(state.traces.len(), 2);
    }

    #[test]
    fn test_navigation() {
        let mut state = TracesState::new();
        let traces = vec![
            create_test_trace("trace1", "GET /api/users", false),
            create_test_trace("trace2", "POST /api/orders", true),
            create_test_trace("trace3", "GET /api/products", false),
        ];
        state.update_traces(traces);

        state.selected_index = 1;
        state.select_next();
        assert_eq!(state.selected_index, 2);

        state.select_previous();
        assert_eq!(state.selected_index, 1);
    }

    #[test]
    fn test_search_filtering() {
        let mut state = TracesState::new();
        let traces = vec![
            create_test_trace("trace1", "GET /api/users", false),
            create_test_trace("trace2", "POST /api/orders", true),
            create_test_trace("trace3", "GET /api/users", false),
        ];
        state.update_traces(traces);

        state.set_search_query("users".to_string());
        let filtered = state.filtered_traces();
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn test_error_filtering() {
        let mut state = TracesState::new();
        let traces = vec![
            create_test_trace("trace1", "GET /api/users", false),
            create_test_trace("trace2", "POST /api/orders", true),
            create_test_trace("trace3", "GET /api/products", false),
        ];
        state.update_traces(traces);

        state.set_filter("has_errors".to_string(), "true".to_string());
        let filtered = state.filtered_traces();
        assert_eq!(filtered.len(), 1);
        assert!(filtered[0].has_errors);
    }
}
