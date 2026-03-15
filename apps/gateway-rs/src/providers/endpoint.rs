use crate::models::registry;

/// Construct the upstream provider endpoint URL
pub fn get_provider_endpoint(
	provider: &str,
	base_url: Option<&str>,
	model: &str,
	token: Option<&str>,
	stream: bool,
	supports_reasoning: bool,
	has_existing_tool_calls: bool,
	provider_key_options: Option<&serde_json::Value>,
	config_index: usize,
	image_generations: bool,
) -> Option<String> {
	match provider {
		"openai" => {
			let base = base_url.unwrap_or("https://api.openai.com");
			if supports_reasoning && !has_existing_tool_calls {
				Some(format!("{base}/v1/responses"))
			} else {
				Some(format!("{base}/v1/chat/completions"))
			}
		}

		"anthropic" => {
			let base = base_url.unwrap_or("https://api.anthropic.com");
			Some(format!("{base}/v1/messages"))
		}

		"google-ai-studio" => {
			let base = "https://generativelanguage.googleapis.com";
			let key = token.unwrap_or("");
			if stream {
				Some(format!("{base}/v1beta/models/{model}:streamGenerateContent?alt=sse&key={key}"))
			} else {
				Some(format!("{base}/v1beta/models/{model}:generateContent?key={key}"))
			}
		}

		"google-vertex" => {
			let project = registry::get_provider_env_value("google-vertex", "project", config_index)
				.unwrap_or_default();
			let location =
				registry::get_provider_env_value("google-vertex", "region", config_index)
					.unwrap_or_else(|| "us-central1".to_string());
			let method = if stream {
				"streamGenerateContent?alt=sse"
			} else {
				"generateContent"
			};
			Some(format!(
				"https://{location}-aiplatform.googleapis.com/v1/projects/{project}/locations/{location}/publishers/google/models/{model}:{method}"
			))
		}

		"groq" => {
			let base = base_url.unwrap_or("https://api.groq.com/openai");
			Some(format!("{base}/v1/chat/completions"))
		}

		"cerebras" => {
			let base = base_url.unwrap_or("https://api.cerebras.ai");
			Some(format!("{base}/v1/chat/completions"))
		}

		"xai" => {
			let base = base_url.unwrap_or("https://api.x.ai");
			if image_generations {
				Some(format!("{base}/v1/images/generations"))
			} else {
				Some(format!("{base}/v1/chat/completions"))
			}
		}

		"deepseek" => {
			let base = base_url.unwrap_or("https://api.deepseek.com");
			Some(format!("{base}/chat/completions"))
		}

		"alibaba" => {
			let base = base_url.unwrap_or("https://dashscope-intl.aliyuncs.com/compatible-mode");
			if image_generations {
				Some(format!("{base}/v1/images/generations"))
			} else {
				Some(format!("{base}/v1/chat/completions"))
			}
		}

		"aws-bedrock" => {
			let region = registry::get_provider_env_value("aws-bedrock", "region", config_index)
				.unwrap_or_else(|| "us-east-1".to_string());

			// Check for region prefix in provider key options
			let region_prefix = provider_key_options
				.and_then(|opts| opts.get("aws_bedrock_region_prefix"))
				.and_then(|v| v.as_str())
				.unwrap_or("");

			let effective_region = if region_prefix.is_empty() {
				region
			} else {
				format!("{region_prefix}.{region}")
			};

			let base = format!("https://bedrock-runtime.{effective_region}.amazonaws.com");
			if stream {
				Some(format!(
					"{base}/model/{model}/converse-stream"
				))
			} else {
				Some(format!("{base}/model/{model}/converse"))
			}
		}

		"azure" => {
			let env_resource = registry::get_provider_env_value("azure", "resource", config_index);
			let resource = provider_key_options
				.and_then(|opts| opts.get("azure_resource"))
				.and_then(|v| v.as_str())
				.map(|s| s.to_string())
				.or(env_resource)
				.unwrap_or_else(|| "default".to_string());
			let resource = resource.as_str();

			let api_version = provider_key_options
				.and_then(|opts| opts.get("azure_api_version"))
				.and_then(|v| v.as_str())
				.unwrap_or("2024-12-01-preview");

			let deployment_type = provider_key_options
				.and_then(|opts| opts.get("azure_deployment_type"))
				.and_then(|v| v.as_str())
				.unwrap_or("deployment");

			if deployment_type == "ai-foundry" {
				Some(format!(
					"https://{resource}.services.ai.azure.com/openai/deployments/{model}/chat/completions?api-version={api_version}"
				))
			} else {
				Some(format!(
					"https://{resource}.openai.azure.com/openai/deployments/{model}/chat/completions?api-version={api_version}"
				))
			}
		}

		"mistral" => {
			let base = base_url.unwrap_or("https://api.mistral.ai");
			Some(format!("{base}/v1/chat/completions"))
		}

		"perplexity" => {
			let base = base_url.unwrap_or("https://api.perplexity.ai");
			Some(format!("{base}/chat/completions"))
		}

		"moonshot" => {
			let base = base_url.unwrap_or("https://api.moonshot.cn");
			Some(format!("{base}/v1/chat/completions"))
		}

		"together.ai" => {
			let base = base_url.unwrap_or("https://api.together.xyz");
			Some(format!("{base}/v1/chat/completions"))
		}

		"novita" => {
			let base = base_url.unwrap_or("https://api.novita.ai");
			Some(format!("{base}/v3/openai/chat/completions"))
		}

		"nebius" => {
			let base = base_url.unwrap_or("https://api.studio.nebius.ai");
			Some(format!("{base}/v1/chat/completions"))
		}

		"inference.net" => {
			let base = base_url.unwrap_or("https://api.inference.net");
			Some(format!("{base}/v1/chat/completions"))
		}

		"canopywave" => {
			let base = base_url.unwrap_or("https://cloud.canopywave.io");
			Some(format!("{base}/v1/chat/completions"))
		}

		"nanogpt" => {
			let base = base_url.unwrap_or("https://api.nanogpt.cloud");
			Some(format!("{base}/v1/chat/completions"))
		}

		"bytedance" => {
			let base = base_url.unwrap_or("https://ark.cn-beijing.volces.com/api");
			if image_generations {
				Some(format!("{base}/v3/images/generations"))
			} else {
				Some(format!("{base}/v3/chat/completions"))
			}
		}

		"minimax" => {
			let base = base_url.unwrap_or("https://api.minimaxi.chat");
			Some(format!("{base}/v1/text/chatcompletion_v2"))
		}

		"zai" => {
			let base = base_url.unwrap_or("https://open.bigmodel.cn/api/paas");
			if image_generations {
				Some(format!("{base}/v4/images/generations"))
			} else {
				Some(format!("{base}/v4/chat/completions"))
			}
		}

		"obsidian" => {
			let base = base_url.unwrap_or("https://generativelanguage.googleapis.com");
			let method = if stream {
				"streamGenerateContent?alt=sse"
			} else {
				"generateContent"
			};
			let key = token.unwrap_or("");
			Some(format!("{base}/v1beta/models/{model}:{method}&key={key}"))
		}

		"custom" => base_url.map(|b| format!("{b}/v1/chat/completions")),

		_ => {
			// Default OpenAI-compatible endpoint
			if let Some(base) = base_url {
				Some(format!("{base}/v1/chat/completions"))
			} else {
				None
			}
		}
	}
}
