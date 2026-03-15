use axum::{
	body::Body,
	http::{Request, StatusCode},
	middleware::Next,
	response::{IntoResponse, Response},
	Json,
};
use serde_json::json;

/// Middleware to validate Content-Type on POST requests
/// Excludes /mcp, /oauth, and /v1/images endpoints
pub async fn validate_content_type(
	req: Request<Body>,
	next: Next,
) -> Result<Response, Response> {
	let method = req.method().clone();
	let path = req.uri().path().to_string();

	if method == "POST"
		&& !path.starts_with("/mcp")
		&& !path.starts_with("/oauth")
		&& !path.starts_with("/v1/images")
	{
		let content_type = req
			.headers()
			.get("content-type")
			.and_then(|v| v.to_str().ok())
			.unwrap_or("");

		if !content_type.contains("application/json") {
			return Err((
				StatusCode::UNSUPPORTED_MEDIA_TYPE,
				Json(json!({
					"error": true,
					"status": 415,
					"message": "Unsupported Media Type: Content-Type must be application/json",
				})),
			)
				.into_response());
		}
	}

	Ok(next.run(req).await)
}
