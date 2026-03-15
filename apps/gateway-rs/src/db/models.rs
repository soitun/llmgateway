use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

/// API key record from the database
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct ApiKey {
	pub id: String,
	pub token: String,
	pub description: String,
	pub status: Option<String>,
	pub usage_limit: Option<String>,
	pub usage: String,
	pub project_id: String,
	pub created_by: String,
	pub created_at: NaiveDateTime,
	pub updated_at: NaiveDateTime,
}

/// Project record
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Project {
	pub id: String,
	pub name: String,
	pub organization_id: String,
	pub caching_enabled: bool,
	pub cache_duration_seconds: i32,
	pub mode: String,
	pub status: Option<String>,
	pub created_at: NaiveDateTime,
	pub updated_at: NaiveDateTime,
}

/// Organization record
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Organization {
	pub id: String,
	pub name: String,
	pub billing_email: String,
	pub credits: String,
	pub plan: String,
	pub status: Option<String>,
	pub is_personal: bool,
	pub dev_plan: String,
	pub dev_plan_credits_used: String,
	pub dev_plan_credits_limit: String,
	pub dev_plan_expires_at: Option<NaiveDateTime>,
	pub dev_plan_allow_all_models: bool,
	pub retention_level: String,
	pub stripe_customer_id: Option<String>,
	pub stripe_subscription_id: Option<String>,
	pub created_at: NaiveDateTime,
	pub updated_at: NaiveDateTime,
}

impl Organization {
	pub fn total_credits(&self) -> f64 {
		let regular = self.credits.parse::<f64>().unwrap_or(0.0);

		let dev_plan_remaining = if self.dev_plan != "none" {
			let limit = self.dev_plan_credits_limit.parse::<f64>().unwrap_or(0.0);
			let used = self.dev_plan_credits_used.parse::<f64>().unwrap_or(0.0);
			limit - used
		} else {
			0.0
		};

		regular + dev_plan_remaining
	}
}

/// Provider key record
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct ProviderKey {
	pub id: String,
	pub token: String,
	pub provider: String,
	pub name: Option<String>,
	pub base_url: Option<String>,
	pub options: Option<serde_json::Value>,
	pub organization_id: String,
	pub status: String,
	pub created_at: NaiveDateTime,
	pub updated_at: NaiveDateTime,
}

/// IAM rule record
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct ApiKeyIamRule {
	pub id: String,
	pub api_key_id: String,
	pub rule_type: String,
	pub rule_value: serde_json::Value,
	pub status: String,
	pub created_at: NaiveDateTime,
	pub updated_at: NaiveDateTime,
}

/// User record
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct User {
	pub id: String,
	pub name: Option<String>,
	pub email: Option<String>,
	pub email_verified: Option<bool>,
	pub image: Option<String>,
	pub created_at: NaiveDateTime,
	pub updated_at: NaiveDateTime,
}

/// User organization membership
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct UserOrganization {
	pub id: String,
	pub user_id: String,
	pub organization_id: String,
	pub role: Option<String>,
	pub created_at: NaiveDateTime,
}

/// Provider metrics for routing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderMetrics {
	pub provider_id: String,
	pub model_id: String,
	pub uptime: Option<f64>,
	pub average_latency: Option<f64>,
	pub throughput: Option<f64>,
	pub total_requests: i64,
}

/// Discount record
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Discount {
	pub id: String,
	pub organization_id: Option<String>,
	pub provider: Option<String>,
	pub model: Option<String>,
	pub discount_percent: f64,
	pub expires_at: Option<NaiveDateTime>,
	pub created_at: NaiveDateTime,
}

/// Log entry for request tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
	pub id: String,
	pub request_id: String,
	pub organization_id: String,
	pub project_id: String,
	pub api_key_id: String,
	pub provider_key_id: Option<String>,
	pub requested_model: String,
	pub requested_provider: Option<String>,
	pub used_model: String,
	pub used_model_mapping: String,
	pub used_provider: String,
	pub duration: Option<i64>,
	pub time_to_first_token: Option<i64>,
	pub time_to_first_reasoning_token: Option<i64>,
	pub response_size: Option<i64>,
	pub content: Option<String>,
	pub reasoning_content: Option<String>,
	pub finish_reason: Option<String>,
	pub prompt_tokens: Option<String>,
	pub completion_tokens: Option<String>,
	pub total_tokens: Option<String>,
	pub reasoning_tokens: Option<String>,
	pub cached_tokens: Option<String>,
	pub has_error: bool,
	pub streamed: bool,
	pub canceled: bool,
	pub error_details: Option<serde_json::Value>,
	pub cost: f64,
	pub input_cost: Option<f64>,
	pub output_cost: Option<f64>,
	pub cached_input_cost: Option<f64>,
	pub request_cost: Option<f64>,
	pub web_search_cost: Option<f64>,
	pub image_input_tokens: Option<String>,
	pub image_output_tokens: Option<String>,
	pub image_input_cost: Option<f64>,
	pub image_output_cost: Option<f64>,
	pub estimated_cost: bool,
	pub discount: Option<f64>,
	pub pricing_tier: Option<String>,
	pub data_storage_cost: Option<String>,
	pub cached: bool,
	pub retried: Option<bool>,
	pub retried_by_log_id: Option<String>,
	pub messages: Option<serde_json::Value>,
	pub temperature: Option<f64>,
	pub max_tokens: Option<i32>,
	pub top_p: Option<f64>,
	pub frequency_penalty: Option<f64>,
	pub presence_penalty: Option<f64>,
	pub reasoning_effort: Option<String>,
	pub tools: Option<serde_json::Value>,
	pub tool_choice: Option<serde_json::Value>,
	pub tool_results: Option<serde_json::Value>,
	pub response_format: Option<serde_json::Value>,
	pub routing_metadata: Option<serde_json::Value>,
	pub source: Option<String>,
	pub custom_headers: Option<serde_json::Value>,
	pub user_agent: Option<String>,
	pub mode: Option<String>,
	pub raw_request: Option<serde_json::Value>,
	pub raw_response: Option<serde_json::Value>,
	pub upstream_request: Option<serde_json::Value>,
	pub upstream_response: Option<serde_json::Value>,
	pub plugins: Option<serde_json::Value>,
	pub plugin_results: Option<serde_json::Value>,
}
