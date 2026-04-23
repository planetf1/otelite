//! Consistent color scheme for CLI output

use comfy_table::Color;
use std::env;
use std::io::IsTerminal;

/// Check if colors should be disabled
pub fn colors_disabled() -> bool {
    // Check --no-color flag (handled by clap) or NO_COLOR env var
    env::var("NO_COLOR").is_ok() || !std::io::stdout().is_terminal()
}

/// Color scheme for severity levels
pub fn severity_color(severity: &str) -> Color {
    match severity {
        "ERROR" => Color::Red,
        "WARN" => Color::Yellow,
        "INFO" => Color::Green,
        "DEBUG" => Color::DarkGrey,
        _ => Color::Reset,
    }
}

/// Color for timestamps (dim/gray)
pub fn timestamp_color() -> Color {
    Color::DarkGrey
}

/// Color for IDs (cyan)
pub fn id_color() -> Color {
    Color::Cyan
}

/// Color for GenAI system names (magenta)
pub fn genai_system_color() -> Color {
    Color::Magenta
}

/// Color for token counts (yellow)
pub fn token_count_color() -> Color {
    Color::Yellow
}

/// Color for metric types
pub fn metric_type_color(metric_type: &str) -> Color {
    match metric_type {
        "counter" => Color::Green,
        "gauge" => Color::Blue,
        "histogram" => Color::Yellow,
        "summary" => Color::Cyan,
        _ => Color::Reset,
    }
}

/// Color for trace status
pub fn trace_status_color(has_errors: bool) -> Color {
    if has_errors {
        Color::Red
    } else {
        Color::Green
    }
}

/// ANSI escape codes for direct terminal output
pub mod ansi {
    /// Check if colors should be disabled
    pub fn colors_disabled() -> bool {
        super::colors_disabled()
    }

    /// Red color (for ERROR severity)
    pub const RED: &str = "\x1b[31m";

    /// Yellow color (for WARN severity, token counts)
    pub const YELLOW: &str = "\x1b[33m";

    /// Green color (for INFO severity, success)
    pub const GREEN: &str = "\x1b[32m";

    /// Blue color (for INFO alternative)
    pub const BLUE: &str = "\x1b[34m";

    /// Dark grey color (for DEBUG severity, timestamps)
    pub const DARK_GREY: &str = "\x1b[90m";

    /// Cyan color (for IDs)
    pub const CYAN: &str = "\x1b[36m";

    /// Magenta color (for GenAI system names)
    pub const MAGENTA: &str = "\x1b[35m";

    /// Bold text
    pub const BOLD: &str = "\x1b[1m";

    /// Reset all formatting
    pub const RESET: &str = "\x1b[0m";

    /// Get severity color code
    pub fn severity_color(severity: &str) -> &'static str {
        match severity {
            "ERROR" => RED,
            "WARN" => YELLOW,
            "INFO" => GREEN,
            "DEBUG" => DARK_GREY,
            _ => "",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_severity_colors() {
        assert_eq!(severity_color("ERROR"), Color::Red);
        assert_eq!(severity_color("WARN"), Color::Yellow);
        assert_eq!(severity_color("INFO"), Color::Green);
        assert_eq!(severity_color("DEBUG"), Color::DarkGrey);
        assert_eq!(severity_color("UNKNOWN"), Color::Reset);
    }

    #[test]
    fn test_metric_type_colors() {
        assert_eq!(metric_type_color("counter"), Color::Green);
        assert_eq!(metric_type_color("gauge"), Color::Blue);
        assert_eq!(metric_type_color("histogram"), Color::Yellow);
        assert_eq!(metric_type_color("summary"), Color::Cyan);
        assert_eq!(metric_type_color("unknown"), Color::Reset);
    }

    #[test]
    fn test_trace_status_colors() {
        assert_eq!(trace_status_color(true), Color::Red);
        assert_eq!(trace_status_color(false), Color::Green);
    }

    #[test]
    fn test_ansi_severity_colors() {
        assert_eq!(ansi::severity_color("ERROR"), ansi::RED);
        assert_eq!(ansi::severity_color("WARN"), ansi::YELLOW);
        assert_eq!(ansi::severity_color("INFO"), ansi::GREEN);
        assert_eq!(ansi::severity_color("DEBUG"), ansi::DARK_GREY);
        assert_eq!(ansi::severity_color("UNKNOWN"), "");
    }
}
