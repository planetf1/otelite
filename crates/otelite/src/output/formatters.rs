//! Shared formatting utilities for output

use chrono::{DateTime, Utc};

/// Format a timestamp in human-readable format (ISO 8601)
pub fn format_timestamp(timestamp: &DateTime<Utc>) -> String {
    timestamp.format("%Y-%m-%d %H:%M:%S UTC").to_string()
}

/// Format a duration in human-readable format
/// Examples: 1.5s, 250ms, 1500ms
pub fn format_duration(duration_ms: u64) -> String {
    if duration_ms >= 1000 {
        format!("{:.1}s", duration_ms as f64 / 1000.0)
    } else {
        format!("{}ms", duration_ms)
    }
}

/// Parse a duration string (e.g., "1h", "30m", "5s") into milliseconds
pub fn parse_duration(duration_str: &str) -> Result<u64, String> {
    let duration_str = duration_str.trim();

    if duration_str.is_empty() {
        return Err("Duration string is empty".to_string());
    }

    // Find where the number ends and the unit begins
    let mut num_end = 0;
    for (i, c) in duration_str.chars().enumerate() {
        if !c.is_ascii_digit() && c != '.' {
            num_end = i;
            break;
        }
    }

    if num_end == 0 {
        return Err(format!(
            "Invalid duration format: '{}'. Expected format like '1h', '30m', '5s'",
            duration_str
        ));
    }

    let num_str = &duration_str[..num_end];
    let unit = &duration_str[num_end..];

    let value: f64 = num_str
        .parse()
        .map_err(|_| format!("Invalid number in duration: '{}'", num_str))?;

    let multiplier = match unit {
        "ms" => 1,
        "s" => 1000,
        "m" => 60 * 1000,
        "h" => 60 * 60 * 1000,
        "d" => 24 * 60 * 60 * 1000,
        _ => {
            return Err(format!(
                "Invalid duration unit: '{}'. Use 'ms', 's', 'm', 'h', or 'd'",
                unit
            ))
        },
    };

    Ok((value * multiplier as f64) as u64)
}

/// Truncate a string to a maximum length with ellipsis
pub fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}

/// Format a number with thousands separators
pub fn format_number(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();

    for (count, c) in s.chars().rev().enumerate() {
        if count > 0 && count % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }

    result.chars().rev().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(500), "500ms");
        assert_eq!(format_duration(1000), "1.0s");
        assert_eq!(format_duration(1500), "1.5s");
        assert_eq!(format_duration(2000), "2.0s");
    }

    #[test]
    fn test_parse_duration() {
        assert_eq!(parse_duration("500ms").unwrap(), 500);
        assert_eq!(parse_duration("1s").unwrap(), 1000);
        assert_eq!(parse_duration("30m").unwrap(), 30 * 60 * 1000);
        assert_eq!(parse_duration("1h").unwrap(), 60 * 60 * 1000);
        assert_eq!(parse_duration("2d").unwrap(), 2 * 24 * 60 * 60 * 1000);
    }

    #[test]
    fn test_parse_duration_with_decimals() {
        assert_eq!(parse_duration("1.5s").unwrap(), 1500);
        assert_eq!(parse_duration("0.5h").unwrap(), 30 * 60 * 1000);
    }

    #[test]
    fn test_parse_duration_invalid() {
        assert!(parse_duration("").is_err());
        assert!(parse_duration("abc").is_err());
        assert!(parse_duration("1x").is_err());
        assert!(parse_duration("1").is_err());
    }

    #[test]
    fn test_truncate_string() {
        assert_eq!(truncate_string("hello", 10), "hello");
        assert_eq!(truncate_string("hello world", 8), "hello...");
        assert_eq!(truncate_string("hi", 5), "hi");
    }

    #[test]
    fn test_format_number() {
        assert_eq!(format_number(123), "123");
        assert_eq!(format_number(1234), "1,234");
        assert_eq!(format_number(1234567), "1,234,567");
    }

    #[test]
    fn test_format_timestamp() {
        let timestamp = Utc::now();
        let formatted = format_timestamp(&timestamp);
        assert!(formatted.contains("UTC"));
        assert!(formatted.contains("-"));
        assert!(formatted.contains(":"));
    }
}
