//! Response models for API endpoints

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use utoipa::ToSchema;

use super::PaginationMetadata;

/// Generic list response wrapper
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ListResponse<T> {
    /// List of items
    pub items: Vec<T>,

    /// Pagination metadata
    pub pagination: PaginationMetadata,
}

impl<T> ListResponse<T> {
    /// Create a new list response
    pub fn new(items: Vec<T>, pagination: PaginationMetadata) -> Self {
        Self { items, pagination }
    }
}

/// Generic success response
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SuccessResponse {
    /// Success message
    pub message: String,

    /// Optional data
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl SuccessResponse {
    /// Create a new success response
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            data: None,
        }
    }

    /// Add data to the response
    pub fn with_data(mut self, data: serde_json::Value) -> Self {
        self.data = Some(data);
        self
    }
}

/// Resource attributes (common across logs, traces, metrics)
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Default)]
pub struct ResourceAttributes {
    /// Service name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_name: Option<String>,

    /// Service version
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_version: Option<String>,

    /// Service instance ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_instance_id: Option<String>,

    /// Additional attributes
    #[serde(flatten)]
    pub attributes: HashMap<String, serde_json::Value>,
}

/// Log entry response model
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct LogEntry {
    /// Unique log identifier
    pub id: String,

    /// Unix timestamp in nanoseconds
    pub timestamp: i64,

    /// Log level (TRACE, DEBUG, INFO, WARN, ERROR, FATAL)
    pub severity: String,

    /// Log message body
    pub message: String,

    /// Resource attributes (service.name, etc.)
    pub resource: ResourceAttributes,

    /// Log-specific attributes
    pub attributes: HashMap<String, serde_json::Value>,

    /// Associated trace ID if available
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,

    /// Associated span ID if available
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_response() {
        let items = vec![1, 2, 3];
        let pagination = PaginationMetadata::new(100, 0, 10, 3);
        let response = ListResponse::new(items.clone(), pagination.clone());

        assert_eq!(response.items, items);
        assert_eq!(response.pagination.total, 100);
    }

    #[test]
    fn test_success_response() {
        let response = SuccessResponse::new("Operation completed");
        assert_eq!(response.message, "Operation completed");
        assert!(response.data.is_none());

        let response = response.with_data(serde_json::json!({"key": "value"}));
        assert!(response.data.is_some());
    }

    #[test]
    fn test_resource_attributes_default() {
        let attrs = ResourceAttributes::default();
        assert!(attrs.service_name.is_none());
        assert!(attrs.attributes.is_empty());
    }
}

// Made with Bob
