use axum::{
	extract::State,
	http::{HeaderMap, StatusCode},
	response::{
		sse::{KeepAlive, Sse},
		IntoResponse, Response,
	},
	Json,
};
use chrono::Utc;
use serde_json::json;
use std::time::Duration;

use crate::app::AppState;
use crate::auth::iam;
use crate::db;
use crate::error::openai_error;
use crate::models::registry;
use crate::providers::{body, endpoint, headers, selection};
use crate::streaming;

use super::schema::*;

/// POST /v1/chat/completions - Main chat completions handler
pub async fn chat_completions(
	State(state): State<AppState>,
	headers_map: HeaderMap,
	body: String,
) -> Response {
	// Parse JSON body
	let raw_body: serde_json::Value = match serde_json::from_str(&body) {
		Ok(v) => v,
		Err(_) => {
			return openai_error(
				StatusCode::BAD_REQUEST,
				"Invalid JSON in request body",
				"invalid_request_error",
				"invalid_json",
			);
		}
	};

	// Validate against schema
	let request: CompletionsRequest = match serde_json::from_value(raw_body.clone()) {
		Ok(r) => r,
		Err(_) => {
			return openai_error(
				StatusCode::BAD_REQUEST,
				"Invalid request parameters",
				"invalid_request_error",
				"invalid_parameters",
			);
		}
	};

	// Generate request ID
	let request_id = headers_map
		.get("x-request-id")
		.and_then(|v| v.to_str().ok())
		.map(|s| s.to_string())
		.unwrap_or_else(|| nanoid::nanoid!(40));

	// Extract auth token
	let token = extract_token(&headers_map);
	let token = match token {
		Some(t) => t,
		None => {
			return openai_error(
				StatusCode::UNAUTHORIZED,
				"Unauthorized: No API key provided. Expected 'Authorization: Bearer your-api-token' header or 'x-api-key: your-api-token' header",
				"authentication_error",
				"missing_api_key",
			);
		}
	};

	// Validate API key
	let api_key = match db::find_api_key_by_token(&state.db, &token).await {
		Ok(Some(key)) => key,
		Ok(None) => {
			return openai_error(
				StatusCode::UNAUTHORIZED,
				"Unauthorized: Invalid LLMGateway API token. Please make sure the token is not deleted or disabled. Go to the LLMGateway 'API Keys' page to generate a new token.",
				"authentication_error",
				"invalid_api_key",
			);
		}
		Err(e) => {
			tracing::error!("Database error looking up API key: {e}");
			return openai_error(
				StatusCode::INTERNAL_SERVER_ERROR,
				"Internal Server Error",
				"server_error",
				"database_error",
			);
		}
	};

	if api_key.status.as_deref() != Some("active") {
		return openai_error(
			StatusCode::UNAUTHORIZED,
			"Unauthorized: Invalid LLMGateway API token. Please make sure the token is not deleted or disabled.",
			"authentication_error",
			"invalid_api_key",
		);
	}

	// Check usage limit
	if let Some(ref limit) = api_key.usage_limit {
		let limit_val = limit.parse::<f64>().unwrap_or(f64::MAX);
		let usage_val = api_key.usage.parse::<f64>().unwrap_or(0.0);
		if usage_val >= limit_val {
			return openai_error(
				StatusCode::UNAUTHORIZED,
				"Unauthorized: LLMGateway API key reached its usage limit.",
				"authentication_error",
				"usage_limit_exceeded",
			);
		}
	}

	// Load project
	let project = match db::find_project_by_id(&state.db, &api_key.project_id).await {
		Ok(Some(p)) => p,
		Ok(None) => {
			return openai_error(
				StatusCode::INTERNAL_SERVER_ERROR,
				"Could not find project",
				"server_error",
				"project_not_found",
			);
		}
		Err(e) => {
			tracing::error!("Database error loading project: {e}");
			return openai_error(
				StatusCode::INTERNAL_SERVER_ERROR,
				"Internal Server Error",
				"server_error",
				"database_error",
			);
		}
	};

	if project.status.as_deref() == Some("deleted") {
		return openai_error(
			StatusCode::GONE,
			"Project has been archived and is no longer accessible",
			"invalid_request_error",
			"project_archived",
		);
	}

	// Load organization
	let organization = match db::find_organization_by_id(&state.db, &project.organization_id).await
	{
		Ok(Some(o)) => o,
		Ok(None) => {
			return openai_error(
				StatusCode::INTERNAL_SERVER_ERROR,
				"Could not find organization",
				"server_error",
				"organization_not_found",
			);
		}
		Err(e) => {
			tracing::error!("Database error loading organization: {e}");
			return openai_error(
				StatusCode::INTERNAL_SERVER_ERROR,
				"Internal Server Error",
				"server_error",
				"database_error",
			);
		}
	};

	// Parse model input (may include provider prefix like "openai/gpt-4")
	let (requested_model, requested_provider, custom_provider_name) =
		parse_model_input(&request.model);

	// Resolve model info
	let model_info = match registry::find_model(&requested_model) {
		Some(m) => m,
		None => {
			return openai_error(
				StatusCode::BAD_REQUEST,
				&format!("Model not found: {requested_model}"),
				"invalid_request_error",
				"model_not_found",
			);
		}
	};

	// Resolve reasoning effort
	let reasoning_effort = request
		.reasoning
		.as_ref()
		.and_then(|r| r.effort.as_deref())
		.or(request.reasoning_effort.as_deref())
		.and_then(|e| if e == "none" { None } else { Some(e.to_string()) });

	let reasoning_max_tokens = request
		.reasoning
		.as_ref()
		.and_then(|r| r.max_tokens);

	// Validate IAM
	let iam_result = match iam::validate_model_access(
		&state.db,
		&api_key.id,
		&requested_model,
		requested_provider.as_deref(),
		Some(model_info),
	)
	.await
	{
		Ok(r) => r,
		Err(e) => {
			tracing::error!("IAM validation error: {e}");
			return openai_error(
				StatusCode::INTERNAL_SERVER_ERROR,
				"Internal Server Error",
				"server_error",
				"iam_error",
			);
		}
	};

	if !iam_result.allowed {
		return openai_error(
			StatusCode::FORBIDDEN,
			&format!("Access denied: {}", iam_result.reason.unwrap_or_default()),
			"permission_error",
			"access_denied",
		);
	}

	// Select provider
	let mut used_provider = requested_provider.clone().unwrap_or_default();
	let mut used_model = requested_model.clone();

	// If no provider specified, select best one
	if used_provider.is_empty() {
		let available_providers = &iam_result
			.allowed_providers
			.as_ref()
			.map(|ap| {
				model_info
					.providers
					.iter()
					.filter(|p| ap.contains(&p.provider_id))
					.collect::<Vec<_>>()
			})
			.unwrap_or_else(|| model_info.providers.iter().collect());

		if available_providers.is_empty() {
			return openai_error(
				StatusCode::BAD_REQUEST,
				&format!("No available provider for model {used_model}"),
				"invalid_request_error",
				"no_provider",
			);
		}

		if available_providers.len() == 1 {
			used_provider = available_providers[0].provider_id.clone();
			used_model = available_providers[0].model_name.clone();
		} else {
			// Use provider selection algorithm
			let owned_providers: Vec<_> =
				available_providers.iter().map(|p| (*p).clone()).collect();
			let metrics_map = db::get_provider_metrics_for_combinations(
				&state.db,
				&owned_providers
					.iter()
					.map(|p| (model_info.id.clone(), p.provider_id.clone()))
					.collect::<Vec<_>>(),
			)
			.await
			.unwrap_or_default();

			let result = selection::get_cheapest_from_available_providers(
				&owned_providers,
				&model_info.id,
				&metrics_map,
				request.stream,
				false,
			);

			if let Some(r) = result {
				used_provider = r.provider.provider_id.clone();
				used_model = r.provider.model_name.clone();
			} else {
				used_provider = available_providers[0].provider_id.clone();
				used_model = available_providers[0].model_name.clone();
			}
		}
	} else {
		// Specific provider requested, find the model name
		if let Some(mapping) = model_info
			.providers
			.iter()
			.find(|p| p.provider_id == used_provider)
		{
			used_model = mapping.model_name.clone();
		}
	}

	// Resolve provider token
	let (provider_token, config_index, env_var_name) =
		resolve_provider_token(&state, &project, &organization, &used_provider, custom_provider_name.as_deref())
			.await;

	let provider_token = match provider_token {
		Some(t) => t,
		None => {
			return openai_error(
				StatusCode::INTERNAL_SERVER_ERROR,
				"No token",
				"server_error",
				"no_token",
			);
		}
	};

	// Get provider endpoint
	let supports_reasoning = model_info
		.providers
		.iter()
		.find(|p| p.provider_id == used_provider && p.model_name == used_model)
		.map(|p| p.has_reasoning())
		.unwrap_or(false);

	let has_existing_tool_calls = request.messages.iter().any(|msg| {
		msg.get("tool_calls").is_some() || msg.get("role").and_then(|v| v.as_str()) == Some("tool")
	});

	let url = endpoint::get_provider_endpoint(
		&used_provider,
		None,
		&used_model,
		if used_provider == "google-ai-studio" || used_provider == "google-vertex" {
			Some(&provider_token)
		} else {
			None
		},
		request.stream,
		supports_reasoning,
		has_existing_tool_calls,
		None,
		config_index,
		false,
	);

	let url = match url {
		Some(u) => u,
		None => {
			return openai_error(
				StatusCode::BAD_REQUEST,
				&format!("No base URL set for provider: {used_provider}"),
				"invalid_request_error",
				"no_base_url",
			);
		}
	};

	let use_responses_api = url.contains("/responses");

	// Build request body
	let web_search_tool = if request.web_search {
		Some(crate::models::WebSearchTool {
			tool_type: "web_search".to_string(),
			user_location: None,
			search_context_size: None,
			max_uses: None,
		})
	} else {
		None
	};

	let request_body = body::prepare_request_body(
		&used_provider,
		&used_model,
		&json!(request.messages),
		request.stream,
		request.temperature,
		request.max_tokens,
		request.top_p,
		request.frequency_penalty,
		request.presence_penalty,
		request.response_format.as_ref(),
		request.tools.as_ref().map(|t| json!(t)).as_ref(),
		request.tool_choice.as_ref(),
		reasoning_effort.as_deref(),
		supports_reasoning,
		request.image_config.as_ref(),
		request.effort.as_deref(),
		false,
		web_search_tool.as_ref(),
		reasoning_max_tokens,
		use_responses_api,
	);

	// Build headers
	let mut provider_headers = headers::get_provider_headers(
		&used_provider,
		&provider_token,
		request.web_search,
	);
	provider_headers.insert("Content-Type".to_string(), "application/json".to_string());

	let base_model_name = model_info.id.clone();
	let used_model_formatted = format!("{used_provider}/{base_model_name}");

	let start_time = std::time::Instant::now();

	// Make request to upstream provider
	let client = reqwest::Client::new();
	let mut req_builder = client
		.post(&url)
		.timeout(Duration::from_millis(
			if request.stream {
				state.config.ai_streaming_timeout_ms
			} else {
				state.config.ai_timeout_ms
			},
		))
		.body(serde_json::to_string(&request_body).unwrap_or_default());

	for (key, value) in &provider_headers {
		req_builder = req_builder.header(key, value);
	}

	// Set response headers
	let mut response_headers = HeaderMap::new();
	if let Ok(val) = request_id.parse() {
		response_headers.insert("x-request-id", val);
	}

	if request.stream {
		// Streaming response
		handle_streaming_response(
			req_builder,
			response_headers,
			&state,
			&request_id,
			&used_provider,
			&used_model,
			&used_model_formatted,
			&base_model_name,
			&request,
			start_time,
		)
		.await
	} else {
		// Non-streaming response
		handle_non_streaming_response(
			req_builder,
			response_headers,
			&state,
			&request_id,
			&used_provider,
			&used_model,
			&used_model_formatted,
			&base_model_name,
			&request,
			start_time,
		)
		.await
	}
}

/// Handle streaming SSE response
async fn handle_streaming_response(
	req_builder: reqwest::RequestBuilder,
	response_headers: HeaderMap,
	state: &AppState,
	request_id: &str,
	used_provider: &str,
	used_model: &str,
	used_model_formatted: &str,
	base_model_name: &str,
	request: &CompletionsRequest,
	start_time: std::time::Instant,
) -> Response {
	let (tx, sse_stream) = streaming::create_sse_stream();

	let state = state.clone();
	let request_id = request_id.to_string();
	let used_provider = used_provider.to_string();
	let used_model = used_model.to_string();
	let used_model_formatted = used_model_formatted.to_string();
	let base_model_name = base_model_name.to_string();
	let requested_model = request.model.clone();

	// Spawn task to handle the upstream response
	tokio::spawn(async move {
		let mut event_id: u64 = 0;

		match req_builder.send().await {
			Ok(response) => {
				if !response.status().is_success() {
					let status = response.status().as_u16();
					let error_text = response.text().await.unwrap_or_default();
					let _ = streaming::send_sse_error(
						&tx,
						&json!({
							"error": {
								"message": format!("Upstream provider error ({}): {}", status, error_text),
								"type": "upstream_error",
								"code": status.to_string(),
							}
						})
						.to_string(),
						event_id,
					)
					.await;
					let _ = streaming::send_sse_done(&tx).await;
					return;
				}

				// Stream the response
				let mut stream = response.bytes_stream();
				use futures::StreamExt;
				let mut buffer = String::new();

				while let Some(chunk_result) = stream.next().await {
					match chunk_result {
						Ok(chunk) => {
							let text = String::from_utf8_lossy(&chunk);
							buffer.push_str(&text);

							// Process complete SSE lines
							while let Some(line_end) = buffer.find("\n\n") {
								let event_data = buffer[..line_end].to_string();
								buffer = buffer[line_end + 2..].to_string();

								// Extract data from SSE format
								for line in event_data.lines() {
									if let Some(data) = line.strip_prefix("data: ") {
										if data == "[DONE]" {
											let _ = streaming::send_sse_done(&tx).await;
											return;
										}
										let _ = streaming::send_sse_data(
											&tx, data, event_id,
										)
										.await;
										event_id += 1;
									}
								}
							}
						}
						Err(e) => {
							tracing::error!("Error reading stream: {e}");
							let _ = streaming::send_sse_error(
								&tx,
								&json!({
									"error": {
										"message": format!("Stream read error: {e}"),
										"type": "stream_error",
										"code": "stream_error",
									}
								})
								.to_string(),
								event_id,
							)
							.await;
							break;
						}
					}
				}

				// Send final done if we haven't already
				let _ = streaming::send_sse_done(&tx).await;
			}
			Err(e) => {
				if e.is_timeout() {
					let _ = streaming::send_sse_error(
						&tx,
						&json!({
							"error": {
								"message": "Upstream provider timeout",
								"type": "upstream_timeout",
								"code": "timeout",
							}
						})
						.to_string(),
						event_id,
					)
					.await;
				} else {
					let _ = streaming::send_sse_error(
						&tx,
						&json!({
							"error": {
								"message": format!("Upstream request failed: {e}"),
								"type": "upstream_error",
								"code": "request_failed",
							}
						})
						.to_string(),
						event_id,
					)
					.await;
				}
				let _ = streaming::send_sse_done(&tx).await;
			}
		}
	});

	// Return SSE response
	let sse = Sse::new(sse_stream).keep_alive(
		KeepAlive::new()
			.interval(Duration::from_secs(15))
			.text("ping"),
	);

	let mut response = sse.into_response();
	let headers = response.headers_mut();
	for (key, value) in response_headers.iter() {
		headers.insert(key.clone(), value.clone());
	}

	response
}

/// Handle non-streaming JSON response
async fn handle_non_streaming_response(
	req_builder: reqwest::RequestBuilder,
	response_headers: HeaderMap,
	state: &AppState,
	request_id: &str,
	used_provider: &str,
	used_model: &str,
	used_model_formatted: &str,
	base_model_name: &str,
	request: &CompletionsRequest,
	start_time: std::time::Instant,
) -> Response {
	match req_builder.send().await {
		Ok(response) => {
			let status = response.status();
			let response_text = response.text().await.unwrap_or_default();

			if !status.is_success() {
				// Try to parse the error response
				if let Ok(error_json) = serde_json::from_str::<serde_json::Value>(&response_text) {
					return (
						StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
						Json(error_json),
					)
						.into_response();
				}
				return openai_error(
					StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
					&format!("Upstream provider error: {response_text}"),
					"upstream_error",
					&status.as_u16().to_string(),
				);
			}

			// Parse upstream response and transform to OpenAI format
			let duration = start_time.elapsed().as_millis() as i64;

			match serde_json::from_str::<serde_json::Value>(&response_text) {
				Ok(upstream_response) => {
					// Transform non-OpenAI responses to OpenAI format
					let transformed = transform_response_to_openai(
						&upstream_response,
						used_provider,
						used_model,
						base_model_name,
						&request.model,
						request
							.messages
							.last()
							.and_then(|m| m.get("role").and_then(|r| r.as_str()))
							.unwrap_or("user"),
					);

					(StatusCode::OK, Json(transformed)).into_response()
				}
				Err(_) => {
					openai_error(
						StatusCode::INTERNAL_SERVER_ERROR,
						"Failed to parse upstream response",
						"server_error",
						"parse_error",
					)
				}
			}
		}
		Err(e) => {
			if e.is_timeout() {
				openai_error(
					StatusCode::GATEWAY_TIMEOUT,
					"Gateway Timeout",
					"timeout_error",
					"timeout",
				)
			} else {
				openai_error(
					StatusCode::BAD_GATEWAY,
					&format!("Upstream request failed: {e}"),
					"upstream_error",
					"request_failed",
				)
			}
		}
	}
}

/// Transform upstream response to OpenAI format
fn transform_response_to_openai(
	response: &serde_json::Value,
	provider: &str,
	used_model: &str,
	base_model_name: &str,
	requested_model: &str,
	_last_role: &str,
) -> serde_json::Value {
	match provider {
		"anthropic" => transform_anthropic_response(response, base_model_name, requested_model),
		"google-ai-studio" | "google-vertex" | "obsidian" => {
			transform_google_response(response, base_model_name, requested_model)
		}
		_ => {
			// OpenAI-compatible, add metadata
			let mut result = response.clone();
			if let Some(obj) = result.as_object_mut() {
				obj.insert(
					"metadata".to_string(),
					json!({
						"requested_model": requested_model,
						"requested_provider": serde_json::Value::Null,
						"used_model": format!("{provider}/{base_model_name}"),
						"used_provider": provider,
						"underlying_used_model": used_model,
					}),
				);
			}
			result
		}
	}
}

fn transform_anthropic_response(
	response: &serde_json::Value,
	base_model_name: &str,
	requested_model: &str,
) -> serde_json::Value {
	let mut content = String::new();
	let mut reasoning = String::new();
	let mut tool_calls = vec![];

	if let Some(content_blocks) = response.get("content").and_then(|c| c.as_array()) {
		for block in content_blocks {
			match block.get("type").and_then(|t| t.as_str()) {
				Some("text") => {
					if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
						content.push_str(text);
					}
				}
				Some("thinking") => {
					if let Some(text) = block.get("thinking").and_then(|t| t.as_str()) {
						reasoning.push_str(text);
					}
				}
				Some("tool_use") => {
					tool_calls.push(json!({
						"id": block.get("id"),
						"type": "function",
						"function": {
							"name": block.get("name"),
							"arguments": serde_json::to_string(
								block.get("input").unwrap_or(&json!({}))
							).unwrap_or_default(),
						}
					}));
				}
				_ => {}
			}
		}
	}

	let usage = response.get("usage");
	let prompt_tokens = usage
		.and_then(|u| u.get("input_tokens"))
		.and_then(|v| v.as_i64())
		.unwrap_or(0);
	let completion_tokens = usage
		.and_then(|u| u.get("output_tokens"))
		.and_then(|v| v.as_i64())
		.unwrap_or(0);

	let finish_reason = match response.get("stop_reason").and_then(|v| v.as_str()) {
		Some("end_turn") => "stop",
		Some("tool_use") => "tool_calls",
		Some("max_tokens") => "length",
		Some(other) => other,
		None => "stop",
	};

	let mut message = json!({
		"role": "assistant",
		"content": if content.is_empty() { serde_json::Value::Null } else { json!(content) },
	});

	if !reasoning.is_empty() {
		message["reasoning"] = json!(reasoning);
	}
	if !tool_calls.is_empty() {
		message["tool_calls"] = json!(tool_calls);
	}

	json!({
		"id": response.get("id").and_then(|v| v.as_str()).unwrap_or(""),
		"object": "chat.completion",
		"created": Utc::now().timestamp(),
		"model": base_model_name,
		"choices": [{
			"index": 0,
			"message": message,
			"finish_reason": finish_reason,
		}],
		"usage": {
			"prompt_tokens": prompt_tokens,
			"completion_tokens": completion_tokens,
			"total_tokens": prompt_tokens + completion_tokens,
		},
		"metadata": {
			"requested_model": requested_model,
			"requested_provider": serde_json::Value::Null,
			"used_model": format!("anthropic/{base_model_name}"),
			"used_provider": "anthropic",
			"underlying_used_model": base_model_name,
		},
	})
}

fn transform_google_response(
	response: &serde_json::Value,
	base_model_name: &str,
	requested_model: &str,
) -> serde_json::Value {
	let mut content = String::new();
	let mut reasoning = String::new();

	if let Some(candidates) = response.get("candidates").and_then(|c| c.as_array()) {
		if let Some(candidate) = candidates.first() {
			if let Some(parts) = candidate
				.get("content")
				.and_then(|c| c.get("parts"))
				.and_then(|p| p.as_array())
			{
				for part in parts {
					if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
						if part.get("thought").and_then(|t| t.as_bool()).unwrap_or(false) {
							reasoning.push_str(text);
						} else {
							content.push_str(text);
						}
					}
				}
			}
		}
	}

	let usage = response.get("usageMetadata");
	let prompt_tokens = usage
		.and_then(|u| u.get("promptTokenCount"))
		.and_then(|v| v.as_i64())
		.unwrap_or(0);
	let completion_tokens = usage
		.and_then(|u| u.get("candidatesTokenCount"))
		.and_then(|v| v.as_i64())
		.unwrap_or(0);

	let mut message = json!({
		"role": "assistant",
		"content": if content.is_empty() { serde_json::Value::Null } else { json!(content) },
	});

	if !reasoning.is_empty() {
		message["reasoning"] = json!(reasoning);
	}

	json!({
		"id": format!("chatcmpl-{}", nanoid::nanoid!(24)),
		"object": "chat.completion",
		"created": Utc::now().timestamp(),
		"model": base_model_name,
		"choices": [{
			"index": 0,
			"message": message,
			"finish_reason": "stop",
		}],
		"usage": {
			"prompt_tokens": prompt_tokens,
			"completion_tokens": completion_tokens,
			"total_tokens": prompt_tokens + completion_tokens,
		},
		"metadata": {
			"requested_model": requested_model,
			"requested_provider": serde_json::Value::Null,
			"used_model": format!("google-ai-studio/{base_model_name}"),
			"used_provider": "google-ai-studio",
			"underlying_used_model": base_model_name,
		},
	})
}

// --- Helper functions ---

pub fn extract_token(headers: &HeaderMap) -> Option<String> {
	// Check Authorization header
	if let Some(auth) = headers.get("authorization").and_then(|v| v.to_str().ok()) {
		if let Some(token) = auth.strip_prefix("Bearer ") {
			return Some(token.to_string());
		}
	}

	// Check x-api-key header
	if let Some(key) = headers.get("x-api-key").and_then(|v| v.to_str().ok()) {
		return Some(key.to_string());
	}

	None
}

pub fn parse_model_input(model_input: &str) -> (String, Option<String>, Option<String>) {
	// Check for custom provider format: "custom:providerName/modelName"
	if let Some(rest) = model_input.strip_prefix("custom:") {
		if let Some(slash_pos) = rest.find('/') {
			let provider_name = &rest[..slash_pos];
			let model_name = &rest[slash_pos + 1..];
			return (
				model_name.to_string(),
				Some("custom".to_string()),
				Some(provider_name.to_string()),
			);
		}
	}

	// Check for provider/model format
	if let Some(slash_pos) = model_input.find('/') {
		let provider = &model_input[..slash_pos];
		let model = &model_input[slash_pos + 1..];

		// Verify it's a known provider
		if registry::find_provider(provider).is_some() || provider == "llmgateway" {
			return (model.to_string(), Some(provider.to_string()), None);
		}
	}

	// Plain model name
	(model_input.to_string(), None, None)
}

async fn resolve_provider_token(
	state: &AppState,
	project: &db::Project,
	organization: &db::Organization,
	provider: &str,
	custom_provider_name: Option<&str>,
) -> (Option<String>, usize, Option<String>) {
	match project.mode.as_str() {
		"api-keys" => {
			let key = if provider == "custom" {
				if let Some(name) = custom_provider_name {
					db::find_custom_provider_key(&state.db, &project.organization_id, name)
						.await
						.ok()
						.flatten()
				} else {
					None
				}
			} else {
				db::find_provider_key(&state.db, &project.organization_id, provider)
					.await
					.ok()
					.flatten()
			};
			(key.map(|k| k.token), 0, None)
		}
		"credits" => {
			let (token, index, env_name) = registry::get_provider_env(provider);
			(token, index, env_name)
		}
		"hybrid" => {
			// Try DB first, then fall back to env
			let key = if provider == "custom" {
				if let Some(name) = custom_provider_name {
					db::find_custom_provider_key(&state.db, &project.organization_id, name)
						.await
						.ok()
						.flatten()
				} else {
					None
				}
			} else {
				db::find_provider_key(&state.db, &project.organization_id, provider)
					.await
					.ok()
					.flatten()
			};

			if let Some(k) = key {
				(Some(k.token), 0, None)
			} else {
				let (token, index, env_name) = registry::get_provider_env(provider);
				(token, index, env_name)
			}
		}
		_ => (None, 0, None),
	}
}
