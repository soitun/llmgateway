use axum::{
	middleware,
	routing::{get, post},
	Router,
};
use sqlx::PgPool;
use std::time::Duration;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use crate::cache::RedisCache;
use crate::config::Config;
use crate::middleware::content_type::validate_content_type;
use crate::routes;

/// Shared application state
#[derive(Clone)]
pub struct AppState {
	pub db: PgPool,
	pub redis: RedisCache,
	pub config: Config,
	pub http_client: reqwest::Client,
}

/// Build the application router with all routes and middleware
pub fn build_router(state: AppState) -> Router {
	let cors = CorsLayer::new()
		.allow_origin(Any)
		.allow_methods(Any)
		.allow_headers(Any)
		.expose_headers(Any)
		.max_age(Duration::from_secs(600));

	// V1 routes
	let v1 = Router::new()
		.route("/chat/completions", post(routes::chat::chat_completions))
		.route("/models", get(routes::models::list_models));

	Router::new()
		// Health check
		.route("/", get(routes::health::health_check))
		// V1 API routes
		.nest("/v1", v1)
		// Middleware
		.layer(middleware::from_fn(validate_content_type))
		.layer(cors)
		.layer(TraceLayer::new_for_http())
		// State
		.with_state(state)
}
