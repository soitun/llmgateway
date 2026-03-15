use axum::{
	extract::{Query, State},
	http::StatusCode,
	Json,
};
use serde::{Deserialize, Serialize};

use crate::app::AppState;

#[derive(Deserialize)]
pub struct HealthQuery {
	pub skip: Option<String>,
}

#[derive(Serialize)]
pub struct HealthResponse {
	pub message: String,
	pub version: String,
	pub health: HealthStatus,
}

#[derive(Serialize)]
pub struct HealthStatus {
	pub status: String,
	pub redis: ComponentHealth,
	pub database: ComponentHealth,
}

#[derive(Serialize)]
pub struct ComponentHealth {
	pub connected: bool,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub error: Option<String>,
}

/// GET / - Health check endpoint
pub async fn health_check(
	State(state): State<AppState>,
	Query(query): Query<HealthQuery>,
) -> (StatusCode, Json<HealthResponse>) {
	let skip_checks: Vec<String> = query
		.skip
		.map(|s| s.split(',').map(|s| s.trim().to_lowercase()).collect())
		.unwrap_or_default();

	// By default, skip database health check for gateway
	let skip_database = state.config.health_check_skip_database;
	let should_skip_database = skip_database || skip_checks.contains(&"database".to_string());
	let should_skip_redis = skip_checks.contains(&"redis".to_string());

	// Check Redis
	let redis_health = if should_skip_redis {
		ComponentHealth {
			connected: true,
			error: None,
		}
	} else {
		match state.redis.check_health().await {
			Ok(true) => ComponentHealth {
				connected: true,
				error: None,
			},
			Ok(false) => ComponentHealth {
				connected: false,
				error: Some("Redis not responding".to_string()),
			},
			Err(e) => ComponentHealth {
				connected: false,
				error: Some(e),
			},
		}
	};

	// Check Database
	let database_health = if should_skip_database {
		ComponentHealth {
			connected: true,
			error: None,
		}
	} else {
		match crate::db::check_database(&state.db).await {
			Ok(true) => ComponentHealth {
				connected: true,
				error: None,
			},
			Ok(false) => ComponentHealth {
				connected: false,
				error: Some("Database not responding".to_string()),
			},
			Err(e) => ComponentHealth {
				connected: false,
				error: Some(e),
			},
		}
	};

	let overall_status = if redis_health.connected && database_health.connected {
		"ok"
	} else {
		"error"
	};

	let status_code = if overall_status == "ok" {
		StatusCode::OK
	} else {
		StatusCode::SERVICE_UNAVAILABLE
	};

	(
		status_code,
		Json(HealthResponse {
			message: "LLMGateway Gateway".to_string(),
			version: "1.0.0".to_string(),
			health: HealthStatus {
				status: overall_status.to_string(),
				redis: redis_health,
				database: database_health,
			},
		}),
	)
}
