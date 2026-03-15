use axum::{
	http::StatusCode,
	response::{IntoResponse, Response},
	Json,
};
use serde_json::json;

/// Application error type that maps to HTTP responses
#[derive(Debug)]
pub enum AppError {
	/// Client errors (4xx)
	BadRequest(String),
	Unauthorized(String),
	PaymentRequired(String),
	Forbidden(String),
	NotFound(String),
	Gone(String),
	UnsupportedMediaType(String),
	TooManyRequests(String),

	/// Server errors (5xx)
	Internal(String),
	BadGateway(String),
	GatewayTimeout(String),

	/// Client disconnected
	ClientClosed(String),

	/// Database error
	Database(sqlx::Error),

	/// Redis error
	Redis(redis::RedisError),

	/// HTTP client error
	HttpClient(reqwest::Error),

	/// JSON error
	Json(serde_json::Error),
}

impl std::fmt::Display for AppError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::BadRequest(msg) => write!(f, "Bad Request: {msg}"),
			Self::Unauthorized(msg) => write!(f, "Unauthorized: {msg}"),
			Self::PaymentRequired(msg) => write!(f, "Payment Required: {msg}"),
			Self::Forbidden(msg) => write!(f, "Forbidden: {msg}"),
			Self::NotFound(msg) => write!(f, "Not Found: {msg}"),
			Self::Gone(msg) => write!(f, "Gone: {msg}"),
			Self::UnsupportedMediaType(msg) => write!(f, "Unsupported Media Type: {msg}"),
			Self::TooManyRequests(msg) => write!(f, "Too Many Requests: {msg}"),
			Self::Internal(msg) => write!(f, "Internal Server Error: {msg}"),
			Self::BadGateway(msg) => write!(f, "Bad Gateway: {msg}"),
			Self::GatewayTimeout(msg) => write!(f, "Gateway Timeout: {msg}"),
			Self::ClientClosed(msg) => write!(f, "Client Closed: {msg}"),
			Self::Database(e) => write!(f, "Database Error: {e}"),
			Self::Redis(e) => write!(f, "Redis Error: {e}"),
			Self::HttpClient(e) => write!(f, "HTTP Client Error: {e}"),
			Self::Json(e) => write!(f, "JSON Error: {e}"),
		}
	}
}

impl std::error::Error for AppError {}

impl IntoResponse for AppError {
	fn into_response(self) -> Response {
		let (status, message) = match &self {
			Self::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
			Self::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, msg.clone()),
			Self::PaymentRequired(msg) => (StatusCode::PAYMENT_REQUIRED, msg.clone()),
			Self::Forbidden(msg) => (StatusCode::FORBIDDEN, msg.clone()),
			Self::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
			Self::Gone(msg) => (StatusCode::GONE, msg.clone()),
			Self::UnsupportedMediaType(msg) => (StatusCode::UNSUPPORTED_MEDIA_TYPE, msg.clone()),
			Self::TooManyRequests(msg) => (StatusCode::TOO_MANY_REQUESTS, msg.clone()),
			Self::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
			Self::BadGateway(msg) => (StatusCode::BAD_GATEWAY, msg.clone()),
			Self::GatewayTimeout(msg) => (StatusCode::GATEWAY_TIMEOUT, msg.clone()),
			Self::ClientClosed(msg) => (
				StatusCode::from_u16(499).unwrap_or(StatusCode::BAD_REQUEST),
				msg.clone(),
			),
			Self::Database(e) => {
				tracing::error!("Database error: {e}");
				(
					StatusCode::INTERNAL_SERVER_ERROR,
					"Internal Server Error".to_string(),
				)
			}
			Self::Redis(e) => {
				tracing::error!("Redis error: {e}");
				(
					StatusCode::INTERNAL_SERVER_ERROR,
					"Internal Server Error".to_string(),
				)
			}
			Self::HttpClient(e) => {
				tracing::error!("HTTP client error: {e}");
				(
					StatusCode::INTERNAL_SERVER_ERROR,
					"Internal Server Error".to_string(),
				)
			}
			Self::Json(e) => {
				tracing::error!("JSON error: {e}");
				(StatusCode::BAD_REQUEST, format!("Invalid JSON: {e}"))
			}
		};

		let body = json!({
			"error": true,
			"status": status.as_u16(),
			"message": message,
		});

		(status, Json(body)).into_response()
	}
}

/// OpenAI-compatible error response format
pub fn openai_error(status: StatusCode, message: &str, error_type: &str, code: &str) -> Response {
	let body = json!({
		"error": {
			"message": message,
			"type": error_type,
			"param": serde_json::Value::Null,
			"code": code,
		}
	});
	(status, Json(body)).into_response()
}

impl From<sqlx::Error> for AppError {
	fn from(e: sqlx::Error) -> Self {
		Self::Database(e)
	}
}

impl From<redis::RedisError> for AppError {
	fn from(e: redis::RedisError) -> Self {
		Self::Redis(e)
	}
}

impl From<reqwest::Error> for AppError {
	fn from(e: reqwest::Error) -> Self {
		Self::HttpClient(e)
	}
}

impl From<serde_json::Error> for AppError {
	fn from(e: serde_json::Error) -> Self {
		Self::Json(e)
	}
}
