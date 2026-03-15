use crate::db::ProviderMetrics;
use crate::models::ProviderModelMapping;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Routing metadata included in the response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingMetadata {
	pub available_providers: Vec<String>,
	pub selected_provider: String,
	pub selection_reason: String,
	pub provider_scores: Vec<ProviderScore>,
	pub original_provider: Option<String>,
	pub original_provider_uptime: Option<f64>,
	pub no_fallback: Option<bool>,
	pub routing: Option<Vec<RoutingAttempt>>,
}

/// Score for a single provider
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderScore {
	pub provider_id: String,
	pub score: f64,
	pub price: f64,
	pub uptime: Option<f64>,
	pub latency: Option<f64>,
	pub throughput: Option<f64>,
	pub priority: Option<f64>,
	pub failed: Option<bool>,
	pub status_code: Option<u16>,
	pub error_type: Option<String>,
}

/// Record of a routing attempt (success or failure)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingAttempt {
	pub provider: String,
	pub model: String,
	pub status_code: u16,
	pub error_type: String,
	pub succeeded: bool,
}

/// Result of provider selection
pub struct ProviderSelectionResult {
	pub provider: ProviderModelMapping,
	pub metadata: RoutingMetadata,
}

// Scoring weights
const PRICE_WEIGHT: f64 = 0.3;
const UPTIME_WEIGHT: f64 = 0.5;
const THROUGHPUT_WEIGHT: f64 = 0.05;
const LATENCY_WEIGHT: f64 = 0.025;
const IMAGE_PRICE_WEIGHT: f64 = 0.5;

/// Maximum retries for provider fallback
pub const MAX_RETRIES: usize = 2;

/// Select the cheapest/best provider from available providers using multi-factor scoring
pub fn get_cheapest_from_available_providers(
	available_providers: &[ProviderModelMapping],
	model_id: &str,
	metrics_map: &HashMap<String, ProviderMetrics>,
	is_streaming: bool,
	is_image_model: bool,
) -> Option<ProviderSelectionResult> {
	if available_providers.is_empty() {
		return None;
	}

	// Filter out unstable/experimental providers
	let stable_providers: Vec<&ProviderModelMapping> = available_providers
		.iter()
		.filter(|p| {
			!matches!(
				p.stability,
				Some(crate::models::StabilityLevel::Unstable)
					| Some(crate::models::StabilityLevel::Experimental)
			)
		})
		.collect();

	let providers_to_score = if stable_providers.is_empty() {
		available_providers.iter().collect::<Vec<_>>()
	} else {
		stable_providers
	};

	// Epsilon-greedy exploration (1% random) - disabled in test mode
	#[cfg(test)]
	let explore = false;
	#[cfg(not(test))]
	let explore = rand_simple() < 0.01;

	if explore && providers_to_score.len() > 1 {
		let random_index = (rand_simple() * providers_to_score.len() as f64) as usize;
		let selected = providers_to_score[random_index].clone();
		let scores = build_provider_scores(providers_to_score.as_slice(), model_id, metrics_map);

		return Some(ProviderSelectionResult {
			provider: selected,
			metadata: RoutingMetadata {
				available_providers: providers_to_score
					.iter()
					.map(|p| p.provider_id.clone())
					.collect(),
				selected_provider: providers_to_score[random_index].provider_id.clone(),
				selection_reason: "random-exploration".to_string(),
				provider_scores: scores,
				original_provider: None,
				original_provider_uptime: None,
				no_fallback: None,
				routing: None,
			},
		});
	}

	// Score each provider
	let mut best_score = f64::MAX;
	let mut best_provider: Option<&ProviderModelMapping> = None;
	let mut all_scores = Vec::new();

	let price_weight = if is_image_model {
		IMAGE_PRICE_WEIGHT
	} else {
		PRICE_WEIGHT
	};

	// Compute normalization ranges
	let prices: Vec<f64> = providers_to_score
		.iter()
		.map(|p| p.input_price.unwrap_or(0.0) + p.output_price.unwrap_or(0.0))
		.collect();
	let max_price = prices.iter().cloned().fold(f64::MIN, f64::max);
	let min_price = prices.iter().cloned().fold(f64::MAX, f64::min);
	let price_range = if (max_price - min_price).abs() < f64::EPSILON {
		1.0
	} else {
		max_price - min_price
	};

	for provider in &providers_to_score {
		let key = format!("{}:{}", model_id, provider.provider_id);
		let metrics = metrics_map.get(&key);

		let price = provider.input_price.unwrap_or(0.0) + provider.output_price.unwrap_or(0.0);
		let normalized_price = (price - min_price) / price_range;

		let uptime = metrics.and_then(|m| m.uptime).unwrap_or(100.0);
		let latency = metrics.and_then(|m| m.average_latency).unwrap_or(0.0);
		let throughput = metrics.and_then(|m| m.throughput).unwrap_or(0.0);

		// Normalized uptime (higher is better, invert for scoring where lower is better)
		let uptime_score = 1.0 - (uptime / 100.0);

		// Exponential penalty for uptime < 95%
		let uptime_penalty = if uptime < 95.0 {
			((95.0 - uptime) / 10.0).powi(2)
		} else {
			0.0
		};

		// Provider priority (lower value = higher priority = lower score)
		let priority = provider.priority.unwrap_or(1.0);

		let score = (normalized_price * price_weight)
			+ (uptime_score * UPTIME_WEIGHT)
			+ uptime_penalty
			+ (latency / 10000.0 * LATENCY_WEIGHT)
			+ ((1.0 - throughput / 1000.0).max(0.0) * THROUGHPUT_WEIGHT)
			+ (priority - 1.0) * 0.1;

		all_scores.push(ProviderScore {
			provider_id: provider.provider_id.clone(),
			score,
			price,
			uptime: Some(uptime),
			latency: Some(latency),
			throughput: Some(throughput),
			priority: Some(priority),
			failed: None,
			status_code: None,
			error_type: None,
		});

		if score < best_score {
			best_score = score;
			best_provider = Some(provider);
		}
	}

	let has_metrics = metrics_map.values().any(|m| m.total_requests > 0);
	let selection_reason = if has_metrics {
		"weighted-score"
	} else if !prices.iter().all(|p| (*p - prices[0]).abs() < f64::EPSILON) {
		"price-only"
	} else {
		"price-only-no-metrics"
	};

	best_provider.map(|provider| ProviderSelectionResult {
		provider: (*provider).clone(),
		metadata: RoutingMetadata {
			available_providers: providers_to_score
				.iter()
				.map(|p| p.provider_id.clone())
				.collect(),
			selected_provider: provider.provider_id.clone(),
			selection_reason: selection_reason.to_string(),
			provider_scores: all_scores,
			original_provider: None,
			original_provider_uptime: None,
			no_fallback: None,
			routing: None,
		},
	})
}

fn build_provider_scores(
	providers: &[&ProviderModelMapping],
	model_id: &str,
	metrics_map: &HashMap<String, ProviderMetrics>,
) -> Vec<ProviderScore> {
	providers
		.iter()
		.map(|p| {
			let key = format!("{model_id}:{}", p.provider_id);
			let metrics = metrics_map.get(&key);
			let price = p.input_price.unwrap_or(0.0) + p.output_price.unwrap_or(0.0);

			ProviderScore {
				provider_id: p.provider_id.clone(),
				score: 0.0,
				price,
				uptime: metrics.and_then(|m| m.uptime),
				latency: metrics.and_then(|m| m.average_latency),
				throughput: metrics.and_then(|m| m.throughput),
				priority: p.priority,
				failed: None,
				status_code: None,
				error_type: None,
			}
		})
		.collect()
}

/// Check if an HTTP status code is retryable
pub fn is_retryable_error(status_code: u16) -> bool {
	status_code == 429 || status_code >= 500 || status_code == 0
}

/// Determine whether a failed request should be retried with a different provider
pub fn should_retry_request(
	requested_provider: Option<&str>,
	no_fallback: bool,
	status_code: u16,
	retry_count: usize,
	remaining_providers: usize,
	used_provider: &str,
) -> bool {
	if requested_provider.is_some() {
		return false;
	}
	if no_fallback {
		return false;
	}
	if !is_retryable_error(status_code) {
		return false;
	}
	if retry_count >= MAX_RETRIES {
		return false;
	}
	if remaining_providers == 0 {
		return false;
	}
	if used_provider == "custom" || used_provider == "llmgateway" {
		return false;
	}
	true
}

/// Select the next best provider, excluding failed ones
pub fn select_next_provider(
	provider_scores: &[ProviderScore],
	failed_providers: &std::collections::HashSet<String>,
	model_providers: &[ProviderModelMapping],
) -> Option<(String, String)> {
	let mut sorted = provider_scores.to_vec();
	sorted.sort_by(|a, b| a.score.partial_cmp(&b.score).unwrap_or(std::cmp::Ordering::Equal));

	for score in &sorted {
		if failed_providers.contains(&score.provider_id) {
			continue;
		}
		if let Some(mapping) = model_providers
			.iter()
			.find(|p| p.provider_id == score.provider_id)
		{
			return Some((mapping.provider_id.clone(), mapping.model_name.clone()));
		}
	}

	None
}

/// Map HTTP status code to error type string
pub fn get_error_type(status_code: u16) -> String {
	match status_code {
		0 => "network_error".to_string(),
		429 => "rate_limited".to_string(),
		_ => "upstream_error".to_string(),
	}
}

/// Simple pseudo-random for exploration (not crypto-quality, just for epsilon-greedy)
fn rand_simple() -> f64 {
	use std::time::SystemTime;
	let nanos = SystemTime::now()
		.duration_since(SystemTime::UNIX_EPOCH)
		.unwrap_or_default()
		.subsec_nanos();
	(nanos % 10000) as f64 / 10000.0
}
