import { afterEach, beforeEach, describe, expect, test } from "vitest";

import { app } from "@/index.js";
import { getTestToken } from "@/testing.js";

import {
	db,
	eq,
	modelHistory,
	modelProviderMappingHistory,
	tables,
} from "@llmgateway/db";

describe("admin models endpoint", () => {
	const previousAdminEmails = process.env.ADMIN_EMAILS;
	const modelId = "test-admin-model-window";
	const providerId = "test-admin-provider-window";
	const userId = "test-admin-user-id";
	const orgId = "test-admin-org-id";
	const projectId = "test-admin-project-id";
	const apiKeyId = "test-admin-api-key-id";
	const mappingId = "test-admin-mapping-id";
	let token: string;

	async function cleanupTestData() {
		await db.delete(tables.log).where(eq(tables.log.projectId, projectId));
		await db.delete(tables.apiKey).where(eq(tables.apiKey.id, apiKeyId));
		await db.delete(modelHistory).where(eq(modelHistory.modelId, modelId));
		await db
			.delete(modelProviderMappingHistory)
			.where(eq(modelProviderMappingHistory.modelId, modelId));
		await db
			.delete(tables.modelProviderMapping)
			.where(eq(tables.modelProviderMapping.modelId, modelId));
		await db.delete(tables.model).where(eq(tables.model.id, modelId));
		await db.delete(tables.provider).where(eq(tables.provider.id, providerId));
		await db
			.delete(tables.project)
			.where(eq(tables.project.organizationId, orgId));
		await db
			.delete(tables.userOrganization)
			.where(eq(tables.userOrganization.userId, userId));
		await db
			.delete(tables.organization)
			.where(eq(tables.organization.id, orgId));
		await db.delete(tables.session).where(eq(tables.session.userId, userId));
		await db.delete(tables.account).where(eq(tables.account.userId, userId));
		await db.delete(tables.user).where(eq(tables.user.id, userId));
	}

	beforeEach(async () => {
		process.env.ADMIN_EMAILS = "admin@example.com";

		await cleanupTestData();

		await db.insert(tables.user).values({
			id: userId,
			name: "Test Admin User",
			email: "admin@example.com",
			emailVerified: true,
		});

		await db.insert(tables.account).values({
			id: "test-admin-account-id",
			providerId: "credential",
			accountId: "test-admin-account-id",
			userId,
			password:
				"c11ef27a7f9264be08db228ebb650888:a4d985a9c6bd98608237fd507534424950aa7fc255930d972242b81cbe78594f8568feb0d067e95ddf7be242ad3e9d013f695f4414fce68bfff091079f1dc460",
		});

		token = await getTestToken(app);

		await db.insert(tables.organization).values({
			id: orgId,
			name: "Test Admin Org",
			billingEmail: "admin@example.com",
		});

		await db.insert(tables.userOrganization).values({
			id: "test-admin-user-org-id",
			userId,
			organizationId: orgId,
		});

		await db.insert(tables.project).values({
			id: projectId,
			name: "Test Admin Project",
			organizationId: orgId,
		});

		await db.insert(tables.apiKey).values({
			id: apiKeyId,
			token: "test-admin-api-token",
			projectId,
			description: "Test Admin API Key",
			createdBy: userId,
		});

		await db.insert(tables.provider).values({
			id: providerId,
			name: "Test Admin Provider",
			description: "Provider for admin models test",
		});

		await db.insert(tables.model).values({
			id: modelId,
			name: "Test Admin Model",
			family: "test",
		});

		await db.insert(tables.modelProviderMapping).values({
			id: mappingId,
			modelId,
			providerId,
			modelName: "provider-side-model-name",
		});

		const now = new Date();
		const minuteTimestamp = new Date(now);
		minuteTimestamp.setSeconds(0, 0);

		await db.insert(tables.log).values([
			{
				id: "test-admin-log-1",
				requestId: "test-admin-log-1",
				createdAt: new Date(now.getTime() - 30_000),
				updatedAt: now,
				organizationId: orgId,
				projectId,
				apiKeyId,
				duration: 120,
				requestedModel: modelId,
				requestedProvider: providerId,
				usedModel: `${providerId}/${modelId}`,
				usedProvider: providerId,
				responseSize: 100,
				promptTokens: "20",
				completionTokens: "40",
				totalTokens: "60",
				cost: 1.25,
				messages: JSON.stringify([{ role: "user", content: "hello" }]),
				mode: "api-keys",
				usedMode: "api-keys",
			},
			{
				id: "test-admin-log-2",
				requestId: "test-admin-log-2",
				createdAt: new Date(now.getTime() - 20_000),
				updatedAt: now,
				organizationId: orgId,
				projectId,
				apiKeyId,
				duration: 220,
				requestedModel: modelId,
				requestedProvider: providerId,
				usedModel: `${providerId}/${modelId}`,
				usedProvider: providerId,
				responseSize: 100,
				promptTokens: "20",
				completionTokens: "40",
				totalTokens: "60",
				cost: 0.75,
				hasError: true,
				cached: true,
				messages: JSON.stringify([{ role: "user", content: "world" }]),
				mode: "api-keys",
				usedMode: "api-keys",
			},
		]);

		await db.insert(modelHistory).values({
			modelId,
			minuteTimestamp,
			logsCount: 2,
			errorsCount: 1,
			cachedCount: 1,
			totalTokens: 120,
			totalTimeToFirstToken: 300,
			totalCost: 2,
		});

		await db.insert(modelProviderMappingHistory).values({
			modelProviderMappingId: mappingId,
			providerId,
			modelId,
			minuteTimestamp,
			logsCount: 3,
			errorsCount: 1,
			cachedCount: 1,
			totalTokens: 210,
			totalTimeToFirstToken: 500,
			totalCost: 3.5,
		});
	});

	afterEach(async () => {
		process.env.ADMIN_EMAILS = previousAdminEmails;
		await cleanupTestData();
	});

	test("GET /admin/models returns windowed stats for provider/model ids", async () => {
		const now = new Date();
		const fiveMinutesMs = 5 * 60_000;
		const from = new Date(now.getTime() - fiveMinutesMs).toISOString();
		const to = new Date(now.getTime() + 60_000).toISOString();

		const res = await app.request(
			`/admin/models?search=${modelId}&from=${encodeURIComponent(from)}&to=${encodeURIComponent(to)}`,
			{
				headers: {
					Cookie: token,
				},
			},
		);

		expect(res.status).toBe(200);
		const data = await res.json();

		expect(data.total).toBe(1);
		expect(data.totalTokens).toBe(120);
		expect(data.totalCost).toBe(2);
		expect(data.models).toHaveLength(1);
		expect(data.models[0]).toMatchObject({
			id: modelId,
			logsCount: 2,
			errorsCount: 1,
			cachedCount: 1,
			totalTokens: 120,
			totalCost: 2,
			providerCount: 1,
		});
		expect(data.models[0].avgTimeToFirstToken).toBe(300);
	});

	test("GET /admin/providers returns history-table totals for the selected window", async () => {
		const now = new Date();
		const from = new Date(now.getTime() - 5 * 60_000).toISOString();
		const to = new Date(now.getTime() + 60_000).toISOString();

		const res = await app.request(
			`/admin/providers?from=${encodeURIComponent(from)}&to=${encodeURIComponent(to)}`,
			{
				headers: {
					Cookie: token,
				},
			},
		);

		expect(res.status).toBe(200);
		const data = await res.json();
		const provider = data.providers.find(
			(entry: { id: string }) => entry.id === providerId,
		);

		expect(data.totalTokens).toBe(210);
		expect(data.totalCost).toBe(3.5);
		expect(provider).toMatchObject({
			id: providerId,
			logsCount: 3,
			errorsCount: 1,
			cachedCount: 1,
			modelCount: 1,
			totalTokens: 210,
			totalCost: 3.5,
		});
		expect(provider?.avgTimeToFirstToken).toBe(250);
	});

	test("GET /admin/model-provider-mappings returns mapping history totals for the selected window", async () => {
		const now = new Date();
		const from = new Date(now.getTime() - 5 * 60_000).toISOString();
		const to = new Date(now.getTime() + 60_000).toISOString();

		const res = await app.request(
			`/admin/model-provider-mappings?search=${modelId}&from=${encodeURIComponent(from)}&to=${encodeURIComponent(to)}`,
			{
				headers: {
					Cookie: token,
				},
			},
		);

		expect(res.status).toBe(200);
		const data = await res.json();

		expect(data.total).toBe(1);
		expect(data.totalTokens).toBe(210);
		expect(data.totalCost).toBe(3.5);
		expect(data.mappings).toHaveLength(1);
		expect(data.mappings[0]).toMatchObject({
			modelId,
			providerId,
			logsCount: 3,
			errorsCount: 1,
			cachedCount: 1,
		});
		expect(data.mappings[0].avgTimeToFirstToken).toBe(250);
	});
});
