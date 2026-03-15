use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::env;
use std::path::PathBuf;

use super::definition::*;

/// Global model registry loaded from JSON
static MODELS: Lazy<Vec<ModelDefinition>> = Lazy::new(|| load_models().unwrap_or_default());

/// Global provider registry loaded from JSON
static PROVIDERS: Lazy<Vec<ProviderDefinition>> =
	Lazy::new(|| load_providers().unwrap_or_default());

/// Load models from the TypeScript-generated JSON export
fn load_models() -> Result<Vec<ModelDefinition>, Box<dyn std::error::Error>> {
	// Try multiple locations for the models JSON
	let possible_paths = [
		PathBuf::from("models.json"),
		PathBuf::from("../gateway-rs/models.json"),
		PathBuf::from(
			env::var("MODELS_JSON_PATH").unwrap_or_else(|_| "models.json".to_string()),
		),
	];

	for path in &possible_paths {
		if path.exists() {
			let content = std::fs::read_to_string(path)?;
			match serde_json::from_str::<Vec<ModelDefinition>>(&content) {
				Ok(models) => return Ok(models),
				Err(e) => {
					eprintln!("Failed to parse models.json: {e}");
					return Err(e.into());
				}
			}
		}
	}

	eprintln!("No models.json found in: {:?}", possible_paths);
	Ok(vec![])
}

/// Load providers from JSON
fn load_providers() -> Result<Vec<ProviderDefinition>, Box<dyn std::error::Error>> {
	let possible_paths = [
		PathBuf::from("providers.json"),
		PathBuf::from("../gateway-rs/providers.json"),
		PathBuf::from(
			env::var("PROVIDERS_JSON_PATH").unwrap_or_else(|_| "providers.json".to_string()),
		),
	];

	for path in &possible_paths {
		if path.exists() {
			let content = std::fs::read_to_string(path)?;
			let providers: Vec<ProviderDefinition> = serde_json::from_str(&content)?;
			return Ok(providers);
		}
	}

	tracing::warn!("No providers.json found, using empty provider registry");
	Ok(vec![])
}

/// Get all models
pub fn get_models() -> &'static [ModelDefinition] {
	&MODELS
}

/// Get all providers
pub fn get_providers() -> &'static [ProviderDefinition] {
	&PROVIDERS
}

/// Find a model by ID
pub fn find_model(model_id: &str) -> Option<&'static ModelDefinition> {
	MODELS.iter().find(|m| {
		m.id == model_id
			|| m.aliases
				.as_ref()
				.is_some_and(|a| a.iter().any(|alias| alias == model_id))
	})
}

/// Find a model by provider model name
pub fn find_model_by_provider_name(provider_model_name: &str) -> Option<&'static ModelDefinition> {
	MODELS.iter().find(|m| {
		m.providers
			.iter()
			.any(|p| p.model_name == provider_model_name)
	})
}

/// Find a provider definition by ID
pub fn find_provider(provider_id: &str) -> Option<&'static ProviderDefinition> {
	PROVIDERS.iter().find(|p| p.id == provider_id)
}

/// Check if a provider has an environment token available
pub fn has_provider_environment_token(provider_id: &str) -> bool {
	let provider = find_provider(provider_id);
	if let Some(p) = provider {
		if let Some(ref env_config) = p.env {
			if let Some(api_key_var) = env_config.required.get("apiKey") {
				return env::var(api_key_var).is_ok();
			}
		}
	}
	false
}

/// Get provider environment variable value
pub fn get_provider_env_value(
	provider_id: &str,
	key: &str,
	config_index: usize,
) -> Option<String> {
	let provider = find_provider(provider_id)?;
	let env_config = provider.env.as_ref()?;

	let env_var_name = env_config
		.required
		.get(key)
		.or_else(|| env_config.optional.as_ref()?.get(key))?;

	let value = env::var(env_var_name).ok()?;

	// Support comma-separated values for round-robin
	let values: Vec<&str> = value.split(',').collect();
	if config_index < values.len() {
		Some(values[config_index].trim().to_string())
	} else {
		Some(values[0].trim().to_string())
	}
}

/// Get the environment token for a provider (with round-robin support)
pub fn get_provider_env(provider_id: &str) -> (Option<String>, usize, Option<String>) {
	let provider = find_provider(provider_id);
	if let Some(p) = provider {
		if let Some(ref env_config) = p.env {
			if let Some(api_key_var) = env_config.required.get("apiKey") {
				if let Ok(value) = env::var(api_key_var) {
					let values: Vec<&str> = value.split(',').collect();
					if values.len() > 1 {
						// Round-robin: pick based on a simple counter
						// In production, use an atomic counter for better distribution
						use std::sync::atomic::{AtomicUsize, Ordering};
						static COUNTER: AtomicUsize = AtomicUsize::new(0);
						let index = COUNTER.fetch_add(1, Ordering::Relaxed) % values.len();
						return (
							Some(values[index].trim().to_string()),
							index,
							Some(api_key_var.clone()),
						);
					}
					return (Some(value), 0, Some(api_key_var.clone()));
				}
			}
		}
	}
	(None, 0, None)
}

/// Get model streaming support
pub fn get_model_streaming_support(model_id: &str, provider_id: &str) -> Option<bool> {
	let model = find_model(model_id)?;
	let mapping = model
		.providers
		.iter()
		.find(|p| p.provider_id == provider_id)?;
	Some(mapping.streaming)
}

/// Build a map of provider IDs to their definitions for quick lookup
pub fn provider_map() -> HashMap<&'static str, &'static ProviderDefinition> {
	PROVIDERS.iter().map(|p| (p.id.as_str(), p)).collect()
}
