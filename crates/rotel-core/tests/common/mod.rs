// Common test utilities shared across integration and e2e tests

use std::path::PathBuf;

/// Get the path to the fixtures directory
pub fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

/// Load a fixture file as a string
pub fn load_fixture(filename: &str) -> String {
    let path = fixtures_dir().join(filename);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to load fixture {}: {}", filename, e))
}

/// Load a JSON fixture and parse it
pub fn load_json_fixture<T: serde::de::DeserializeOwned>(filename: &str) -> T {
    let content = load_fixture(filename);
    serde_json::from_str(&content)
        .unwrap_or_else(|e| panic!("Failed to parse JSON fixture {}: {}", filename, e))
}

/// Setup function for tests that need common initialization
pub fn setup() {
    // Initialize logging for tests if needed
    let _ = env_logger::builder().is_test(true).try_init();
}

/// Teardown function for tests that need cleanup
pub fn teardown() {
    // Cleanup logic if needed
}

/// Helper to create a temporary directory for tests
pub fn temp_dir() -> tempfile::TempDir {
    tempfile::tempdir().expect("Failed to create temp directory")
}

/// Helper to assert that a result is Ok and return the value
pub fn assert_ok<T, E: std::fmt::Debug>(result: Result<T, E>) -> T {
    match result {
        Ok(value) => value,
        Err(e) => panic!("Expected Ok, got Err: {:?}", e),
    }
}

/// Helper to assert that a result is Err
pub fn assert_err<T: std::fmt::Debug, E>(result: Result<T, E>) {
    match result {
        Ok(value) => panic!("Expected Err, got Ok: {:?}", value),
        Err(_) => {},
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fixtures_dir_exists() {
        let dir = fixtures_dir();
        assert!(dir.exists(), "Fixtures directory should exist");
    }

    #[test]
    fn test_load_fixture() {
        let content = load_fixture("sample_metrics.json");
        assert!(!content.is_empty(), "Fixture content should not be empty");
        assert!(
            content.contains("resourceMetrics"),
            "Should contain OTLP metrics structure"
        );
    }

    #[test]
    fn test_temp_dir() {
        let dir = temp_dir();
        assert!(dir.path().exists(), "Temp directory should exist");
    }

    #[test]
    fn test_assert_ok() {
        let result: Result<i32, &str> = Ok(42);
        let value = assert_ok(result);
        assert_eq!(value, 42);
    }

    #[test]
    fn test_assert_err() {
        let result: Result<i32, &str> = Err("error");
        assert_err(result);
    }
}

// Made with Bob
