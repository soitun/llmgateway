use serde::{Deserialize, Serialize};

/// Chat completions request matching OpenAI's API
#[derive(Debug, Clone, Deserialize)]
pub struct CompletionsRequest {
	pub model: String,
	pub messages: Vec<serde_json::Value>,
	pub temperature: Option<f64>,
	pub max_tokens: Option<i64>,
	pub top_p: Option<f64>,
	pub frequency_penalty: Option<f64>,
	pub presence_penalty: Option<f64>,
	pub response_format: Option<serde_json::Value>,
	#[serde(default)]
	pub stream: bool,
	pub tools: Option<Vec<serde_json::Value>>,
	pub tool_choice: Option<serde_json::Value>,
	pub reasoning_effort: Option<String>,
	pub reasoning: Option<ReasoningConfig>,
	pub effort: Option<String>,
	#[serde(default)]
	pub free_models_only: bool,
	#[serde(default)]
	pub onboarding: bool,
	#[serde(default)]
	pub no_reasoning: bool,
	pub sensitive_word_check: Option<serde_json::Value>,
	pub image_config: Option<serde_json::Value>,
	#[serde(default)]
	pub web_search: bool,
	pub plugins: Option<Vec<PluginConfig>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReasoningConfig {
	pub effort: Option<String>,
	pub max_tokens: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PluginConfig {
	pub id: String,
}

/// OpenAI-compatible chat completion response
#[derive(Debug, Clone, Serialize)]
pub struct CompletionsResponse {
	pub id: String,
	pub object: String,
	pub created: i64,
	pub model: String,
	pub choices: Vec<Choice>,
	pub usage: Usage,
	pub metadata: ResponseMetadata,
}

#[derive(Debug, Clone, Serialize)]
pub struct Choice {
	pub index: u32,
	pub message: Message,
	pub finish_reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Message {
	pub role: String,
	pub content: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub reasoning: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub tool_calls: Option<Vec<serde_json::Value>>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub images: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Usage {
	pub prompt_tokens: i64,
	pub completion_tokens: i64,
	pub total_tokens: i64,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub reasoning_tokens: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub prompt_tokens_details: Option<PromptTokensDetails>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub cost_usd_total: Option<f64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub cost_usd_input: Option<f64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub cost_usd_output: Option<f64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub cost_usd_cached_input: Option<f64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub info: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub cost_usd_request: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PromptTokensDetails {
	pub cached_tokens: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResponseMetadata {
	pub requested_model: String,
	pub requested_provider: Option<String>,
	pub used_model: String,
	pub used_provider: String,
	pub underlying_used_model: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub routing: Option<Vec<serde_json::Value>>,
}
