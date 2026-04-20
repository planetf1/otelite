//! Resource types for telemetry data

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Represents a resource (source of telemetry)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Resource {
    /// Resource attributes (key-value pairs)
    pub attributes: HashMap<String, String>,
}

impl Resource {
    /// Create a new empty resource
    pub fn new() -> Self {
        Self {
            attributes: HashMap::new(),
        }
    }

    /// Create a resource with service name
    pub fn with_service_name(name: impl Into<String>) -> Self {
        let mut resource = Self::new();
        resource
            .attributes
            .insert("service.name".to_string(), name.into());
        resource
    }

    /// Add an attribute to the resource
    pub fn with_attribute(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.attributes.insert(key.into(), value.into());
        self
    }

    /// Get an attribute value
    pub fn get_attribute(&self, key: &str) -> Option<&String> {
        self.attributes.get(key)
    }

    /// Get the service name
    pub fn service_name(&self) -> Option<&String> {
        self.get_attribute("service.name")
    }
}

impl Default for Resource {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resource_creation() {
        let resource = Resource::new();
        assert!(resource.attributes.is_empty());
    }

    #[test]
    fn test_resource_with_service_name() {
        let resource = Resource::with_service_name("my-service");
        assert_eq!(resource.service_name(), Some(&"my-service".to_string()));
    }

    #[test]
    fn test_resource_with_attributes() {
        let resource = Resource::with_service_name("my-service")
            .with_attribute("service.version", "1.0.0")
            .with_attribute("deployment.environment", "production")
            .with_attribute("host.name", "server-01");

        assert_eq!(resource.attributes.len(), 4);
        assert_eq!(
            resource.get_attribute("service.version"),
            Some(&"1.0.0".to_string())
        );
        assert_eq!(
            resource.get_attribute("deployment.environment"),
            Some(&"production".to_string())
        );
    }

    #[test]
    fn test_resource_get_nonexistent_attribute() {
        let resource = Resource::with_service_name("my-service");
        assert_eq!(resource.get_attribute("nonexistent"), None);
    }
}
