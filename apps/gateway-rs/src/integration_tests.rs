#[cfg(test)]
mod selection_tests {
	use crate::providers::selection::*;
	use std::collections::{HashMap, HashSet};
	
	use crate::models::ProviderModelMapping;

	fn make_provider(id: &str, model: &str, input_price: f64, output_price: f64) -> ProviderModelMapping {
		ProviderModelMapping {
			provider_id: id.to_string(),
			model_name: model.to_string(),
			input_price: Some(input_price),
			output_price: Some(output_price),
			image_output_price: None,
			image_input_price: None,
			image_output_tokens_by_resolution: None,
			image_input_tokens_by_resolution: None,
			cached_input_price: None,
			min_cacheable_tokens: None,
			request_price: None,
			discount: None,
			pricing_tiers: None,
			context_size: Some(128000),
			max_output: Some(4096),
			streaming: true,
			vision: Some(false),
			reasoning: None,
			supports_responses_api: None,
			reasoning_output: None,
			reasoning_max_tokens: None,
			tools: Some(true),
			parallel_tool_calls: None,
			json_output: Some(true),
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

	#[test]
	fn test_is_retryable_error() {
		assert!(is_retryable_error(500));
		assert!(is_retryable_error(502));
		assert!(is_retryable_error(503));
		assert!(is_retryable_error(429));
		assert!(is_retryable_error(0));
		assert!(!is_retryable_error(200));
		assert!(!is_retryable_error(400));
		assert!(!is_retryable_error(401));
		assert!(!is_retryable_error(403));
		assert!(!is_retryable_error(404));
	}

	#[test]
	fn test_should_retry_request_no_retry_on_specific_provider() {
		assert!(!should_retry_request(
			Some("openai"),
			false,
			500,
			0,
			2,
			"openai",
		));
	}

	#[test]
	fn test_should_retry_request_no_retry_on_no_fallback() {
		assert!(!should_retry_request(
			None,
			true, // no_fallback
			500,
			0,
			2,
			"openai",
		));
	}

	#[test]
	fn test_should_retry_request_no_retry_on_client_error() {
		assert!(!should_retry_request(
			None,
			false,
			400, // client error
			0,
			2,
			"openai",
		));
	}

	#[test]
	fn test_should_retry_request_no_retry_max_retries() {
		assert!(!should_retry_request(
			None,
			false,
			500,
			MAX_RETRIES, // at max
			2,
			"openai",
		));
	}

	#[test]
	fn test_should_retry_request_no_retry_no_remaining() {
		assert!(!should_retry_request(
			None,
			false,
			500,
			0,
			0, // no remaining providers
			"openai",
		));
	}

	#[test]
	fn test_should_retry_request_no_retry_custom_provider() {
		assert!(!should_retry_request(
			None,
			false,
			500,
			0,
			2,
			"custom",
		));
	}

	#[test]
	fn test_should_retry_request_yes_retry() {
		assert!(should_retry_request(
			None,
			false,
			500,
			0,
			2,
			"openai",
		));
	}

	#[test]
	fn test_should_retry_on_rate_limit() {
		assert!(should_retry_request(
			None,
			false,
			429,
			0,
			2,
			"anthropic",
		));
	}

	#[test]
	fn test_get_error_type() {
		assert_eq!(get_error_type(0), "network_error");
		assert_eq!(get_error_type(429), "rate_limited");
		assert_eq!(get_error_type(500), "upstream_error");
		assert_eq!(get_error_type(502), "upstream_error");
	}

	#[test]
	fn test_select_next_provider_excludes_failed() {
		let scores = vec![
			ProviderScore {
				provider_id: "openai".to_string(),
				score: 0.5,
				price: 1.0,
				uptime: Some(99.0),
				latency: Some(100.0),
				throughput: Some(50.0),
				priority: None,
				failed: None,
				status_code: None,
				error_type: None,
			},
			ProviderScore {
				provider_id: "anthropic".to_string(),
				score: 0.3,
				price: 0.8,
				uptime: Some(98.0),
				latency: Some(120.0),
				throughput: Some(45.0),
				priority: None,
				failed: None,
				status_code: None,
				error_type: None,
			},
		];

		let mut failed = HashSet::new();
		failed.insert("anthropic".to_string());

		let providers = vec![
			make_provider("openai", "gpt-4", 0.03, 0.06),
			make_provider("anthropic", "claude-3", 0.025, 0.05),
		];

		let result = select_next_provider(&scores, &failed, &providers);
		assert!(result.is_some());
		assert_eq!(result.unwrap().0, "openai");
	}

	#[test]
	fn test_select_next_provider_returns_none_all_failed() {
		let scores = vec![
			ProviderScore {
				provider_id: "openai".to_string(),
				score: 0.5,
				price: 1.0,
				uptime: None,
				latency: None,
				throughput: None,
				priority: None,
				failed: None,
				status_code: None,
				error_type: None,
			},
		];

		let mut failed = HashSet::new();
		failed.insert("openai".to_string());

		let providers = vec![make_provider("openai", "gpt-4", 0.03, 0.06)];

		let result = select_next_provider(&scores, &failed, &providers);
		assert!(result.is_none());
	}

	#[test]
	fn test_provider_selection_cheapest() {
		let providers = vec![
			make_provider("openai", "gpt-4", 0.03, 0.06),
			make_provider("anthropic", "claude-3", 0.01, 0.02),
		];

		let metrics = HashMap::new();

		let result = get_cheapest_from_available_providers(
			&providers,
			"test-model",
			&metrics,
			false,
			false,
		);

		assert!(result.is_some());
		let r = result.unwrap();
		// Anthropic should be cheaper
		assert_eq!(r.provider.provider_id, "anthropic");
	}

	#[test]
	fn test_provider_selection_single_provider() {
		let providers = vec![
			make_provider("openai", "gpt-4", 0.03, 0.06),
		];

		let metrics = HashMap::new();

		let result = get_cheapest_from_available_providers(
			&providers,
			"test-model",
			&metrics,
			false,
			false,
		);

		assert!(result.is_some());
		assert_eq!(result.unwrap().provider.provider_id, "openai");
	}

	#[test]
	fn test_provider_selection_empty() {
		let providers: Vec<ProviderModelMapping> = vec![];
		let metrics = HashMap::new();

		let result = get_cheapest_from_available_providers(
			&providers,
			"test-model",
			&metrics,
			false,
			false,
		);

		assert!(result.is_none());
	}
}

#[cfg(test)]
mod endpoint_tests {
	use crate::providers::endpoint::get_provider_endpoint;

	#[test]
	fn test_openai_endpoint() {
		let url = get_provider_endpoint(
			"openai", None, "gpt-4", None, false, false, false, None, 0, false,
		);
		assert_eq!(url, Some("https://api.openai.com/v1/chat/completions".to_string()));
	}

	#[test]
	fn test_openai_responses_endpoint() {
		let url = get_provider_endpoint(
			"openai", None, "gpt-4", None, false, true, false, None, 0, false,
		);
		assert_eq!(url, Some("https://api.openai.com/v1/responses".to_string()));
	}

	#[test]
	fn test_openai_responses_with_tool_calls_falls_back() {
		let url = get_provider_endpoint(
			"openai", None, "gpt-4", None, false, true, true, None, 0, false,
		);
		assert_eq!(url, Some("https://api.openai.com/v1/chat/completions".to_string()));
	}

	#[test]
	fn test_anthropic_endpoint() {
		let url = get_provider_endpoint(
			"anthropic", None, "claude-3", None, false, false, false, None, 0, false,
		);
		assert_eq!(url, Some("https://api.anthropic.com/v1/messages".to_string()));
	}

	#[test]
	fn test_groq_endpoint() {
		let url = get_provider_endpoint(
			"groq", None, "llama-3", None, false, false, false, None, 0, false,
		);
		assert_eq!(url, Some("https://api.groq.com/openai/v1/chat/completions".to_string()));
	}

	#[test]
	fn test_deepseek_endpoint() {
		let url = get_provider_endpoint(
			"deepseek", None, "deepseek-chat", None, false, false, false, None, 0, false,
		);
		assert_eq!(url, Some("https://api.deepseek.com/chat/completions".to_string()));
	}

	#[test]
	fn test_xai_image_endpoint() {
		let url = get_provider_endpoint(
			"xai", None, "grok-2-image", None, false, false, false, None, 0, true,
		);
		assert_eq!(url, Some("https://api.x.ai/v1/images/generations".to_string()));
	}

	#[test]
	fn test_custom_base_url() {
		let url = get_provider_endpoint(
			"openai", Some("https://custom.api.com"), "gpt-4", None, false, false, false, None, 0, false,
		);
		assert_eq!(url, Some("https://custom.api.com/v1/chat/completions".to_string()));
	}

	#[test]
	fn test_custom_provider_with_base() {
		let url = get_provider_endpoint(
			"custom", Some("https://my-provider.com"), "my-model", None, false, false, false, None, 0, false,
		);
		assert_eq!(url, Some("https://my-provider.com/v1/chat/completions".to_string()));
	}

	#[test]
	fn test_custom_provider_without_base() {
		let url = get_provider_endpoint(
			"custom", None, "my-model", None, false, false, false, None, 0, false,
		);
		assert_eq!(url, None);
	}
}

#[cfg(test)]
mod headers_tests {
	use crate::providers::headers::get_provider_headers;

	#[test]
	fn test_openai_headers() {
		let headers = get_provider_headers("openai", "sk-test-123", false);
		assert_eq!(headers.get("Authorization").unwrap(), "Bearer sk-test-123");
		assert!(!headers.contains_key("x-api-key"));
	}

	#[test]
	fn test_anthropic_headers() {
		let headers = get_provider_headers("anthropic", "sk-ant-test", false);
		assert_eq!(headers.get("x-api-key").unwrap(), "sk-ant-test");
		assert_eq!(headers.get("anthropic-version").unwrap(), "2023-06-01");
		assert!(!headers.contains_key("Authorization"));
	}

	#[test]
	fn test_anthropic_headers_with_web_search() {
		let headers = get_provider_headers("anthropic", "sk-ant-test", true);
		let beta = headers.get("anthropic-beta").unwrap();
		assert!(beta.contains("web-search-2025-03-05"));
	}

	#[test]
	fn test_google_headers_empty() {
		let headers = get_provider_headers("google-ai-studio", "key", false);
		assert!(!headers.contains_key("Authorization"));
		assert!(!headers.contains_key("x-api-key"));
	}

	#[test]
	fn test_azure_headers() {
		let headers = get_provider_headers("azure", "azure-key", false);
		assert_eq!(headers.get("api-key").unwrap(), "azure-key");
		assert!(!headers.contains_key("Authorization"));
	}
}

#[cfg(test)]
mod body_tests {
	use crate::providers::body::prepare_request_body;
	use serde_json::json;

	#[test]
	fn test_openai_body_basic() {
		let messages = json!([{"role": "user", "content": "Hello"}]);
		let body = prepare_request_body(
			"openai", "gpt-4", &messages, false,
			Some(0.7), Some(1000), None, None, None,
			None, None, None, None, false, None, None, false, None, None, false,
		);

		assert_eq!(body["model"], "gpt-4");
		assert_eq!(body["temperature"], 0.7);
		assert_eq!(body["max_tokens"], 1000);
		assert!(body.get("stream").is_none() || body["stream"] == json!(null));
	}

	#[test]
	fn test_openai_body_streaming() {
		let messages = json!([{"role": "user", "content": "Hello"}]);
		let body = prepare_request_body(
			"openai", "gpt-4", &messages, true,
			None, None, None, None, None,
			None, None, None, None, false, None, None, false, None, None, false,
		);

		assert_eq!(body["stream"], true);
		assert_eq!(body["stream_options"]["include_usage"], true);
	}

	#[test]
	fn test_anthropic_body_basic() {
		let messages = json!([
			{"role": "system", "content": "You are helpful."},
			{"role": "user", "content": "Hello"}
		]);
		let body = prepare_request_body(
			"anthropic", "claude-3-sonnet", &messages, false,
			Some(0.7), Some(4096), None, None, None,
			None, None, None, None, false, None, None, false, None, None, false,
		);

		assert_eq!(body["model"], "claude-3-sonnet");
		assert_eq!(body["system"], "You are helpful.");
		assert_eq!(body["max_tokens"], 4096);
		assert_eq!(body["temperature"], 0.7);
		// Messages should not include the system message
		let msgs = body["messages"].as_array().unwrap();
		assert_eq!(msgs.len(), 1);
		assert_eq!(msgs[0]["role"], "user");
	}

	#[test]
	fn test_google_body_basic() {
		let messages = json!([{"role": "user", "content": "Hello"}]);
		let body = prepare_request_body(
			"google-ai-studio", "gemini-pro", &messages, false,
			Some(0.7), Some(1000), None, None, None,
			None, None, None, None, false, None, None, false, None, None, false,
		);

		assert!(body.get("contents").is_some());
		let config = &body["generationConfig"];
		assert_eq!(config["temperature"], 0.7);
		assert_eq!(config["maxOutputTokens"], 1000);
	}

	#[test]
	fn test_responses_api_body() {
		let messages = json!([{"role": "user", "content": "Hello"}]);
		let body = prepare_request_body(
			"openai", "gpt-4", &messages, false,
			None, None, None, None, None,
			None, None, None, Some("medium"), false, None, None, false, None, None, true,
		);

		assert_eq!(body["model"], "gpt-4");
		assert!(body.get("input").is_some());
		assert_eq!(body["reasoning"]["effort"], "medium");
	}
}

#[cfg(test)]
mod iam_tests {
	use crate::models::definition::{ModelDefinition, ProviderModelMapping};

	fn make_test_model() -> ModelDefinition {
		ModelDefinition {
			id: "gpt-4".to_string(),
			name: Some("GPT-4".to_string()),
			aliases: None,
			family: "openai".to_string(),
			providers: vec![
				ProviderModelMapping {
					provider_id: "openai".to_string(),
					model_name: "gpt-4".to_string(),
					input_price: Some(0.03),
					output_price: Some(0.06),
					streaming: true,
					vision: Some(false),
					tools: Some(true),
					json_output: Some(true),
					context_size: Some(128000),
					max_output: Some(4096),
					..Default::default()
				},
				ProviderModelMapping {
					provider_id: "azure".to_string(),
					model_name: "gpt-4".to_string(),
					input_price: Some(0.03),
					output_price: Some(0.06),
					streaming: true,
					vision: Some(false),
					tools: Some(true),
					json_output: Some(true),
					context_size: Some(128000),
					max_output: Some(4096),
					..Default::default()
				},
			],
			free: None,
			rate_limit_kind: None,
			output: None,
			image_input_required: None,
			stability: None,
			supports_system_role: None,
			description: None,
			released_at: None,
		}
	}

	#[test]
	fn test_model_is_free() {
		let mut model = make_test_model();
		assert!(!model.is_free());
		model.free = Some(true);
		assert!(model.is_free());
	}

	#[test]
	fn test_provider_capabilities() {
		let model = make_test_model();
		let provider = &model.providers[0];
		assert!(!provider.has_vision());
		assert!(provider.has_tools());
		assert!(provider.has_json_output());
		assert!(!provider.has_reasoning());
		assert!(!provider.has_web_search());
		assert!(!provider.is_deprecated());
		assert!(!provider.is_deactivated());
	}
}

#[cfg(test)]
mod cache_tests {
	use crate::cache::RedisCache;
	use serde_json::json;

	#[test]
	fn test_generate_cache_key_deterministic() {
		let payload = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
		let key1 = RedisCache::generate_cache_key(&payload);
		let key2 = RedisCache::generate_cache_key(&payload);
		assert_eq!(key1, key2);
	}

	#[test]
	fn test_generate_cache_key_different_payloads() {
		let payload1 = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
		let payload2 = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hello"}]});
		let key1 = RedisCache::generate_cache_key(&payload1);
		let key2 = RedisCache::generate_cache_key(&payload2);
		assert_ne!(key1, key2);
	}

	#[test]
	fn test_generate_streaming_cache_key() {
		let payload = json!({"model": "gpt-4"});
		let key = RedisCache::generate_streaming_cache_key(&payload);
		assert!(key.starts_with("stream:"));
	}
}

#[cfg(test)]
mod content_type_tests {
	use axum::{
		body::Body,
		http::{Method, Request, StatusCode},
		middleware,
		routing::{get, post},
		Router,
	};
	
	use tower::ServiceExt;

	use crate::middleware::content_type::validate_content_type;

	async fn dummy_handler() -> &'static str {
		"ok"
	}

	fn build_test_app() -> Router {
		Router::new()
			.route("/test", post(dummy_handler))
			.route("/mcp", post(dummy_handler))
			.route("/health", get(dummy_handler))
			.layer(middleware::from_fn(validate_content_type))
	}

	#[tokio::test]
	async fn test_post_without_content_type_returns_415() {
		let app = build_test_app();
		let response = app
			.oneshot(
				Request::builder()
					.method(Method::POST)
					.uri("/test")
					.body(Body::empty())
					.unwrap(),
			)
			.await
			.unwrap();

		assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
	}

	#[tokio::test]
	async fn test_post_with_json_content_type_passes() {
		let app = build_test_app();
		let response = app
			.oneshot(
				Request::builder()
					.method(Method::POST)
					.uri("/test")
					.header("content-type", "application/json")
					.body(Body::empty())
					.unwrap(),
			)
			.await
			.unwrap();

		assert_eq!(response.status(), StatusCode::OK);
	}

	#[tokio::test]
	async fn test_mcp_endpoint_skips_content_type_check() {
		let app = build_test_app();
		let response = app
			.oneshot(
				Request::builder()
					.method(Method::POST)
					.uri("/mcp")
					.body(Body::empty())
					.unwrap(),
			)
			.await
			.unwrap();

		assert_eq!(response.status(), StatusCode::OK);
	}

	#[tokio::test]
	async fn test_get_skips_content_type_check() {
		let app = build_test_app();
		let response = app
			.oneshot(
				Request::builder()
					.method(Method::GET)
					.uri("/health")
					.body(Body::empty())
					.unwrap(),
			)
			.await
			.unwrap();

		assert_eq!(response.status(), StatusCode::OK);
	}
}

#[cfg(test)]
mod json_parse_tests {
    use crate::models::definition::ModelDefinition;

    #[test]
    fn test_parse_models_json_file() {
        let path = std::path::Path::new("models.json");
        if !path.exists() {
            println!("models.json not found, skipping");
            return;
        }
        let content = std::fs::read_to_string(path).unwrap();
        match serde_json::from_str::<Vec<ModelDefinition>>(&content) {
            Ok(models) => {
                println!("Successfully parsed {} models", models.len());
                assert!(models.len() > 0, "Should have at least 1 model");
            }
            Err(e) => {
                // Show position
                let line = e.line();
                let col = e.column();
                let lines: Vec<&str> = content.lines().collect();
                if line > 0 && line <= lines.len() {
                    println!("Error at line {line}, col {col}:");
                    println!("  {}", lines[line - 1]);
                }
                panic!("Failed to parse models.json: {e}");
            }
        }
    }
}
