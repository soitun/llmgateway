use serde_json::{json, Value};

use crate::models::WebSearchTool;

/// Prepare the request body for a specific provider
///
/// This transforms the OpenAI-compatible format to whatever the upstream provider expects.
/// For most providers this is close to OpenAI format; Anthropic and Google need deeper transformation.
pub fn prepare_request_body(
	provider: &str,
	model: &str,
	messages: &Value,
	stream: bool,
	temperature: Option<f64>,
	max_tokens: Option<i64>,
	top_p: Option<f64>,
	frequency_penalty: Option<f64>,
	presence_penalty: Option<f64>,
	response_format: Option<&Value>,
	tools: Option<&Value>,
	tool_choice: Option<&Value>,
	reasoning_effort: Option<&str>,
	supports_reasoning: bool,
	image_config: Option<&Value>,
	effort: Option<&str>,
	image_generations: bool,
	web_search_tool: Option<&WebSearchTool>,
	reasoning_max_tokens: Option<i64>,
	use_responses_api: bool,
) -> Value {
	match provider {
		"anthropic" => build_anthropic_body(
			model,
			messages,
			stream,
			temperature,
			max_tokens,
			top_p,
			tools,
			tool_choice,
			reasoning_effort,
			supports_reasoning,
			effort,
			response_format,
			reasoning_max_tokens,
			web_search_tool,
		),

		"google-ai-studio" | "google-vertex" | "obsidian" => build_google_body(
			messages,
			stream,
			temperature,
			max_tokens,
			top_p,
			tools,
			reasoning_effort,
			supports_reasoning,
			image_config,
			image_generations,
			reasoning_max_tokens,
			web_search_tool,
		),

		"openai" if use_responses_api => build_openai_responses_body(
			model,
			messages,
			stream,
			temperature,
			max_tokens,
			reasoning_effort,
			tools,
			tool_choice,
			response_format,
		),

		_ => build_openai_compatible_body(
			provider,
			model,
			messages,
			stream,
			temperature,
			max_tokens,
			top_p,
			frequency_penalty,
			presence_penalty,
			response_format,
			tools,
			tool_choice,
			reasoning_effort,
			supports_reasoning,
			image_config,
			image_generations,
			web_search_tool,
			reasoning_max_tokens,
		),
	}
}

fn build_openai_compatible_body(
	_provider: &str,
	model: &str,
	messages: &Value,
	stream: bool,
	temperature: Option<f64>,
	max_tokens: Option<i64>,
	top_p: Option<f64>,
	frequency_penalty: Option<f64>,
	presence_penalty: Option<f64>,
	response_format: Option<&Value>,
	tools: Option<&Value>,
	tool_choice: Option<&Value>,
	reasoning_effort: Option<&str>,
	_supports_reasoning: bool,
	_image_config: Option<&Value>,
	_image_generations: bool,
	web_search_tool: Option<&WebSearchTool>,
	_reasoning_max_tokens: Option<i64>,
) -> Value {
	let mut body = json!({
		"model": model,
		"messages": messages,
	});

	if stream {
		body["stream"] = json!(true);
		body["stream_options"] = json!({"include_usage": true});
	}

	if let Some(t) = temperature {
		body["temperature"] = json!(t);
	}
	if let Some(mt) = max_tokens {
		body["max_tokens"] = json!(mt);
	}
	if let Some(tp) = top_p {
		body["top_p"] = json!(tp);
	}
	if let Some(fp) = frequency_penalty {
		body["frequency_penalty"] = json!(fp);
	}
	if let Some(pp) = presence_penalty {
		body["presence_penalty"] = json!(pp);
	}
	if let Some(rf) = response_format {
		body["response_format"] = rf.clone();
	}
	if let Some(t) = tools {
		if let Value::Array(arr) = t {
			if !arr.is_empty() {
				body["tools"] = t.clone();
			}
		}
	}
	if let Some(tc) = tool_choice {
		body["tool_choice"] = tc.clone();
	}
	if let Some(re) = reasoning_effort {
		body["reasoning_effort"] = json!(re);
	}

	// Add web search as a tool if enabled
	if let Some(_ws) = web_search_tool {
		let tools_arr = body
			.get("tools")
			.and_then(|v| v.as_array())
			.cloned()
			.unwrap_or_default();
		let mut tools_vec = tools_arr;
		tools_vec.push(json!({"type": "web_search_preview"}));
		body["tools"] = json!(tools_vec);
	}

	body
}

fn build_anthropic_body(
	model: &str,
	messages: &Value,
	stream: bool,
	temperature: Option<f64>,
	max_tokens: Option<i64>,
	top_p: Option<f64>,
	tools: Option<&Value>,
	tool_choice: Option<&Value>,
	reasoning_effort: Option<&str>,
	supports_reasoning: bool,
	effort: Option<&str>,
	response_format: Option<&Value>,
	reasoning_max_tokens: Option<i64>,
	web_search_tool: Option<&WebSearchTool>,
) -> Value {
	// Transform OpenAI messages to Anthropic format
	let (system_prompt, anthropic_messages) = transform_messages_to_anthropic(messages);

	let mut body = json!({
		"model": model,
		"messages": anthropic_messages,
		"max_tokens": max_tokens.unwrap_or(4096),
	});

	if let Some(sys) = system_prompt {
		body["system"] = json!(sys);
	}

	if stream {
		body["stream"] = json!(true);
	}

	if let Some(t) = temperature {
		// Anthropic doesn't allow both temperature and top_p
		body["temperature"] = json!(t);
	} else if let Some(tp) = top_p {
		body["top_p"] = json!(tp);
	}

	if let Some(t) = tools {
		// Transform OpenAI tools to Anthropic format
		let anthropic_tools = transform_tools_to_anthropic(t);
		if let Value::Array(ref arr) = anthropic_tools {
			if !arr.is_empty() {
				body["tools"] = anthropic_tools;
			}
		}
	}

	if let Some(tc) = tool_choice {
		body["tool_choice"] = transform_tool_choice_to_anthropic(tc);
	}

	// Handle reasoning/thinking
	if supports_reasoning {
		if let Some(rmt) = reasoning_max_tokens {
			body["thinking"] = json!({"type": "enabled", "budget_tokens": rmt});
		} else if let Some(re) = reasoning_effort {
			let budget = match re {
				"minimal" => 1024,
				"low" => 2048,
				"medium" => 4096,
				"high" => 8192,
				"xhigh" => 16384,
				_ => 4096,
			};
			body["thinking"] = json!({"type": "enabled", "budget_tokens": budget});
		}
	}

	if let Some(e) = effort {
		body["output_config"] = json!({"effort": e});
	}

	// Handle response format for Anthropic
	if let Some(rf) = response_format {
		if rf.get("type").and_then(|v| v.as_str()) == Some("json_schema") {
			// Anthropic structured outputs
			if let Some(schema) = rf.get("json_schema") {
				body["response_format"] = json!({
					"type": "json_schema",
					"json_schema": schema,
				});
			}
		}
	}

	// Add web search tool for Anthropic
	if let Some(ws) = web_search_tool {
		let tools_arr = body
			.get("tools")
			.and_then(|v| v.as_array())
			.cloned()
			.unwrap_or_default();
		let mut tools_vec = tools_arr;
		let mut web_search_def = json!({
			"type": "web_search_20250305",
			"name": "web_search",
		});
		if let Some(ref loc) = ws.user_location {
			web_search_def["user_location"] = loc.clone();
		}
		if let Some(max) = ws.max_uses {
			web_search_def["max_uses"] = json!(max);
		}
		tools_vec.push(web_search_def);
		body["tools"] = json!(tools_vec);
	}

	body
}

fn build_google_body(
	messages: &Value,
	_stream: bool,
	temperature: Option<f64>,
	max_tokens: Option<i64>,
	top_p: Option<f64>,
	tools: Option<&Value>,
	reasoning_effort: Option<&str>,
	supports_reasoning: bool,
	image_config: Option<&Value>,
	image_generations: bool,
	reasoning_max_tokens: Option<i64>,
	web_search_tool: Option<&WebSearchTool>,
) -> Value {
	let google_messages = transform_messages_to_google(messages);

	let mut body = json!({
		"contents": google_messages,
	});

	let mut gen_config = json!({});

	if let Some(t) = temperature {
		gen_config["temperature"] = json!(t);
	}
	if let Some(mt) = max_tokens {
		gen_config["maxOutputTokens"] = json!(mt);
	}
	if let Some(tp) = top_p {
		gen_config["topP"] = json!(tp);
	}

	// Handle reasoning/thinking
	if supports_reasoning {
		let include_thoughts = reasoning_effort.is_some() || reasoning_max_tokens.is_some();
		if include_thoughts {
			gen_config["thinkingConfig"] = json!({"includeThoughts": true});
			if let Some(rmt) = reasoning_max_tokens {
				gen_config["thinkingConfig"]["thinkingBudget"] = json!(rmt);
			}
		}
	}

	// Handle image generation
	if image_generations {
		gen_config["responseModalities"] = json!(["TEXT", "IMAGE"]);
		if let Some(ic) = image_config {
			if let Some(ar) = ic.get("aspect_ratio").and_then(|v| v.as_str()) {
				gen_config["imageConfig"] = json!({"aspectRatio": ar});
			}
		}
	}

	if gen_config != json!({}) {
		body["generationConfig"] = gen_config;
	}

	if let Some(t) = tools {
		let google_tools = transform_tools_to_google(t);
		if let Value::Array(ref arr) = google_tools {
			if !arr.is_empty() {
				body["tools"] = google_tools;
			}
		}
	}

	// Add Google search tool
	if web_search_tool.is_some() {
		let tools_arr = body
			.get("tools")
			.and_then(|v| v.as_array())
			.cloned()
			.unwrap_or_default();
		let mut tools_vec = tools_arr;
		tools_vec.push(json!({"googleSearch": {}}));
		body["tools"] = json!(tools_vec);
	}

	body
}

fn build_openai_responses_body(
	model: &str,
	messages: &Value,
	stream: bool,
	temperature: Option<f64>,
	max_tokens: Option<i64>,
	reasoning_effort: Option<&str>,
	tools: Option<&Value>,
	tool_choice: Option<&Value>,
	response_format: Option<&Value>,
) -> Value {
	// Transform messages to responses API input format
	let input = transform_messages_to_responses_input(messages);

	let mut body = json!({
		"model": model,
		"input": input,
	});

	if stream {
		body["stream"] = json!(true);
	}

	if let Some(t) = temperature {
		body["temperature"] = json!(t);
	}
	if let Some(mt) = max_tokens {
		body["max_output_tokens"] = json!(mt);
	}

	if let Some(re) = reasoning_effort {
		body["reasoning"] = json!({
			"effort": re,
			"summary": "detailed",
		});
	}

	if let Some(t) = tools {
		// Convert to responses API tool format
		let mut responses_tools = vec![];
		if let Value::Array(arr) = t {
			for tool in arr {
				if tool.get("type").and_then(|v| v.as_str()) == Some("function") {
					if let Some(func) = tool.get("function") {
						responses_tools.push(json!({
							"type": "function",
							"name": func.get("name"),
							"description": func.get("description"),
							"parameters": func.get("parameters"),
						}));
					}
				}
			}
		}
		if !responses_tools.is_empty() {
			body["tools"] = json!(responses_tools);
		}
	}

	if let Some(tc) = tool_choice {
		body["tool_choice"] = tc.clone();
	}

	if let Some(rf) = response_format {
		body["text"] = json!({"format": rf});
	}

	body
}

// --- Message transformation helpers ---

fn transform_messages_to_anthropic(messages: &Value) -> (Option<String>, Value) {
	let mut system_prompt = None;
	let mut result = vec![];

	if let Value::Array(msgs) = messages {
		for msg in msgs {
			let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("user");

			match role {
				"system" => {
					let content = msg
						.get("content")
						.and_then(|v| v.as_str())
						.unwrap_or("")
						.to_string();
					system_prompt = Some(content);
				}
				"assistant" => {
					let mut anthropic_msg = json!({"role": "assistant"});
					if let Some(content) = msg.get("content") {
						anthropic_msg["content"] = content.clone();
					}
					// Preserve tool_calls
					if let Some(tool_calls) = msg.get("tool_calls") {
						if let Value::Array(tcs) = tool_calls {
							let mut content_blocks = vec![];
							// Add text content first if present
							if let Some(text) = msg.get("content").and_then(|v| v.as_str()) {
								if !text.is_empty() {
									content_blocks.push(json!({"type": "text", "text": text}));
								}
							}
							for tc in tcs {
								content_blocks.push(json!({
									"type": "tool_use",
									"id": tc.get("id"),
									"name": tc.get("function").and_then(|f| f.get("name")),
									"input": serde_json::from_str::<Value>(
										tc.get("function")
											.and_then(|f| f.get("arguments"))
											.and_then(|a| a.as_str())
											.unwrap_or("{}")
									).unwrap_or(json!({})),
								}));
							}
							anthropic_msg["content"] = json!(content_blocks);
						}
					}
					result.push(anthropic_msg);
				}
				"tool" => {
					let tool_call_id = msg
						.get("tool_call_id")
						.and_then(|v| v.as_str())
						.unwrap_or("");
					let content = msg
						.get("content")
						.and_then(|v| v.as_str())
						.unwrap_or("");
					result.push(json!({
						"role": "user",
						"content": [{
							"type": "tool_result",
							"tool_use_id": tool_call_id,
							"content": content,
						}],
					}));
				}
				_ => {
					// user message
					result.push(json!({
						"role": "user",
						"content": msg.get("content").cloned().unwrap_or(json!("")),
					}));
				}
			}
		}
	}

	(system_prompt, json!(result))
}

fn transform_messages_to_google(messages: &Value) -> Value {
	let mut result = vec![];

	if let Value::Array(msgs) = messages {
		for msg in msgs {
			let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("user");
			let google_role = match role {
				"assistant" => "model",
				"system" => "user", // Google uses user role for system messages
				_ => "user",
			};

			let mut parts = vec![];

			if let Some(content) = msg.get("content") {
				match content {
					Value::String(text) => {
						parts.push(json!({"text": text}));
					}
					Value::Array(content_parts) => {
						for part in content_parts {
							if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
								parts.push(json!({"text": text}));
							} else if let Some(image_url) = part.get("image_url") {
								if let Some(url) =
									image_url.get("url").and_then(|v| v.as_str())
								{
									if url.starts_with("data:") {
										// Base64 data URL
										if let Some((mime, data)) = parse_data_url(url) {
											parts.push(json!({
												"inline_data": {
													"mime_type": mime,
													"data": data,
												}
											}));
										}
									}
								}
							}
						}
					}
					_ => {}
				}
			}

			// Handle tool calls for Google
			if let Some(tool_calls) = msg.get("tool_calls") {
				if let Value::Array(tcs) = tool_calls {
					for tc in tcs {
						if let Some(func) = tc.get("function") {
							parts.push(json!({
								"functionCall": {
									"name": func.get("name"),
									"args": serde_json::from_str::<Value>(
										func.get("arguments")
											.and_then(|a| a.as_str())
											.unwrap_or("{}")
									).unwrap_or(json!({})),
								}
							}));
						}
					}
				}
			}

			// Handle tool results
			if role == "tool" {
				let content = msg
					.get("content")
					.and_then(|v| v.as_str())
					.unwrap_or("");
				let name = msg.get("name").and_then(|v| v.as_str()).unwrap_or("result");
				parts = vec![json!({
					"functionResponse": {
						"name": name,
						"response": {"result": content},
					}
				})];
			}

			if !parts.is_empty() {
				result.push(json!({
					"role": google_role,
					"parts": parts,
				}));
			}
		}
	}

	json!(result)
}

fn transform_messages_to_responses_input(messages: &Value) -> Value {
	let mut result = vec![];

	if let Value::Array(msgs) = messages {
		for msg in msgs {
			let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("user");
			match role {
				"tool" => {
					result.push(json!({
						"type": "function_call_output",
						"call_id": msg.get("tool_call_id"),
						"output": msg.get("content"),
					}));
				}
				"assistant" if msg.get("tool_calls").is_some() => {
					// Add content first if present
					if let Some(content) = msg.get("content").and_then(|v| v.as_str()) {
						if !content.is_empty() {
							result.push(json!({
								"role": "assistant",
								"content": content,
							}));
						}
					}
					// Add function calls
					if let Some(Value::Array(tcs)) = msg.get("tool_calls") {
						for tc in tcs {
							result.push(json!({
								"type": "function_call",
								"id": tc.get("id"),
								"call_id": tc.get("id"),
								"name": tc.get("function").and_then(|f| f.get("name")),
								"arguments": tc.get("function").and_then(|f| f.get("arguments")),
							}));
						}
					}
				}
				_ => {
					result.push(json!({
						"role": role,
						"content": msg.get("content"),
					}));
				}
			}
		}
	}

	json!(result)
}

fn transform_tools_to_anthropic(tools: &Value) -> Value {
	let mut result = vec![];

	if let Value::Array(tool_list) = tools {
		for tool in tool_list {
			if tool.get("type").and_then(|v| v.as_str()) == Some("function") {
				if let Some(func) = tool.get("function") {
					result.push(json!({
						"name": func.get("name"),
						"description": func.get("description"),
						"input_schema": func.get("parameters").unwrap_or(&json!({"type": "object", "properties": {}})),
					}));
				}
			}
		}
	}

	json!(result)
}

fn transform_tool_choice_to_anthropic(tool_choice: &Value) -> Value {
	match tool_choice {
		Value::String(s) => match s.as_str() {
			"auto" => json!({"type": "auto"}),
			"none" => json!({"type": "none"}),
			"required" => json!({"type": "any"}),
			_ => json!({"type": "auto"}),
		},
		Value::Object(obj) => {
			if let Some(func) = obj.get("function") {
				if let Some(name) = func.get("name") {
					return json!({"type": "tool", "name": name});
				}
			}
			json!({"type": "auto"})
		}
		_ => json!({"type": "auto"}),
	}
}

fn transform_tools_to_google(tools: &Value) -> Value {
	let mut declarations = vec![];

	if let Value::Array(tool_list) = tools {
		for tool in tool_list {
			if tool.get("type").and_then(|v| v.as_str()) == Some("function") {
				if let Some(func) = tool.get("function") {
					declarations.push(json!({
						"name": func.get("name"),
						"description": func.get("description"),
						"parameters": func.get("parameters"),
					}));
				}
			}
		}
	}

	if declarations.is_empty() {
		json!([])
	} else {
		json!([{"functionDeclarations": declarations}])
	}
}

/// Parse a data URL into (mime_type, base64_data)
fn parse_data_url(url: &str) -> Option<(String, String)> {
	let stripped = url.strip_prefix("data:")?;
	let (mime_part, data) = stripped.split_once(',')?;
	let mime = mime_part.strip_suffix(";base64").unwrap_or(mime_part);
	Some((mime.to_string(), data.to_string()))
}
