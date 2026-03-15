use std::collections::HashMap;

/// Construct provider-specific HTTP headers
pub fn get_provider_headers(
	provider: &str,
	token: &str,
	web_search_enabled: bool,
) -> HashMap<String, String> {
	let mut headers = HashMap::new();

	match provider {
		"anthropic" => {
			headers.insert("x-api-key".to_string(), token.to_string());
			headers.insert("anthropic-version".to_string(), "2023-06-01".to_string());

			// Build beta features list
			let mut beta_features = vec![
				"tools-2024-04-04",
				"prompt-caching-2024-07-31",
				"output-128k-2025-02-19",
				"token-counting-2024-11-01",
			];
			if web_search_enabled {
				beta_features.push("web-search-2025-03-05");
			}
			headers.insert("anthropic-beta".to_string(), beta_features.join(","));
		}

		"google-ai-studio" | "google-vertex" | "obsidian" => {
			// Google uses API key in URL query params, no auth header needed
		}

		"azure" => {
			headers.insert("api-key".to_string(), token.to_string());
		}

		"aws-bedrock" => {
			headers.insert(
				"Authorization".to_string(),
				format!("Bearer {token}"),
			);
			headers.insert("Content-Type".to_string(), "application/json".to_string());
		}

		_ => {
			// Standard Bearer token auth (OpenAI, Groq, xAI, DeepSeek, etc.)
			headers.insert(
				"Authorization".to_string(),
				format!("Bearer {token}"),
			);
		}
	}

	headers
}
