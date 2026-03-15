use redis::aio::ConnectionManager;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Streaming cache chunk
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamingChunk {
	pub data: String,
	pub event_id: u64,
	pub event: Option<String>,
	pub timestamp: u64,
}

/// Streaming cache metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamingCacheMetadata {
	pub model: String,
	pub provider: String,
	pub finish_reason: Option<String>,
	pub total_chunks: usize,
	pub duration: u64,
	pub completed: bool,
	#[serde(default)]
	pub tool_results: Option<serde_json::Value>,
}

/// Streaming cache data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamingCacheData {
	pub chunks: Vec<StreamingChunk>,
	pub metadata: StreamingCacheMetadata,
}

/// Redis cache wrapper
#[derive(Clone)]
pub struct RedisCache {
	conn: ConnectionManager,
}

impl RedisCache {
	pub async fn new(redis_url: &str) -> Result<Self, redis::RedisError> {
		let client = redis::Client::open(redis_url)?;
		let conn = ConnectionManager::new(client).await?;
		Ok(Self { conn })
	}

	/// Check Redis connectivity
	pub async fn check_health(&self) -> Result<bool, String> {
		let mut conn = self.conn.clone();
		match redis::cmd("PING")
			.query_async::<String>(&mut conn)
			.await
		{
			Ok(response) if response == "PONG" => Ok(true),
			Ok(response) => Err(format!("Unexpected PING response: {response}")),
			Err(e) => Err(format!("Redis health check failed: {e}")),
		}
	}

	/// Generate a cache key from a payload
	pub fn generate_cache_key(payload: &serde_json::Value) -> String {
		let serialized = serde_json::to_string(payload).unwrap_or_default();
		let mut hasher = Sha256::new();
		hasher.update(serialized.as_bytes());
		let hash = hex::encode(hasher.finalize());
		hash
	}

	/// Generate a streaming cache key
	pub fn generate_streaming_cache_key(payload: &serde_json::Value) -> String {
		format!("stream:{}", Self::generate_cache_key(payload))
	}

	/// Get a cached value
	pub async fn get_cache(&self, key: &str) -> Option<serde_json::Value> {
		let mut conn = self.conn.clone();
		match conn.get::<_, Option<String>>(key).await {
			Ok(Some(data)) => serde_json::from_str(&data).ok(),
			_ => None,
		}
	}

	/// Set a cached value with expiration
	pub async fn set_cache(
		&self,
		key: &str,
		value: &serde_json::Value,
		expiration_seconds: u64,
	) -> Result<(), redis::RedisError> {
		let mut conn = self.conn.clone();
		let serialized = serde_json::to_string(value).unwrap_or_default();
		conn.set_ex(key, serialized, expiration_seconds).await
	}

	/// Get streaming cache data
	pub async fn get_streaming_cache(&self, key: &str) -> Option<StreamingCacheData> {
		let mut conn = self.conn.clone();
		match conn.get::<_, Option<String>>(key).await {
			Ok(Some(data)) => serde_json::from_str(&data).ok(),
			_ => None,
		}
	}

	/// Set streaming cache data
	pub async fn set_streaming_cache(
		&self,
		key: &str,
		data: &StreamingCacheData,
		expiration_seconds: u64,
	) -> Result<(), redis::RedisError> {
		let mut conn = self.conn.clone();
		let serialized = serde_json::to_string(data).unwrap_or_default();
		conn.set_ex(key, serialized, expiration_seconds).await
	}

	/// Get a plain string value (for thought_signature, reasoning_content caching)
	pub async fn get_string(&self, key: &str) -> Option<String> {
		let mut conn = self.conn.clone();
		conn.get(key).await.ok().flatten()
	}

	/// Set a plain string value with expiration
	pub async fn set_string(
		&self,
		key: &str,
		value: &str,
		expiration_seconds: u64,
	) -> Result<(), redis::RedisError> {
		let mut conn = self.conn.clone();
		conn.set_ex(key, value, expiration_seconds).await
	}

	/// Increment a rate limit counter
	pub async fn increment_rate_limit(
		&self,
		key: &str,
		window_seconds: u64,
	) -> Result<i64, redis::RedisError> {
		let mut conn = self.conn.clone();
		let count: i64 = conn.incr(key, 1i64).await?;
		if count == 1 {
			let _: () = conn.expire(key, window_seconds as i64).await?;
		}
		Ok(count)
	}

	/// Get rate limit count
	pub async fn get_rate_limit_count(&self, key: &str) -> Result<i64, redis::RedisError> {
		let mut conn = self.conn.clone();
		let count: i64 = conn.get(key).await.unwrap_or(0);
		Ok(count)
	}

	/// Report key health (track API key errors)
	pub async fn report_key_error(
		&self,
		env_var_name: &str,
		config_index: i32,
	) -> Result<(), redis::RedisError> {
		let key = format!("api_key_health:{env_var_name}:{config_index}");
		let mut conn = self.conn.clone();
		let _: () = conn.incr(&key, 1i64).await?;
		let _: () = conn.expire(&key, 300).await?; // 5 min TTL
		Ok(())
	}

	/// Report key success
	pub async fn report_key_success(
		&self,
		env_var_name: &str,
		config_index: i32,
	) -> Result<(), redis::RedisError> {
		let key = format!("api_key_health:{env_var_name}:{config_index}");
		let mut conn = self.conn.clone();
		let _: () = conn.del(&key).await?;
		Ok(())
	}

	/// Graceful shutdown
	pub async fn quit(&self) -> Result<(), redis::RedisError> {
		// ConnectionManager handles connection lifecycle; drop is sufficient
		Ok(())
	}
}
