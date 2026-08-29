use axum::{Json, http::StatusCode};
use serde::{Deserialize, Serialize};

/// Error codes used in `Response::error`.
pub const INVALID_INPUT: &str = "INVALID_INPUT";
pub const NOT_FOUND: &str = "NOT_FOUND";
pub const UNAUTHORIZED: &str = "UNAUTHORIZED";
pub const DB_ERROR: &str = "DB_ERROR";
pub const SCHEDULER_ERROR: &str = "SCHEDULER_ERROR";
pub const INTERNAL_ERROR: &str = "INTERNAL_ERROR";

/// Error return type of HTTP handlers: `(StatusCode, Json<Response<T>>)`.
pub type ErrorResponse<T> = (StatusCode, Json<Response<T>>);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response<T> {
    #[serde(rename = "code")]
    pub error_code: String,
    #[serde(rename = "msg")]
    pub error_message: String,
    #[serde(rename = "data", skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
}

impl<T> Response<T> {
    pub fn success(data: T) -> Self {
        Self {
            error_code: "0".to_string(),
            error_message: "success".to_string(),
            data: Some(data),
        }
    }

    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            error_code: code.into(),
            error_message: message.into(),
            data: None,
        }
    }
}

fn error_response<T>(
    status: StatusCode,
    code: &'static str,
    message: impl Into<String>,
) -> ErrorResponse<T> {
    (status, Json(Response::error(code, message)))
}

/// 400 Bad Request with code `INVALID_INPUT`.
pub fn bad_request<T>(message: impl Into<String>) -> ErrorResponse<T> {
    error_response(StatusCode::BAD_REQUEST, INVALID_INPUT, message)
}

/// 404 Not Found with code `NOT_FOUND`.
pub fn not_found<T>(message: impl Into<String>) -> ErrorResponse<T> {
    error_response(StatusCode::NOT_FOUND, NOT_FOUND, message)
}

/// 401 Unauthorized with code `UNAUTHORIZED`.
pub fn unauthorized<T>(message: impl Into<String>) -> ErrorResponse<T> {
    error_response(StatusCode::UNAUTHORIZED, UNAUTHORIZED, message)
}

/// 500 Internal Server Error with code `DB_ERROR`.
pub fn db_error<T>(message: impl Into<String>) -> ErrorResponse<T> {
    error_response(StatusCode::INTERNAL_SERVER_ERROR, DB_ERROR, message)
}

/// Code `SCHEDULER_ERROR` with a caller-chosen status code.
pub fn scheduler_error<T>(status: StatusCode, message: impl Into<String>) -> ErrorResponse<T> {
    error_response(status, SCHEDULER_ERROR, message)
}

/// 500 Internal Server Error with code `INTERNAL_ERROR`.
pub fn internal_error<T>(message: impl Into<String>) -> ErrorResponse<T> {
    error_response(StatusCode::INTERNAL_SERVER_ERROR, INTERNAL_ERROR, message)
}
