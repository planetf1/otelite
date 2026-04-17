//! OTLP protocol handling (Protobuf and JSON)

pub mod json;
pub mod protobuf;
pub mod validation;

pub use json::{parse_logs_json, parse_metrics_json, parse_traces_json, validate_json_message};
pub use protobuf::{handle_parse_error, parse_message, validate_message};
pub use validation::{validate_otlp_version, OtlpVersion};

// Made with Bob
