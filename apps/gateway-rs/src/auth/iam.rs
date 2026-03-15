use crate::db::find_active_iam_rules;
use crate::models::ModelDefinition;
use sqlx::PgPool;

/// Result of IAM validation
pub struct IamValidationResult {
	pub allowed: bool,
	pub reason: Option<String>,
	pub allowed_providers: Option<Vec<String>>,
}

/// Validate model access based on IAM rules
pub async fn validate_model_access(
	pool: &PgPool,
	api_key_id: &str,
	requested_model: &str,
	requested_provider: Option<&str>,
	model_def: Option<&ModelDefinition>,
) -> Result<IamValidationResult, sqlx::Error> {
	let iam_rules = find_active_iam_rules(pool, api_key_id).await?;

	let model_def = match model_def {
		Some(m) => m,
		None => {
			return Ok(IamValidationResult {
				allowed: false,
				reason: Some(format!("Model {requested_model} not found")),
				allowed_providers: None,
			});
		}
	};

	// If no rules exist, allow all access
	if iam_rules.is_empty() {
		return Ok(IamValidationResult {
			allowed: true,
			allowed_providers: Some(
				model_def
					.providers
					.iter()
					.map(|p| p.provider_id.clone())
					.collect(),
			),
			reason: None,
		});
	}

	let model_provider_ids: Vec<String> =
		model_def.providers.iter().map(|p| p.provider_id.clone()).collect();
	let mut allowed_providers: std::collections::HashSet<String> =
		model_provider_ids.iter().cloned().collect();

	for rule in &iam_rules {
		let rule_value: serde_json::Value =
			serde_json::from_value(rule.rule_value.clone()).unwrap_or_default();

		match rule.rule_type.as_str() {
			"allow_models" => {
				if let Some(models) = rule_value
					.get("models")
					.and_then(|v| v.as_array())
				{
					let model_ids: Vec<&str> = models
						.iter()
						.filter_map(|v| v.as_str())
						.collect();
					if !model_ids.contains(&model_def.id.as_str()) {
						return Ok(IamValidationResult {
							allowed: false,
							reason: Some(format!(
								"Model {} is not in the allowed models list. Adapt your LLMGateway API key IAM permissions in the dashboard or contact your LLMGateway API Key issuer. (Rule ID: {})",
								model_def.id, rule.id
							)),
							allowed_providers: None,
						});
					}
				}
			}

			"deny_models" => {
				if let Some(models) = rule_value
					.get("models")
					.and_then(|v| v.as_array())
				{
					let model_ids: Vec<&str> = models
						.iter()
						.filter_map(|v| v.as_str())
						.collect();
					if model_ids.contains(&model_def.id.as_str()) {
						return Ok(IamValidationResult {
							allowed: false,
							reason: Some(format!(
								"Model {} is in the denied models list. Adapt your LLMGateway API key IAM permissions in the dashboard or contact your LLMGateway API Key issuer. (Rule ID: {})",
								model_def.id, rule.id
							)),
							allowed_providers: None,
						});
					}
				}
			}

			"allow_providers" => {
				if let Some(providers) = rule_value
					.get("providers")
					.and_then(|v| v.as_array())
				{
					let provider_ids: Vec<&str> = providers
						.iter()
						.filter_map(|v| v.as_str())
						.collect();

					if let Some(rp) = requested_provider {
						if !provider_ids.contains(&rp) {
							return Ok(IamValidationResult {
								allowed: false,
								reason: Some(format!(
									"Provider {rp} is not in the allowed providers list. Adapt your LLMGateway API key IAM permissions in the dashboard or contact your LLMGateway API Key issuer. (Rule ID: {})",
									rule.id
								)),
								allowed_providers: None,
							});
						}
					}

					allowed_providers.retain(|p| provider_ids.contains(&p.as_str()));

					if allowed_providers.is_empty() {
						return Ok(IamValidationResult {
							allowed: false,
							reason: Some(format!(
								"None of the model's providers are in the allowed providers list. Adapt your LLMGateway API key IAM permissions in the dashboard or contact your LLMGateway API Key issuer. (Rule ID: {})",
								rule.id
							)),
							allowed_providers: None,
						});
					}
				}
			}

			"deny_providers" => {
				if let Some(providers) = rule_value
					.get("providers")
					.and_then(|v| v.as_array())
				{
					let provider_ids: Vec<&str> = providers
						.iter()
						.filter_map(|v| v.as_str())
						.collect();

					if let Some(rp) = requested_provider {
						if provider_ids.contains(&rp) {
							return Ok(IamValidationResult {
								allowed: false,
								reason: Some(format!(
									"Provider {rp} is in the denied providers list. Adapt your LLMGateway API key IAM permissions in the dashboard or contact your LLMGateway API Key issuer. (Rule ID: {})",
									rule.id
								)),
								allowed_providers: None,
							});
						}
					}

					allowed_providers.retain(|p| !provider_ids.contains(&p.as_str()));

					if allowed_providers.is_empty() {
						return Ok(IamValidationResult {
							allowed: false,
							reason: Some(format!(
								"All of the model's providers are in the denied providers list. Adapt your LLMGateway API key IAM permissions in the dashboard or contact your LLMGateway API Key issuer. (Rule ID: {})",
								rule.id
							)),
							allowed_providers: None,
						});
					}
				}
			}

			"allow_pricing" => {
				let is_free = model_def.is_free();
				if let Some(pricing_type) = rule_value
					.get("pricingType")
					.and_then(|v| v.as_str())
				{
					if pricing_type == "free" && !is_free {
						return Ok(IamValidationResult {
							allowed: false,
							reason: Some(format!(
								"Only free models are allowed. Adapt your LLMGateway API key IAM permissions in the dashboard or contact your LLMGateway API Key issuer. (Rule ID: {})",
								rule.id
							)),
							allowed_providers: None,
						});
					}
					if pricing_type == "paid" && is_free {
						return Ok(IamValidationResult {
							allowed: false,
							reason: Some(format!(
								"Only paid models are allowed. Adapt your LLMGateway API key IAM permissions in the dashboard or contact your LLMGateway API Key issuer. (Rule ID: {})",
								rule.id
							)),
							allowed_providers: None,
						});
					}
				}

				// Check price limits
				let max_input = rule_value.get("maxInputPrice").and_then(|v| v.as_f64());
				let max_output = rule_value.get("maxOutputPrice").and_then(|v| v.as_f64());

				for provider in &model_def.providers {
					if let Some(rp) = requested_provider {
						if provider.provider_id != rp {
							continue;
						}
					}

					if let Some(max_in) = max_input {
						if provider.input_price.unwrap_or(0.0) > max_in {
							return Ok(IamValidationResult {
								allowed: false,
								reason: Some(format!(
									"Model input price exceeds maximum allowed ({} > {}). Adapt your LLMGateway API key IAM permissions in the dashboard or contact your LLMGateway API Key issuer. (Rule ID: {})",
									provider.input_price.unwrap_or(0.0), max_in, rule.id
								)),
								allowed_providers: None,
							});
						}
					}

					if let Some(max_out) = max_output {
						if provider.output_price.unwrap_or(0.0) > max_out {
							return Ok(IamValidationResult {
								allowed: false,
								reason: Some(format!(
									"Model output price exceeds maximum allowed ({} > {}). Adapt your LLMGateway API key IAM permissions in the dashboard or contact your LLMGateway API Key issuer. (Rule ID: {})",
									provider.output_price.unwrap_or(0.0), max_out, rule.id
								)),
								allowed_providers: None,
							});
						}
					}
				}
			}

			"deny_pricing" => {
				let is_free = model_def.is_free();
				if let Some(pricing_type) = rule_value
					.get("pricingType")
					.and_then(|v| v.as_str())
				{
					if pricing_type == "free" && is_free {
						return Ok(IamValidationResult {
							allowed: false,
							reason: Some(format!(
								"Free models are not allowed. Adapt your LLMGateway API key IAM permissions in the dashboard or contact your LLMGateway API Key issuer. (Rule ID: {})",
								rule.id
							)),
							allowed_providers: None,
						});
					}
					if pricing_type == "paid" && !is_free {
						return Ok(IamValidationResult {
							allowed: false,
							reason: Some(format!(
								"Paid models are not allowed. Adapt your LLMGateway API key IAM permissions in the dashboard or contact your LLMGateway API Key issuer. (Rule ID: {})",
								rule.id
							)),
							allowed_providers: None,
						});
					}
				}
			}

			_ => {}
		}
	}

	if allowed_providers.is_empty() {
		return Ok(IamValidationResult {
			allowed: false,
			reason: Some(format!(
				"No providers are allowed for model {} due to IAM rules",
				requested_model
			)),
			allowed_providers: None,
		});
	}

	Ok(IamValidationResult {
		allowed: true,
		reason: None,
		allowed_providers: Some(allowed_providers.into_iter().collect()),
	})
}
