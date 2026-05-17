// API module

use axum::{http::StatusCode, Json};
use otelite_core::api::ErrorResponse;

pub mod admin;
pub mod genai;
pub mod health;
pub mod help;
pub mod logs;
pub mod metrics;
pub mod resource_keys;
pub mod stats;
pub mod traces;

pub use genai::get_token_usage;
pub use health::health_check;
pub use help::api_help;

pub(crate) fn validate_time_range(
    start_time: Option<i64>,
    end_time: Option<i64>,
) -> Result<(), (StatusCode, Json<ErrorResponse>)> {
    if let Some(start_time) = start_time {
        if start_time < 0 {
            return Err(time_range_error("start_time must be non-negative"));
        }
    }

    if let Some(end_time) = end_time {
        if end_time < 0 {
            return Err(time_range_error("end_time must be non-negative"));
        }
    }

    if let (Some(start_time), Some(end_time)) = (start_time, end_time) {
        if start_time >= end_time {
            return Err(time_range_error("start_time must be less than end_time"));
        }
    }

    Ok(())
}

fn time_range_error(message: &'static str) -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::BAD_REQUEST,
        Json(ErrorResponse::bad_request(message)),
    )
}
