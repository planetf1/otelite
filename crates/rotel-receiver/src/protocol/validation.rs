//! OTLP protocol validation

use crate::error::{ReceiverError, Result};

/// Supported OTLP protocol versions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OtlpVersion {
    /// OTLP v1.0.0
    V1_0_0,

    /// OTLP v1.1.0
    V1_1_0,

    /// OTLP v1.2.0
    V1_2_0,
}

impl OtlpVersion {
    /// Parse version string
    pub fn parse(version: &str) -> Result<Self> {
        match version {
            "1.0.0" | "1.0" | "1" => Ok(Self::V1_0_0),
            "1.1.0" | "1.1" => Ok(Self::V1_1_0),
            "1.2.0" | "1.2" => Ok(Self::V1_2_0),
            _ => Err(ReceiverError::InvalidProtocolVersion(version.to_string())),
        }
    }

    /// Get version string
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::V1_0_0 => "1.0.0",
            Self::V1_1_0 => "1.1.0",
            Self::V1_2_0 => "1.2.0",
        }
    }

    /// Check if version is supported
    pub fn is_supported(&self) -> bool {
        // All versions are currently supported
        true
    }
}

/// Validate OTLP protocol version
pub fn validate_otlp_version(version: &str) -> Result<OtlpVersion> {
    let parsed = OtlpVersion::parse(version)?;

    if !parsed.is_supported() {
        return Err(ReceiverError::InvalidProtocolVersion(format!(
            "Version {} is not supported",
            version
        )));
    }

    Ok(parsed)
}

/// Validate message size
pub fn validate_message_size(size: usize, max_size: usize) -> Result<()> {
    if size > max_size {
        return Err(ReceiverError::MessageTooLarge {
            size,
            max: max_size,
        });
    }
    Ok(())
}

/// Validate required field is present
pub fn validate_required_field<T>(field: Option<T>, field_name: &str) -> Result<T> {
    field.ok_or_else(|| ReceiverError::MissingField(field_name.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_parsing() {
        assert_eq!(OtlpVersion::parse("1.0.0").unwrap(), OtlpVersion::V1_0_0);
        assert_eq!(OtlpVersion::parse("1.0").unwrap(), OtlpVersion::V1_0_0);
        assert_eq!(OtlpVersion::parse("1").unwrap(), OtlpVersion::V1_0_0);
        assert_eq!(OtlpVersion::parse("1.1.0").unwrap(), OtlpVersion::V1_1_0);
        assert_eq!(OtlpVersion::parse("1.2.0").unwrap(), OtlpVersion::V1_2_0);
    }

    #[test]
    fn test_invalid_version() {
        assert!(OtlpVersion::parse("2.0.0").is_err());
        assert!(OtlpVersion::parse("invalid").is_err());
    }

    #[test]
    fn test_validate_otlp_version() {
        assert!(validate_otlp_version("1.0.0").is_ok());
        assert!(validate_otlp_version("1.1.0").is_ok());
        assert!(validate_otlp_version("2.0.0").is_err());
    }

    #[test]
    fn test_validate_message_size() {
        assert!(validate_message_size(1000, 10000).is_ok());
        assert!(validate_message_size(10000, 10000).is_ok());
        assert!(validate_message_size(10001, 10000).is_err());
    }

    #[test]
    fn test_validate_required_field() {
        assert!(validate_required_field(Some("value"), "field").is_ok());
        assert!(validate_required_field(None::<&str>, "field").is_err());
    }
}

// Made with Bob
