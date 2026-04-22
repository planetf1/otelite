//! Helpers for formatting telemetry attribute values.

use serde_json::Value;

/// If the string is valid JSON, return it pretty-printed. Otherwise return as-is.
#[must_use]
pub fn format_attribute_value(value: &str) -> String {
    if let Ok(parsed) = serde_json::from_str::<Value>(value) {
        serde_json::to_string_pretty(&parsed).unwrap_or_else(|_| value.to_string())
    } else {
        value.to_string()
    }
}

/// Return a truncated preview of a JSON value (first N chars + "...")
#[must_use]
pub fn format_attribute_preview(value: &str, max_chars: usize) -> String {
    if let Ok(parsed) = serde_json::from_str::<Value>(value) {
        match parsed {
            Value::Object(map) => format!("{{object, {} keys}}", map.len()),
            Value::Array(items) => format!("[array, {} items]", items.len()),
            _ => truncate_with_ellipsis(value, max_chars),
        }
    } else {
        truncate_with_ellipsis(value, max_chars)
    }
}

fn truncate_with_ellipsis(value: &str, max_chars: usize) -> String {
    let char_count = value.chars().count();

    if char_count <= max_chars {
        return value.to_string();
    }

    if max_chars <= 3 {
        return ".".repeat(max_chars);
    }

    let truncated: String = value.chars().take(max_chars - 3).collect();
    format!("{truncated}...")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_json_object() {
        let input = r#"{"key":"value","nested":{"a":1}}"#;
        let output = format_attribute_value(input);
        assert!(output.contains('\n'));
    }

    #[test]
    fn test_format_plain_string() {
        assert_eq!(format_attribute_value("hello"), "hello");
    }

    #[test]
    fn test_format_json_array() {
        let input = r#"[1,2,3]"#;
        let output = format_attribute_value(input);
        assert!(output.contains('\n'));
    }

    #[test]
    fn test_format_attribute_preview_for_object() {
        let input = r#"{"key":"value","nested":{"a":1}}"#;
        assert_eq!(format_attribute_preview(input, 10), "{object, 2 keys}");
    }

    #[test]
    fn test_format_attribute_preview_for_array() {
        let input = r#"[1,2,3]"#;
        assert_eq!(format_attribute_preview(input, 10), "[array, 3 items]");
    }

    #[test]
    fn test_format_attribute_preview_for_plain_string() {
        assert_eq!(format_attribute_preview("hello world", 8), "hello...");
    }
}
