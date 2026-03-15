use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Stability level for models/providers
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum StabilityLevel {
	Stable,
	Beta,
	Unstable,
	Experimental,
}

/// Pricing tier for context-length-based pricing
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PricingTier {
	pub name: String,
	pub up_to_tokens: f64, // Can be Infinity
	pub input_price: f64,
	pub output_price: f64,
	pub cached_input_price: Option<f64>,
}

/// Provider-specific model mapping
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModelMapping {
	pub provider_id: String,
	pub model_name: String,
	pub input_price: Option<f64>,
	pub output_price: Option<f64>,
	pub image_output_price: Option<f64>,
	pub image_input_price: Option<f64>,
	pub image_output_tokens_by_resolution: Option<std::collections::HashMap<String, u64>>,
	pub image_input_tokens_by_resolution: Option<std::collections::HashMap<String, u64>>,
	pub cached_input_price: Option<f64>,
	pub min_cacheable_tokens: Option<u64>,
	pub request_price: Option<f64>,
	pub discount: Option<f64>,
	pub pricing_tiers: Option<Vec<PricingTier>>,
	pub context_size: Option<u64>,
	pub max_output: Option<u64>,
	pub streaming: bool,
	pub vision: Option<bool>,
	pub reasoning: Option<bool>,
	pub supports_responses_api: Option<bool>,
	pub reasoning_output: Option<String>,
	pub reasoning_max_tokens: Option<bool>,
	pub tools: Option<bool>,
	pub parallel_tool_calls: Option<bool>,
	pub json_output: Option<bool>,
	pub json_output_schema: Option<bool>,
	pub web_search: Option<bool>,
	pub web_search_price: Option<f64>,
	pub supported_parameters: Option<Vec<String>>,
	pub test: Option<String>,
	pub stability: Option<StabilityLevel>,
	pub deprecated_at: Option<DateTime<Utc>>,
	pub deactivated_at: Option<DateTime<Utc>>,
	pub image_generations: Option<bool>,
	pub priority: Option<f64>,
}

impl Default for ProviderModelMapping {
	fn default() -> Self {
		Self {
			provider_id: String::new(),
			model_name: String::new(),
			input_price: None,
			output_price: None,
			image_output_price: None,
			image_input_price: None,
			image_output_tokens_by_resolution: None,
			image_input_tokens_by_resolution: None,
			cached_input_price: None,
			min_cacheable_tokens: None,
			request_price: None,
			discount: None,
			pricing_tiers: None,
			context_size: None,
			max_output: None,
			streaming: false,
			vision: None,
			reasoning: None,
			supports_responses_api: None,
			reasoning_output: None,
			reasoning_max_tokens: None,
			tools: None,
			parallel_tool_calls: None,
			json_output: None,
			json_output_schema: None,
			web_search: None,
			web_search_price: None,
			supported_parameters: None,
			test: None,
			stability: None,
			deprecated_at: None,
			deactivated_at: None,
			image_generations: None,
			priority: None,
		}
	}
}

impl ProviderModelMapping {
	pub fn has_vision(&self) -> bool {
		self.vision.unwrap_or(false)
	}

	pub fn has_reasoning(&self) -> bool {
		self.reasoning.unwrap_or(false)
	}

	pub fn has_tools(&self) -> bool {
		self.tools.unwrap_or(false)
	}

	pub fn has_json_output(&self) -> bool {
		self.json_output.unwrap_or(false)
	}

	pub fn has_json_output_schema(&self) -> bool {
		self.json_output_schema.unwrap_or(false)
	}

	pub fn has_web_search(&self) -> bool {
		self.web_search.unwrap_or(false)
	}

	pub fn has_image_generations(&self) -> bool {
		self.image_generations.unwrap_or(false)
	}

	pub fn is_deprecated(&self) -> bool {
		self.deprecated_at
			.map(|d| Utc::now() > d)
			.unwrap_or(false)
	}

	pub fn is_deactivated(&self) -> bool {
		self.deactivated_at
			.map(|d| Utc::now() > d)
			.unwrap_or(false)
	}
}

/// Full model definition
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelDefinition {
	pub id: String,
	pub name: Option<String>,
	pub aliases: Option<Vec<String>>,
	pub family: String,
	pub providers: Vec<ProviderModelMapping>,
	pub free: Option<bool>,
	pub rate_limit_kind: Option<String>,
	pub output: Option<Vec<String>>,
	pub image_input_required: Option<bool>,
	pub stability: Option<StabilityLevel>,
	pub supports_system_role: Option<bool>,
	pub description: Option<String>,
	pub released_at: Option<DateTime<Utc>>,
}

impl ModelDefinition {
	pub fn is_free(&self) -> bool {
		self.free.unwrap_or(false)
	}

	/// Check if model is truly free (all providers have zero pricing)
	pub fn is_truly_free(&self) -> bool {
		if self.free == Some(true) {
			return true;
		}
		self.providers.iter().all(|p| {
			p.input_price.unwrap_or(0.0) == 0.0 && p.output_price.unwrap_or(0.0) == 0.0
		})
	}
}

/// Provider definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderDefinition {
	pub id: String,
	pub name: String,
	pub description: Option<String>,
	pub streaming: bool,
	pub cancellation: bool,
	pub color: Option<String>,
	pub website: Option<String>,
	pub priority: Option<f64>,
	pub env: Option<ProviderEnvConfig>,
}

/// Provider environment configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderEnvConfig {
	pub required: std::collections::HashMap<String, String>,
	pub optional: Option<std::collections::HashMap<String, String>>,
}

/// Web search tool configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSearchTool {
	#[serde(rename = "type")]
	pub tool_type: String,
	pub user_location: Option<serde_json::Value>,
	pub search_context_size: Option<String>,
	pub max_uses: Option<u32>,
}
