use std::env;

/// Gateway configuration loaded from environment variables
#[derive(Debug, Clone)]
pub struct Config {
	pub port: u16,
	pub keep_alive_timeout_s: u64,
	pub shutdown_grace_period_ms: u64,
	pub database_url: String,
	pub redis_url: String,
	pub node_env: String,

	// Timeouts
	pub gateway_timeout_ms: u64,
	pub ai_timeout_ms: u64,
	pub ai_streaming_timeout_ms: u64,
	pub health_check_timeout_ms: u64,
	pub health_check_skip_database: bool,

	// Image handling
	pub image_size_limit_free_mb: u64,
	pub image_size_limit_pro_mb: u64,
	pub max_streaming_buffer_mb: u64,

	// Debugging
	pub force_debug_mode: bool,

	// Billing
	pub bill_cancelled_requests: bool,

	// Tracing
	pub otel_service_name: String,
}

impl Config {
	pub fn from_env() -> Self {
		let redis_host = env::var("REDIS_HOST").unwrap_or_else(|_| "localhost".to_string());
		let redis_port = env::var("REDIS_PORT").unwrap_or_else(|_| "6379".to_string());
		let redis_password = env::var("REDIS_PASSWORD").ok();
		let redis_url = if let Some(pw) = redis_password {
			format!("redis://:{pw}@{redis_host}:{redis_port}")
		} else {
			format!("redis://{redis_host}:{redis_port}")
		};

		Self {
			port: env::var("PORT")
				.ok()
				.and_then(|v| v.parse().ok())
				.unwrap_or(4001),
			keep_alive_timeout_s: env::var("KEEP_ALIVE_TIMEOUT_S")
				.ok()
				.and_then(|v| v.parse().ok())
				.unwrap_or(620),
			shutdown_grace_period_ms: env::var("SHUTDOWN_GRACE_PERIOD_MS")
				.ok()
				.and_then(|v| v.parse().ok())
				.unwrap_or(120_000),
			database_url: env::var("DATABASE_URL")
				.unwrap_or_else(|_| "postgres://localhost:5432/llmgateway".to_string()),
			redis_url,
			node_env: env::var("NODE_ENV").unwrap_or_else(|_| "development".to_string()),

			gateway_timeout_ms: env::var("GATEWAY_TIMEOUT_MS")
				.ok()
				.and_then(|v| v.parse().ok())
				.unwrap_or(300_000),
			ai_timeout_ms: env::var("AI_TIMEOUT_MS")
				.ok()
				.and_then(|v| v.parse().ok())
				.unwrap_or(180_000),
			ai_streaming_timeout_ms: env::var("AI_STREAMING_TIMEOUT_MS")
				.ok()
				.and_then(|v| v.parse().ok())
				.unwrap_or(240_000),
			health_check_timeout_ms: env::var("HEALTH_CHECK_TIMEOUT_MS")
				.ok()
				.and_then(|v| v.parse().ok())
				.unwrap_or(15_000),
			health_check_skip_database: env::var("HEALTH_CHECK_SKIP_DATABASE")
				.map(|v| v != "false")
				.unwrap_or(true),

			image_size_limit_free_mb: env::var("IMAGE_SIZE_LIMIT_FREE_MB")
				.ok()
				.and_then(|v| v.parse().ok())
				.unwrap_or(50),
			image_size_limit_pro_mb: env::var("IMAGE_SIZE_LIMIT_PRO_MB")
				.ok()
				.and_then(|v| v.parse().ok())
				.unwrap_or(100),
			max_streaming_buffer_mb: env::var("MAX_STREAMING_BUFFER_MB")
				.ok()
				.and_then(|v| v.parse().ok())
				.unwrap_or(50),

			force_debug_mode: env::var("FORCE_DEBUG_MODE")
				.map(|v| v == "true")
				.unwrap_or(false),

			bill_cancelled_requests: env::var("BILL_CANCELLED_REQUESTS")
				.map(|v| v == "true")
				.unwrap_or(false),

			otel_service_name: env::var("OTEL_SERVICE_NAME")
				.unwrap_or_else(|_| "llmgateway-gateway".to_string()),
		}
	}

	pub fn is_production(&self) -> bool {
		self.node_env == "production"
	}

	pub fn is_debug_mode(&self) -> bool {
		self.force_debug_mode || !self.is_production()
	}
}
