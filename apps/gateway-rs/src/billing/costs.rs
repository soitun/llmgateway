use crate::models::registry;
use rust_decimal::prelude::*;
use rust_decimal::Decimal;

/// Cost calculation result
#[derive(Debug, Clone)]
pub struct CostResult {
	pub input_cost: Option<f64>,
	pub output_cost: Option<f64>,
	pub cached_input_cost: Option<f64>,
	pub request_cost: Option<f64>,
	pub web_search_cost: Option<f64>,
	pub image_input_tokens: Option<i64>,
	pub image_output_tokens: Option<i64>,
	pub image_input_cost: Option<f64>,
	pub image_output_cost: Option<f64>,
	pub total_cost: Option<f64>,
	pub prompt_tokens: Option<i64>,
	pub completion_tokens: Option<i64>,
	pub cached_tokens: Option<i64>,
	pub estimated_cost: bool,
	pub discount: Option<f64>,
	pub pricing_tier: Option<String>,
}

impl Default for CostResult {
	fn default() -> Self {
		Self {
			input_cost: None,
			output_cost: None,
			cached_input_cost: None,
			request_cost: None,
			web_search_cost: None,
			image_input_tokens: None,
			image_output_tokens: None,
			image_input_cost: None,
			image_output_cost: None,
			total_cost: None,
			prompt_tokens: None,
			completion_tokens: None,
			cached_tokens: None,
			estimated_cost: false,
			discount: None,
			pricing_tier: None,
		}
	}
}

/// Calculate costs based on model, provider, and token counts
pub async fn calculate_costs(
	pool: &sqlx::PgPool,
	model: &str,
	provider: &str,
	prompt_tokens: Option<i64>,
	completion_tokens: Option<i64>,
	cached_tokens: Option<i64>,
	reasoning_tokens: Option<i64>,
	output_image_count: i64,
	image_size: Option<&str>,
	input_image_count: i64,
	web_search_count: Option<i64>,
	organization_id: Option<&str>,
) -> CostResult {
	let model_info = registry::find_model(model)
		.or_else(|| registry::find_model_by_provider_name(model));

	let model_info = match model_info {
		Some(m) => m,
		None => {
			return CostResult {
				prompt_tokens,
				completion_tokens,
				cached_tokens,
				..Default::default()
			};
		}
	};

	let provider_info = model_info
		.providers
		.iter()
		.find(|p| p.provider_id == provider);

	let provider_info = match provider_info {
		Some(p) => p,
		None => {
			return CostResult {
				prompt_tokens,
				completion_tokens,
				cached_tokens,
				..Default::default()
			};
		}
	};

	let calculated_prompt_tokens = prompt_tokens.unwrap_or(0);
	let calculated_completion_tokens = completion_tokens.unwrap_or(0);

	if calculated_prompt_tokens == 0 {
		return CostResult {
			prompt_tokens: Some(calculated_prompt_tokens),
			completion_tokens: Some(calculated_completion_tokens),
			cached_tokens,
			..Default::default()
		};
	}

	// Get pricing (possibly tiered)
	let (input_price, output_price, cached_input_price_val, tier_name) =
		get_pricing_for_token_count(
			&provider_info.pricing_tiers,
			provider_info.input_price.unwrap_or(0.0),
			provider_info.output_price.unwrap_or(0.0),
			provider_info.cached_input_price,
			calculated_prompt_tokens,
		);

	let input_price = Decimal::from_f64(input_price).unwrap_or(Decimal::ZERO);
	let output_price = Decimal::from_f64(output_price).unwrap_or(Decimal::ZERO);
	let cached_input_price = Decimal::from_f64(cached_input_price_val).unwrap_or(input_price);
	let request_price =
		Decimal::from_f64(provider_info.request_price.unwrap_or(0.0)).unwrap_or(Decimal::ZERO);

	// Get discount
	let hardcoded_discount = provider_info.discount.unwrap_or(0.0);
	let (discount, _source) = crate::db::get_effective_discount(
		pool,
		organization_id,
		provider,
		model,
		hardcoded_discount,
		Some(&provider_info.model_name),
	)
	.await
	.unwrap_or((0.0, "none".to_string()));

	let discount_multiplier = Decimal::ONE - Decimal::from_f64(discount).unwrap_or(Decimal::ZERO);

	// Calculate image input cost
	let mut image_input_tokens: Option<i64> = None;
	let mut image_input_cost: Option<Decimal> = None;
	if let Some(img_input_price) = provider_info.image_input_price {
		if input_image_count > 0 {
			let tokens_per_image = provider_info
				.image_input_tokens_by_resolution
				.as_ref()
				.and_then(|m| {
					m.get(image_size.unwrap_or("default"))
						.or_else(|| m.get("default"))
				})
				.copied()
				.unwrap_or(560);

			let tokens = input_image_count * tokens_per_image as i64;
			image_input_tokens = Some(tokens);
			image_input_cost = Some(
				Decimal::from(tokens)
					* Decimal::from_f64(img_input_price).unwrap_or(Decimal::ZERO)
					* discount_multiplier,
			);
		}
	}

	// Calculate input cost
	let uncached_prompt_tokens = cached_tokens
		.map(|ct| calculated_prompt_tokens - ct)
		.unwrap_or(calculated_prompt_tokens);

	let input_cost = Decimal::from(uncached_prompt_tokens) * input_price * discount_multiplier
		+ image_input_cost.unwrap_or(Decimal::ZERO);

	// Calculate output cost
	let is_google = provider == "google-ai-studio"
		|| provider == "google-vertex"
		|| provider == "obsidian";
	let total_output_tokens = if is_google {
		calculated_completion_tokens
	} else {
		calculated_completion_tokens + reasoning_tokens.unwrap_or(0)
	};

	let mut image_output_tokens: Option<i64> = None;
	let mut image_output_cost: Option<Decimal> = None;
	let output_cost;

	if let Some(img_output_price) = provider_info.image_output_price {
		if output_image_count > 0 {
			let tokens_per_image = provider_info
				.image_output_tokens_by_resolution
				.as_ref()
				.and_then(|m| {
					m.get(image_size.unwrap_or("default"))
						.or_else(|| m.get("default"))
				})
				.copied()
				.unwrap_or(1120);

			let img_tokens = output_image_count * tokens_per_image as i64;
			image_output_tokens = Some(img_tokens);
			let text_tokens = (total_output_tokens - img_tokens).max(0);

			let img_cost = Decimal::from(img_tokens)
				* Decimal::from_f64(img_output_price).unwrap_or(Decimal::ZERO)
				* discount_multiplier;
			image_output_cost = Some(img_cost);
			output_cost =
				Decimal::from(text_tokens) * output_price * discount_multiplier + img_cost;
		} else {
			output_cost = Decimal::from(total_output_tokens) * output_price * discount_multiplier;
		}
	} else {
		output_cost = Decimal::from(total_output_tokens) * output_price * discount_multiplier;
	}

	let cached_input_cost = cached_tokens
		.map(|ct| Decimal::from(ct) * cached_input_price * discount_multiplier)
		.unwrap_or(Decimal::ZERO);

	let request_cost_val = request_price * discount_multiplier;

	// Web search cost
	let web_search_price = Decimal::from_f64(provider_info.web_search_price.unwrap_or(0.0))
		.unwrap_or(Decimal::ZERO);
	let web_search_cost_val = web_search_count
		.map(|c| Decimal::from(c) * web_search_price * discount_multiplier)
		.unwrap_or(Decimal::ZERO);

	let total_cost =
		input_cost + output_cost + cached_input_cost + request_cost_val + web_search_cost_val;

	// Adjust prompt tokens for Google image input
	let final_prompt_tokens =
		if image_input_tokens.is_some() && is_google {
			Some(calculated_prompt_tokens + image_input_tokens.unwrap_or(0))
		} else {
			Some(calculated_prompt_tokens)
		};

	CostResult {
		input_cost: Some(input_cost.to_f64().unwrap_or(0.0)),
		output_cost: Some(output_cost.to_f64().unwrap_or(0.0)),
		cached_input_cost: Some(cached_input_cost.to_f64().unwrap_or(0.0)),
		request_cost: Some(request_cost_val.to_f64().unwrap_or(0.0)),
		web_search_cost: Some(web_search_cost_val.to_f64().unwrap_or(0.0)),
		image_input_tokens,
		image_output_tokens,
		image_input_cost: image_input_cost.and_then(|c| c.to_f64()),
		image_output_cost: image_output_cost.and_then(|c| c.to_f64()),
		total_cost: Some(total_cost.to_f64().unwrap_or(0.0)),
		prompt_tokens: final_prompt_tokens,
		completion_tokens: Some(calculated_completion_tokens),
		cached_tokens,
		estimated_cost: false,
		discount: if discount != 0.0 {
			Some(discount)
		} else {
			None
		},
		pricing_tier: tier_name,
	}
}

fn get_pricing_for_token_count(
	pricing_tiers: &Option<Vec<crate::models::PricingTier>>,
	base_input: f64,
	base_output: f64,
	base_cached_input: Option<f64>,
	prompt_tokens: i64,
) -> (f64, f64, f64, Option<String>) {
	if let Some(tiers) = pricing_tiers {
		if !tiers.is_empty() {
			for tier in tiers {
				if (prompt_tokens as f64) <= tier.up_to_tokens {
					return (
						tier.input_price,
						tier.output_price,
						tier.cached_input_price.unwrap_or(base_cached_input.unwrap_or(base_input)),
						Some(tier.name.clone()),
					);
				}
			}
			// Fallback to last tier
			let last = &tiers[tiers.len() - 1];
			return (
				last.input_price,
				last.output_price,
				last.cached_input_price.unwrap_or(base_cached_input.unwrap_or(base_input)),
				Some(last.name.clone()),
			);
		}
	}

	(
		base_input,
		base_output,
		base_cached_input.unwrap_or(base_input),
		None,
	)
}

/// Calculate data storage cost based on token counts and retention level
pub fn calculate_data_storage_cost(
	prompt_tokens: Option<i64>,
	cached_tokens: Option<i64>,
	completion_tokens: Option<i64>,
	reasoning_tokens: Option<i64>,
	retention_level: &str,
) -> String {
	if retention_level != "retain" {
		return "0".to_string();
	}

	let total_tokens = prompt_tokens.unwrap_or(0)
		+ cached_tokens.unwrap_or(0)
		+ completion_tokens.unwrap_or(0)
		+ reasoning_tokens.unwrap_or(0);

	// $0.01 per 1M tokens
	let cost = Decimal::from(total_tokens) * Decimal::from_f64(0.00000001).unwrap_or(Decimal::ZERO);
	cost.to_string()
}
