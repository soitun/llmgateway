use sqlx::PgPool;

use super::models::*;

/// Find an API key by its token
pub async fn find_api_key_by_token(pool: &PgPool, token: &str) -> Result<Option<ApiKey>, sqlx::Error> {
	sqlx::query_as::<_, ApiKey>(
		"SELECT id, token, description, status, usage_limit::text, usage::text, project_id, created_by, created_at, updated_at FROM api_key WHERE token = $1 LIMIT 1"
	)
		.bind(token)
		.fetch_optional(pool)
		.await
}

/// Find a project by ID
pub async fn find_project_by_id(pool: &PgPool, id: &str) -> Result<Option<Project>, sqlx::Error> {
	sqlx::query_as::<_, Project>("SELECT * FROM project WHERE id = $1 LIMIT 1")
		.bind(id)
		.fetch_optional(pool)
		.await
}

/// Find an organization by ID
pub async fn find_organization_by_id(
	pool: &PgPool,
	id: &str,
) -> Result<Option<Organization>, sqlx::Error> {
	let org =
		sqlx::query_as::<_, Organization>("SELECT id, name, billing_email, credits::text, plan, status, is_personal, dev_plan, dev_plan_credits_used::text, dev_plan_credits_limit::text, dev_plan_expires_at, dev_plan_allow_all_models, retention_level, stripe_customer_id, stripe_subscription_id, created_at, updated_at FROM organization WHERE id = $1 LIMIT 1")
			.bind(id)
			.fetch_optional(pool)
			.await?;

	// If org has 0 or negative credits, refetch to ensure topups are reflected immediately
	if let Some(ref o) = org {
		if o.total_credits() <= 0.0 {
			return sqlx::query_as::<_, Organization>(
				"SELECT id, name, billing_email, credits::text, plan, status, is_personal, dev_plan, dev_plan_credits_used::text, dev_plan_credits_limit::text, dev_plan_expires_at, dev_plan_allow_all_models, retention_level, stripe_customer_id, stripe_subscription_id, created_at, updated_at FROM organization WHERE id = $1 LIMIT 1",
			)
			.bind(id)
			.fetch_optional(pool)
			.await;
		}
	}

	Ok(org)
}

/// Find a custom provider key by organization and name
pub async fn find_custom_provider_key(
	pool: &PgPool,
	organization_id: &str,
	custom_provider_name: &str,
) -> Result<Option<ProviderKey>, sqlx::Error> {
	sqlx::query_as::<_, ProviderKey>(
		"SELECT * FROM provider_key WHERE status = 'active' AND organization_id = $1 AND provider = 'custom' AND name = $2 LIMIT 1",
	)
	.bind(organization_id)
	.bind(custom_provider_name)
	.fetch_optional(pool)
	.await
}

/// Find a provider key by organization and provider
pub async fn find_provider_key(
	pool: &PgPool,
	organization_id: &str,
	provider: &str,
) -> Result<Option<ProviderKey>, sqlx::Error> {
	sqlx::query_as::<_, ProviderKey>(
		"SELECT * FROM provider_key WHERE status = 'active' AND organization_id = $1 AND provider = $2 LIMIT 1",
	)
	.bind(organization_id)
	.bind(provider)
	.fetch_optional(pool)
	.await
}

/// Find all active provider keys for an organization
pub async fn find_active_provider_keys(
	pool: &PgPool,
	organization_id: &str,
) -> Result<Vec<ProviderKey>, sqlx::Error> {
	sqlx::query_as::<_, ProviderKey>(
		"SELECT * FROM provider_key WHERE status = 'active' AND organization_id = $1",
	)
	.bind(organization_id)
	.fetch_all(pool)
	.await
}

/// Find active provider keys for specific providers
pub async fn find_provider_keys_by_providers(
	pool: &PgPool,
	organization_id: &str,
	providers: &[String],
) -> Result<Vec<ProviderKey>, sqlx::Error> {
	if providers.is_empty() {
		return Ok(vec![]);
	}

	// Build a dynamic IN clause
	let placeholders: Vec<String> = (0..providers.len())
		.map(|i| format!("${}", i + 2))
		.collect();
	let query = format!(
		"SELECT * FROM provider_key WHERE status = 'active' AND organization_id = $1 AND provider IN ({})",
		placeholders.join(", ")
	);

	let mut q = sqlx::query_as::<_, ProviderKey>(&query).bind(organization_id);
	for provider in providers {
		q = q.bind(provider);
	}

	q.fetch_all(pool).await
}

/// Find all active IAM rules for an API key
pub async fn find_active_iam_rules(
	pool: &PgPool,
	api_key_id: &str,
) -> Result<Vec<ApiKeyIamRule>, sqlx::Error> {
	sqlx::query_as::<_, ApiKeyIamRule>(
		"SELECT * FROM api_key_iam_rule WHERE api_key_id = $1 AND status = 'active'",
	)
	.bind(api_key_id)
	.fetch_all(pool)
	.await
}

/// Find user from organization via user_organization join
pub async fn find_user_from_organization(
	pool: &PgPool,
	organization_id: &str,
) -> Result<Option<(UserOrganization, User)>, sqlx::Error> {
	let uo = sqlx::query_as::<_, UserOrganization>(
		"SELECT * FROM user_organization WHERE organization_id = $1 LIMIT 1",
	)
	.bind(organization_id)
	.fetch_optional(pool)
	.await?;

	if let Some(uo) = uo {
		let user = sqlx::query_as::<_, User>(
			r#"SELECT * FROM "user" WHERE id = $1 LIMIT 1"#,
		)
		.bind(&uo.user_id)
		.fetch_optional(pool)
		.await?;

		if let Some(user) = user {
			return Ok(Some((uo, user)));
		}
	}

	Ok(None)
}

/// Check if caching is enabled for a project
pub async fn is_caching_enabled(
	pool: &PgPool,
	project_id: &str,
) -> Result<(bool, i32), sqlx::Error> {
	let result = sqlx::query_as::<_, (Option<bool>, Option<i32>)>(
		"SELECT caching_enabled, cache_duration_seconds FROM project WHERE id = $1 LIMIT 1",
	)
	.bind(project_id)
	.fetch_optional(pool)
	.await?;

	match result {
		Some((enabled, duration)) => Ok((enabled.unwrap_or(false), duration.unwrap_or(60))),
		None => Ok((false, 60)),
	}
}

/// Get effective discount for an organization + provider + model combination
pub async fn get_effective_discount(
	pool: &PgPool,
	organization_id: Option<&str>,
	provider: &str,
	model: &str,
	hardcoded_discount: f64,
	provider_model_name: Option<&str>,
) -> Result<(f64, String), sqlx::Error> {
	// Check organization-specific discounts first, then global, then hardcoded
	let discounts = sqlx::query_as::<_, Discount>(
		r#"
		SELECT * FROM discount
		WHERE (organization_id = $1 OR organization_id IS NULL)
		AND (expires_at IS NULL OR expires_at >= NOW())
		ORDER BY
			CASE WHEN organization_id IS NOT NULL THEN 0 ELSE 1 END,
			CASE WHEN provider IS NOT NULL AND model IS NOT NULL THEN 0
				 WHEN provider IS NOT NULL THEN 1
				 WHEN model IS NOT NULL THEN 2
				 ELSE 3 END
		"#,
	)
	.bind(organization_id)
	.fetch_all(pool)
	.await?;

	for discount in &discounts {
		let provider_match = discount.provider.as_deref().is_none()
			|| discount.provider.as_deref() == Some(provider);
		let model_match = discount.model.as_deref().is_none()
			|| discount.model.as_deref() == Some(model)
			|| discount.model.as_deref() == provider_model_name;

		if provider_match && model_match {
			let source = match (
				discount.organization_id.is_some(),
				discount.provider.is_some(),
				discount.model.is_some(),
			) {
				(true, true, true) => "org_provider_model",
				(true, true, false) => "org_provider",
				(true, false, true) => "org_model",
				(false, true, true) => "global_provider_model",
				(false, true, false) => "global_provider",
				(false, false, true) => "global_model",
				_ => "global",
			};
			return Ok((discount.discount_percent / 100.0, source.to_string()));
		}
	}

	if hardcoded_discount > 0.0 {
		return Ok((hardcoded_discount, "hardcoded".to_string()));
	}

	Ok((0.0, "none".to_string()))
}

/// Get provider metrics for model+provider combinations
pub async fn get_provider_metrics_for_combinations(
	pool: &PgPool,
	combinations: &[(String, String)], // (model_id, provider_id)
) -> Result<std::collections::HashMap<String, ProviderMetrics>, sqlx::Error> {
	let mut result = std::collections::HashMap::new();

	if combinations.is_empty() {
		return Ok(result);
	}

	// Build OR conditions for each combination
	let conditions: Vec<String> = combinations
		.iter()
		.enumerate()
		.map(|(i, _)| format!("(model_id = ${} AND provider_id = ${})", i * 2 + 1, i * 2 + 2))
		.collect();

	let query = format!(
		r#"
		SELECT model_id, provider_id, routing_uptime, routing_latency, routing_throughput, routing_total_requests
		FROM model_provider_mapping
		WHERE status = 'active' AND routing_total_requests > 0 AND ({})
		"#,
		conditions.join(" OR ")
	);

	let mut q = sqlx::query(&query);
	for (model_id, provider_id) in combinations {
		q = q.bind(model_id).bind(provider_id);
	}

	let rows = q.fetch_all(pool).await?;

	for row in rows {
		use sqlx::Row;
		let model_id: String = row.get("model_id");
		let provider_id: String = row.get("provider_id");
		let key = format!("{model_id}:{provider_id}");

		result.insert(
			key,
			ProviderMetrics {
				provider_id,
				model_id,
				uptime: row.get("routing_uptime"),
				average_latency: row.get("routing_latency"),
				throughput: row.get("routing_throughput"),
				total_requests: row.get::<Option<i64>, _>("routing_total_requests").unwrap_or(0),
			},
		);
	}

	Ok(result)
}

/// Insert a log entry
pub async fn insert_log(pool: &PgPool, log: &LogEntry) -> Result<(), sqlx::Error> {
	sqlx::query(
		r#"
		INSERT INTO log (
			id, request_id, organization_id, project_id, api_key_id, provider_key_id,
			requested_model, requested_provider, used_model, used_model_mapping, used_provider,
			duration, time_to_first_token, time_to_first_reasoning_token,
			response_size, content, reasoning_content, finish_reason,
			prompt_tokens, completion_tokens, total_tokens, reasoning_tokens, cached_tokens,
			has_error, streamed, canceled, error_details,
			cost, input_cost, output_cost, cached_input_cost, request_cost, web_search_cost,
			image_input_tokens, image_output_tokens, image_input_cost, image_output_cost,
			estimated_cost, discount, pricing_tier, data_storage_cost,
			cached, retried, retried_by_log_id,
			messages, temperature, max_tokens, top_p, frequency_penalty, presence_penalty,
			reasoning_effort, tools, tool_choice, tool_results, response_format,
			routing_metadata, source, custom_headers, user_agent, mode,
			raw_request, raw_response, upstream_request, upstream_response,
			plugins, plugin_results
		) VALUES (
			$1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18,
			$19, $20, $21, $22, $23, $24, $25, $26, $27, $28, $29, $30, $31, $32, $33,
			$34, $35, $36, $37, $38, $39, $40, $41, $42, $43, $44, $45, $46, $47, $48,
			$49, $50, $51, $52, $53, $54, $55, $56, $57, $58, $59, $60, $61, $62, $63, $64
		)
		"#,
	)
	.bind(&log.id)
	.bind(&log.request_id)
	.bind(&log.organization_id)
	.bind(&log.project_id)
	.bind(&log.api_key_id)
	.bind(&log.provider_key_id)
	.bind(&log.requested_model)
	.bind(&log.requested_provider)
	.bind(&log.used_model)
	.bind(&log.used_model_mapping)
	.bind(&log.used_provider)
	.bind(log.duration)
	.bind(log.time_to_first_token)
	.bind(log.time_to_first_reasoning_token)
	.bind(log.response_size)
	.bind(&log.content)
	.bind(&log.reasoning_content)
	.bind(&log.finish_reason)
	.bind(&log.prompt_tokens)
	.bind(&log.completion_tokens)
	.bind(&log.total_tokens)
	.bind(&log.reasoning_tokens)
	.bind(&log.cached_tokens)
	.bind(log.has_error)
	.bind(log.streamed)
	.bind(log.canceled)
	.bind(&log.error_details)
	.bind(log.cost)
	.bind(log.input_cost)
	.bind(log.output_cost)
	.bind(log.cached_input_cost)
	.bind(log.request_cost)
	.bind(log.web_search_cost)
	.bind(&log.image_input_tokens)
	.bind(&log.image_output_tokens)
	.bind(log.image_input_cost)
	.bind(log.image_output_cost)
	.bind(log.estimated_cost)
	.bind(log.discount)
	.bind(&log.pricing_tier)
	.bind(&log.data_storage_cost)
	.bind(log.cached)
	.bind(log.retried)
	.bind(&log.retried_by_log_id)
	.bind(&log.messages)
	.bind(log.temperature)
	.bind(log.max_tokens)
	.bind(log.top_p)
	.bind(log.frequency_penalty)
	.bind(log.presence_penalty)
	.bind(&log.reasoning_effort)
	.bind(&log.tools)
	.bind(&log.tool_choice)
	.bind(&log.tool_results)
	.bind(&log.response_format)
	.bind(&log.routing_metadata)
	.bind(&log.source)
	.bind(&log.custom_headers)
	.bind(&log.user_agent)
	.bind(&log.mode)
	.bind(&log.raw_request)
	.bind(&log.raw_response)
	.bind(&log.upstream_request)
	.bind(&log.upstream_response)
	.bind(&log.plugins)
	.bind(&log.plugin_results)
	.execute(pool)
	.await?;

	Ok(())
}
