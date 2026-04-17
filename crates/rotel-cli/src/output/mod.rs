//! Output formatting module for CLI

pub mod formatters;
pub mod json;
pub mod pretty;

pub use formatters::{
    format_duration, format_number, format_timestamp, parse_duration, truncate_string,
};
pub use json::{
    print_log_json, print_logs_json, print_metric_json, print_metrics_json, print_trace_json,
    print_traces_json,
};
pub use pretty::{
    print_log_details, print_logs_table, print_metric_details, print_metrics_table,
    print_trace_tree, print_traces_table,
};

// Made with Bob
