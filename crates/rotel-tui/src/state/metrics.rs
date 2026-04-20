// Metrics state implementation - waiting for UI integration
#![allow(dead_code)]

use crate::api::models::Metric;
use std::collections::HashMap;

use super::{
    PaginatedList, StateManager, UpdateTracker, MAX_ITEMS_IN_MEMORY, MIN_REFRESH_INTERVAL,
};

/// State management for the metrics view
#[derive(Debug, Clone)]
pub struct MetricsState {
    /// All metrics fetched from the API (with pagination)
    metrics: PaginatedList<Metric>,
    /// Currently selected metric index
    pub selected_index: usize,
    /// Whether detail panel is shown
    pub show_detail: bool,
    /// Search query
    pub search_query: String,
    /// Active filters (field -> value)
    pub filters: HashMap<String, String>,
    /// Scroll offset for the metrics table (will be used when UI implements scrolling)
    pub scroll_offset: usize,
    /// Last error message
    pub error: Option<String>,
    /// Update tracker for debouncing
    update_tracker: UpdateTracker,
}

impl Default for MetricsState {
    fn default() -> Self {
        Self {
            metrics: PaginatedList::new(MAX_ITEMS_IN_MEMORY),
            selected_index: 0,
            show_detail: false,
            search_query: String::new(),
            filters: HashMap::new(),
            scroll_offset: 0,
            error: None,
            update_tracker: UpdateTracker::new(MIN_REFRESH_INTERVAL),
        }
    }
}

impl MetricsState {
    /// Create a new metrics state
    pub fn new() -> Self {
        Self::default()
    }

    /// Update metrics from API response (with debouncing)
    pub fn update_metrics(&mut self, new_metrics: Vec<Metric>) {
        // Check if we should update (debouncing)
        if !self.update_tracker.should_update() {
            return;
        }

        self.metrics.replace(new_metrics);
        self.update_tracker.mark_updated();

        // Ensure selected index is valid
        if self.selected_index >= self.metrics.len() && !self.metrics.is_empty() {
            self.selected_index = self.metrics.len() - 1;
        }
    }

    /// Get all metrics (will be used by UI components)
    pub fn metrics(&self) -> &[Metric] {
        self.metrics.items()
    }

    /// Check if update is needed (for debouncing)
    pub fn should_update(&self) -> bool {
        self.update_tracker.should_update()
    }

    /// Get filtered metrics based on search query and filters
    pub fn filtered_metrics(&self) -> Vec<&Metric> {
        self.metrics
            .items()
            .iter()
            .filter(|metric| {
                // Apply search query
                if !self.search_query.is_empty() {
                    let query = self.search_query.to_lowercase();
                    let matches = metric.name.to_lowercase().contains(&query)
                        || metric
                            .description
                            .as_ref()
                            .is_some_and(|d: &String| d.to_lowercase().contains(&query))
                        || metric
                            .unit
                            .as_ref()
                            .is_some_and(|u: &String| u.to_lowercase().contains(&query));
                    if !matches {
                        return false;
                    }
                }

                // Apply filters
                for (field, value) in &self.filters {
                    match field.as_str() {
                        "type" if !metric.metric_type.eq_ignore_ascii_case(value) => {
                            return false;
                        },
                        "unit" => {
                            if let Some(unit) = &metric.unit {
                                if !unit.eq_ignore_ascii_case(value.as_str()) {
                                    return false;
                                }
                            } else {
                                return false;
                            }
                        },
                        _ => {},
                    }
                }

                true
            })
            .collect()
    }

    /// Get currently selected metric
    pub fn selected_metric(&self) -> Option<&Metric> {
        let filtered = self.filtered_metrics();
        filtered.get(self.selected_index).copied()
    }

    /// Move selection up
    pub fn select_previous(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    /// Move selection down
    pub fn select_next(&mut self) {
        let filtered_count = self.filtered_metrics().len();
        if filtered_count > 0 && self.selected_index < filtered_count - 1 {
            self.selected_index += 1;
        }
    }

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

    /// Set error message
    pub fn set_error(&mut self, error: String) {
        self.error = Some(error);
    }

    /// Clear error message
    pub fn clear_error(&mut self) {
        self.error = None;
    }
}
impl StateManager for MetricsState {
    fn apply_pagination(&mut self) {
        // Pagination is automatically handled by PaginatedList
    }

    fn cleanup_old_data(&mut self) {
        // For metrics, we keep the most recent items based on pagination
        // No time-based cleanup needed as PaginatedList handles it
    }

    fn item_count(&self) -> usize {
        self.metrics.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::models::MetricValue;

    fn create_test_metric(name: &str, metric_type: &str, unit: Option<&str>) -> Metric {
        Metric {
            name: name.to_string(),
            description: Some(format!("Test metric: {}", name)),
            unit: unit.map(|u| u.to_string()),
            metric_type: metric_type.to_string(),
            value: MetricValue::Gauge(42.0),
            timestamp: 1713360000000000000,
            attributes: HashMap::new(),
            resource: None,
        }
    }

    #[test]
    fn test_metrics_state_default() {
        let state = MetricsState::default();
        assert_eq!(state.metrics.len(), 0);
        assert_eq!(state.selected_index, 0);
        assert!(!state.show_detail);
    }

    #[test]
    fn test_update_metrics() {
        let mut state = MetricsState::new();
        let metrics = vec![
            create_test_metric("cpu.usage", "gauge", Some("percent")),
            create_test_metric("memory.used", "gauge", Some("bytes")),
        ];

        state.update_metrics(metrics);
        assert_eq!(state.metrics.len(), 2);
    }

    #[test]
    fn test_navigation() {
        let mut state = MetricsState::new();
        let metrics = vec![
            create_test_metric("cpu.usage", "gauge", Some("percent")),
            create_test_metric("memory.used", "gauge", Some("bytes")),
            create_test_metric("disk.io", "counter", Some("operations")),
        ];
        state.update_metrics(metrics);

        state.selected_index = 1;
        state.select_next();
        assert_eq!(state.selected_index, 2);

        state.select_previous();
        assert_eq!(state.selected_index, 1);
    }

    #[test]
    fn test_search_filtering() {
        let mut state = MetricsState::new();
        let metrics = vec![
            create_test_metric("cpu.usage", "gauge", Some("percent")),
            create_test_metric("memory.used", "gauge", Some("bytes")),
            create_test_metric("cpu.temperature", "gauge", Some("celsius")),
        ];
        state.update_metrics(metrics);

        state.set_search_query("cpu".to_string());
        let filtered = state.filtered_metrics();
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn test_type_filtering() {
        let mut state = MetricsState::new();
        let metrics = vec![
            create_test_metric("cpu.usage", "gauge", Some("percent")),
            create_test_metric("requests.total", "counter", Some("count")),
            create_test_metric("memory.used", "gauge", Some("bytes")),
        ];
        state.update_metrics(metrics);

        state.set_filter("type".to_string(), "gauge".to_string());
        let filtered = state.filtered_metrics();
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn test_unit_filtering() {
        let mut state = MetricsState::new();
        let metrics = vec![
            create_test_metric("cpu.usage", "gauge", Some("percent")),
            create_test_metric("memory.usage", "gauge", Some("percent")),
            create_test_metric("memory.used", "gauge", Some("bytes")),
        ];
        state.update_metrics(metrics);

        state.set_filter("unit".to_string(), "percent".to_string());
        let filtered = state.filtered_metrics();
        assert_eq!(filtered.len(), 2);
    }
}
