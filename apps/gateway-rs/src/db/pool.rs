use sqlx::postgres::{PgPool, PgPoolOptions};
use std::time::Duration;

/// Create a PostgreSQL connection pool
pub async fn create_pool(database_url: &str) -> Result<PgPool, sqlx::Error> {
	let max_connections: u32 = std::env::var("DATABASE_POOL_MAX")
		.ok()
		.and_then(|v| v.parse().ok())
		.unwrap_or(20);

	let min_connections: u32 = std::env::var("DATABASE_POOL_MIN")
		.ok()
		.and_then(|v| v.parse().ok())
		.unwrap_or(2);

	PgPoolOptions::new()
		.max_connections(max_connections)
		.min_connections(min_connections)
		.idle_timeout(Duration::from_secs(30))
		.acquire_timeout(Duration::from_secs(10))
		.connect(database_url)
		.await
}

/// Check database connectivity
pub async fn check_database(pool: &PgPool) -> Result<bool, String> {
	match sqlx::query("SELECT 1").execute(pool).await {
		Ok(_) => Ok(true),
		Err(e) => Err(format!("Database health check failed: {e}")),
	}
}
