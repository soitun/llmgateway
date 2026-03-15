use axum::{extract::Query, http::StatusCode, Json};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::models::{
	definition::ModelDefinition,
	registry,
};

#[derive(Deserialize)]
pub struct ModelsQuery {
	pub include_deactivated: Option<String>,
	pub exclude_deprecated: Option<String>,
}

#[derive(Serialize)]
pub struct ModelsResponse {
	pub data: Vec<serde_json::Value>,
}

/// GET /v1/models - List available models
pub async fn list_models(
	Query(query): Query<ModelsQuery>,
) -> Result<Json<ModelsResponse>, (StatusCode, Json<serde_json::Value>)> {
	let include_deactivated = query
		.include_deactivated
		.as_deref()
		== Some("true");
	let exclude_deprecated = query
		.exclude_deprecated
		.as_deref()
		== Some("true");
	let current_date = Utc::now();

	let all_models = registry::get_models();
	let providers = registry::get_providers();

	let filtered_models: Vec<&ModelDefinition> = all_models
		.iter()
		.filter(|model| {
			let all_deactivated = model.providers.iter().all(|p| p.is_deactivated());
			if !include_deactivated && all_deactivated {
				return false;
			}

			let all_deprecated = model.providers.iter().all(|p| p.is_deprecated());
			if exclude_deprecated && all_deprecated {
				return false;
			}

			true
		})
		.collect();

	let model_data: Vec<serde_json::Value> = filtered_models
		.iter()
		.map(|model| {
			let mut input_modalities = vec!["text"];
			if model.providers.iter().any(|p| p.has_vision()) {
				input_modalities.push("image");
			}

			let output_modalities = model
				.output
				.as_ref()
				.map(|o| o.iter().map(|s| s.as_str()).collect::<Vec<_>>())
				.unwrap_or_else(|| vec!["text"]);

			let first_provider = model
				.providers
				.iter()
				.find(|p| p.input_price.is_some() || p.output_price.is_some());

			let input_price = first_provider
				.and_then(|p| p.input_price)
				.unwrap_or(0.0)
				.to_string();
			let output_price = first_provider
				.and_then(|p| p.output_price)
				.unwrap_or(0.0)
				.to_string();
			let image_price = first_provider
				.and_then(|p| p.image_input_price)
				.unwrap_or(0.0)
				.to_string();

			let provider_data: Vec<serde_json::Value> = model
				.providers
				.iter()
				.map(|provider| {
					let provider_def = providers.iter().find(|p| p.id == provider.provider_id);

					json!({
						"providerId": provider.provider_id,
						"modelName": provider.model_name,
						"pricing": if provider.input_price.is_some() || provider.output_price.is_some() {
							Some(json!({
								"prompt": provider.input_price.unwrap_or(0.0).to_string(),
								"completion": provider.output_price.unwrap_or(0.0).to_string(),
								"image": provider.image_input_price.unwrap_or(0.0).to_string(),
							}))
						} else {
							None
						},
						"streaming": provider.streaming,
						"vision": provider.has_vision(),
						"cancellation": provider_def.map(|p| p.cancellation).unwrap_or(false),
						"tools": provider.has_tools(),
						"parallelToolCalls": provider.parallel_tool_calls.unwrap_or(false),
						"reasoning": provider.has_reasoning(),
						"stability": provider.stability.as_ref().or(model.stability.as_ref()),
					})
				})
				.collect();

			let context_length = model
				.providers
				.iter()
				.filter_map(|p| p.context_size)
				.max()
				.unwrap_or(0);

			let supported_parameters = get_supported_parameters(model);

			json!({
				"id": model.id,
				"name": model.name.as_deref().unwrap_or(&model.id),
				"aliases": model.aliases,
				"created": Utc::now().timestamp(),
				"description": format!("{} provided by {}", model.id,
					model.providers.iter().map(|p| p.provider_id.as_str()).collect::<Vec<_>>().join(", ")),
				"family": model.family,
				"architecture": {
					"input_modalities": input_modalities,
					"output_modalities": output_modalities,
					"tokenizer": "GPT",
				},
				"top_provider": {
					"is_moderated": true,
				},
				"providers": provider_data,
				"pricing": {
					"prompt": input_price,
					"completion": output_price,
					"image": image_price,
					"request": first_provider.and_then(|p| p.request_price).unwrap_or(0.0).to_string(),
					"input_cache_read": first_provider.and_then(|p| p.cached_input_price).unwrap_or(0.0).to_string(),
					"input_cache_write": "0",
					"web_search": "0",
					"internal_reasoning": "0",
				},
				"context_length": context_length,
				"supported_parameters": supported_parameters,
				"json_output": model.providers.iter().any(|p| p.has_json_output()),
				"structured_outputs": model.providers.iter().any(|p| p.has_json_output_schema()),
				"free": model.is_free(),
				"stability": model.stability,
			})
		})
		.collect();

	Ok(Json(ModelsResponse { data: model_data }))
}

fn get_supported_parameters(model: &ModelDefinition) -> Vec<String> {
	// Check if any provider defines explicit supported parameters
	for provider in &model.providers {
		if let Some(ref params) = provider.supported_parameters {
			if !params.is_empty() {
				let mut result = params.clone();
				if model.providers.iter().any(|p| p.has_reasoning()) && !result.contains(&"reasoning".to_string()) {
					result.push("reasoning".to_string());
				}
				return result;
			}
		}
	}

	let is_anthropic = model.family == "anthropic";
	let params = if is_anthropic {
		vec![
			"temperature", "max_tokens", "top_p", "response_format", "tools", "tool_choice",
		]
	} else {
		vec![
			"temperature",
			"max_tokens",
			"top_p",
			"frequency_penalty",
			"presence_penalty",
			"response_format",
			"tools",
			"tool_choice",
		]
	};

	let mut result: Vec<String> = params.iter().map(|s| s.to_string()).collect();
	if model.providers.iter().any(|p| p.has_reasoning()) {
		result.push("reasoning".to_string());
	}
	result
}
