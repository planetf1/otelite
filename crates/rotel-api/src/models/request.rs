//! Request parameter models

use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use validator::Validate;

/// Common query parameters for list endpoints
#[derive(Debug, Clone, Deserialize, Serialize, Validate, IntoParams)]
pub struct ListQueryParams {
    /// Maximum number of items to return (default: 100, max: 1000)
    #[validate(range(min = 1, max = 1000))]
    #[serde(default = "default_limit")]
    pub limit: usize,

    /// Offset for pagination (default: 0)
    #[serde(default)]
    pub offset: usize,
}

fn default_limit() -> usize {
    100
}

impl Default for ListQueryParams {
    fn default() -> Self {
        Self {
            limit: default_limit(),
            offset: 0,
        }
    }
}

/// Time range filter for queries
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct TimeRange {
    /// Start time (Unix timestamp in milliseconds)
    pub start: Option<i64>,

    /// End time (Unix timestamp in milliseconds)
    pub end: Option<i64>,

    /// Relative time range (e.g., "1h", "30m", "7d")
    /// Supported units: s (seconds), m (minutes), h (hours), d (days)
    pub since: Option<String>,
}

impl TimeRange {
    /// Parse relative time string to milliseconds
    ///
    /// Examples: "1h" = 3600000ms, "30m" = 1800000ms, "7d" = 604800000ms
    pub fn parse_since(&self) -> Option<i64> {
        let since = self.since.as_ref()?;
        let (num_str, unit) = since.split_at(since.len() - 1);
        let num: i64 = num_str.parse().ok()?;

        let multiplier = match unit {
            "s" => 1_000,      // seconds to ms
            "m" => 60_000,     // minutes to ms
            "h" => 3_600_000,  // hours to ms
            "d" => 86_400_000, // days to ms
            _ => return None,
        };

        Some(num * multiplier)
    }

    /// Get the effective start time in milliseconds
    pub fn effective_start(&self) -> Option<i64> {
        if let Some(start) = self.start {
            Some(start)
        } else if let Some(since_ms) = self.parse_since() {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .ok()?
                .as_millis() as i64;
            Some(now - since_ms)
        } else {
            None
        }
    }
}

/// Log query parameters
#[derive(Debug, Clone, Deserialize, Serialize, Validate, IntoParams, ToSchema)]
pub struct LogQueryParams {
    /// Maximum number of logs to return (default: 100, max: 1000)
    #[validate(range(min = 1, max = 1000))]
    #[serde(default = "default_limit")]
    pub limit: usize,

    /// Offset for pagination (default: 0)
    #[serde(default)]
    pub offset: usize,

    /// Filter by severity level (TRACE, DEBUG, INFO, WARN, ERROR, FATAL)
    pub severity: Option<String>,

    /// Search in log message (case-insensitive substring match)
    pub search: Option<String>,

    /// Filter by trace ID
    pub trace_id: Option<String>,

    /// Filter by span ID
    pub span_id: Option<String>,

    /// Start time (Unix timestamp in milliseconds)
    pub start_time: Option<i64>,

    /// End time (Unix timestamp in milliseconds)
    pub end_time: Option<i64>,

    /// Relative time range (e.g., "1h", "30m", "7d")
    pub since: Option<String>,
}

impl Default for LogQueryParams {
    fn default() -> Self {
        Self {
            limit: default_limit(),
            offset: 0,
            severity: None,
            search: None,
            trace_id: None,
            span_id: None,
            start_time: None,
            end_time: None,
            since: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_list_params() {
        let params = ListQueryParams::default();
        assert_eq!(params.limit, 100);
        assert_eq!(params.offset, 0);
    }

    #[test]
    fn test_time_range_parse_since() {
        let range = TimeRange {
            start: None,
            end: None,
            since: Some("1h".to_string()),
        };
        assert_eq!(range.parse_since(), Some(3_600_000));

        let range = TimeRange {
            start: None,
            end: None,
            since: Some("30m".to_string()),
        };
        assert_eq!(range.parse_since(), Some(1_800_000));

        let range = TimeRange {
            start: None,
            end: None,
            since: Some("7d".to_string()),
        };
        assert_eq!(range.parse_since(), Some(604_800_000));
    }

    #[test]
    fn test_time_range_effective_start() {
        let range = TimeRange {
            start: Some(1000),
            end: None,
            since: None,
        };
        assert_eq!(range.effective_start(), Some(1000));
    }
}

// Made with Bob
