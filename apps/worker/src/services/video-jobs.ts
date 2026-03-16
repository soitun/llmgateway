import { createHmac } from "node:crypto";

import {
	and,
	db,
	eq,
	type InferSelectModel,
	inArray,
	isNull,
	lte,
	or,
	tables,
	UnifiedFinishReason,
} from "@llmgateway/db";
import { logger } from "@llmgateway/logger";
import { models, type ProviderModelMapping } from "@llmgateway/models";

const MAX_WEBHOOK_ATTEMPTS = 8;
const WEBHOOK_BASE_DELAY_MS = 30_000;
const WEBHOOK_MAX_DELAY_MS = 60 * 60 * 1000;

const ACTIVE_VIDEO_STATUSES = ["queued", "in_progress"] as const;
const TERMINAL_VIDEO_STATUS_VALUES: Array<VideoJobRecord["status"]> = [
	"completed",
	"failed",
	"canceled",
	"expired",
];
const TERMINAL_VIDEO_STATUSES = new Set<string>(TERMINAL_VIDEO_STATUS_VALUES);

type VideoJobRecord = InferSelectModel<typeof tables.videoJob>;
type WebhookDeliveryRecord = InferSelectModel<typeof tables.webhookDeliveryLog>;

function joinUrl(baseUrl: string, path: string): string {
	const normalizedBaseUrl = baseUrl.endsWith("/") ? baseUrl : `${baseUrl}/`;
	const normalizedPath = path.startsWith("/") ? path.slice(1) : path;
	return new URL(normalizedPath, normalizedBaseUrl).toString();
}

function normalizeVideoStatus(value: unknown): VideoJobRecord["status"] {
	if (typeof value !== "string") {
		return "queued";
	}

	switch (value.toLowerCase()) {
		case "queued":
		case "pending":
			return "queued";
		case "in_progress":
		case "in-progress":
		case "processing":
		case "running":
			return "in_progress";
		case "completed":
		case "success":
		case "succeeded":
			return "completed";
		case "failed":
		case "error":
			return "failed";
		case "canceled":
		case "cancelled":
			return "canceled";
		case "expired":
			return "expired";
		default:
			return "queued";
	}
}

function parseTimestamp(value: unknown): Date | null {
	if (value instanceof Date) {
		return value;
	}

	if (typeof value === "number" && Number.isFinite(value)) {
		return new Date(value > 1_000_000_000_000 ? value : value * 1000);
	}

	if (typeof value === "string" && value.length > 0) {
		const numeric = Number(value);
		if (!Number.isNaN(numeric)) {
			return new Date(numeric > 1_000_000_000_000 ? numeric : numeric * 1000);
		}

		const parsed = new Date(value);
		if (!Number.isNaN(parsed.getTime())) {
			return parsed;
		}
	}

	return null;
}

function extractProgress(body: Record<string, unknown>): number {
	const candidates = [
		body.progress,
		body.progress_percent,
		body.progressPercentage,
		body.data && typeof body.data === "object"
			? (body.data as Record<string, unknown>).progress
			: undefined,
	];

	for (const candidate of candidates) {
		if (typeof candidate === "number" && Number.isFinite(candidate)) {
			return Math.max(0, Math.min(100, Math.round(candidate)));
		}
		if (typeof candidate === "string" && candidate.length > 0) {
			const parsed = Number(candidate);
			if (!Number.isNaN(parsed)) {
				return Math.max(0, Math.min(100, Math.round(parsed)));
			}
		}
	}

	return 0;
}

function extractContentUrl(body: Record<string, unknown>): string | null {
	const candidates = [
		body.url,
		body.video_url,
		body.output_url,
		body.content,
		body.output,
	];

	for (const candidate of candidates) {
		if (typeof candidate === "string" && candidate.startsWith("http")) {
			return candidate;
		}

		if (Array.isArray(candidate)) {
			for (const item of candidate) {
				if (
					item &&
					typeof item === "object" &&
					"url" in item &&
					typeof item.url === "string"
				) {
					return item.url;
				}
			}
		}

		if (candidate && typeof candidate === "object") {
			const obj = candidate as Record<string, unknown>;
			if (typeof obj.url === "string") {
				return obj.url;
			}
		}
	}

	return null;
}

function extractError(body: Record<string, unknown>): VideoJobRecord["error"] {
	const candidate =
		body.error && typeof body.error === "object"
			? (body.error as Record<string, unknown>)
			: undefined;

	if (!candidate) {
		return null;
	}

	return {
		code: typeof candidate.code === "string" ? candidate.code : undefined,
		message:
			typeof candidate.message === "string"
				? candidate.message
				: "Video generation failed",
		details: candidate,
	};
}

function toUnixTimestamp(value: Date | null): number | null {
	return value ? Math.floor(value.getTime() / 1000) : null;
}

function serializeVideoJob(job: VideoJobRecord) {
	return {
		id: job.id,
		object: "video" as const,
		model: job.model,
		status: job.status,
		progress:
			job.status === "completed"
				? 100
				: Math.max(0, Math.min(100, job.progress)),
		created_at: Math.floor(job.createdAt.getTime() / 1000),
		completed_at: toUnixTimestamp(job.completedAt),
		expires_at: toUnixTimestamp(job.expiresAt),
		error: job.error ?? null,
		content: job.contentUrl
			? [
					{
						type: "video" as const,
						url: job.contentUrl,
						mime_type: job.contentType ?? null,
					},
				]
			: undefined,
	};
}

function getRequestPrice(modelId: string, providerId: string): number {
	const model = models.find((item) => item.id === modelId);
	const mapping = model?.providers.find(
		(provider) => provider.providerId === providerId,
	) as ProviderModelMapping | undefined;
	return mapping?.requestPrice ?? 0;
}

function calculateNextWebhookRetryAt(attempt: number): Date {
	const multiplier = Math.pow(2, Math.max(0, attempt - 1));
	const delay = Math.min(
		WEBHOOK_MAX_DELAY_MS,
		WEBHOOK_BASE_DELAY_MS * multiplier,
	);
	return new Date(Date.now() + delay);
}

function createWebhookSignature(
	eventId: string,
	timestamp: string,
	payload: string,
	secret: string,
): string {
	const signedContent = `${eventId}.${timestamp}.${payload}`;
	const signature = createHmac("sha256", secret)
		.update(signedContent)
		.digest("base64");
	return `v1,${signature}`;
}

async function finalizeVideoJob(job: VideoJobRecord): Promise<void> {
	let currentJob = job;

	if (!currentJob.resultLoggedAt) {
		const now = new Date();
		const requestCost =
			currentJob.status === "completed"
				? getRequestPrice(currentJob.model, currentJob.usedProvider)
				: 0;
		const responsePayload = serializeVideoJob(currentJob);
		const responseSize = JSON.stringify(responsePayload).length;

		await db.insert(tables.log).values({
			requestId: currentJob.requestId,
			organizationId: currentJob.organizationId,
			projectId: currentJob.projectId,
			apiKeyId: currentJob.apiKeyId,
			duration: Math.max(0, Date.now() - currentJob.createdAt.getTime()),
			requestedModel: currentJob.model,
			requestedProvider: currentJob.requestedProvider,
			usedModel: currentJob.usedModel,
			usedProvider: currentJob.usedProvider,
			responseSize,
			content:
				currentJob.status === "completed" && currentJob.contentUrl
					? currentJob.contentUrl
					: null,
			finishReason:
				currentJob.status === "completed" ? "completed" : "upstream_error",
			unifiedFinishReason:
				currentJob.status === "completed"
					? UnifiedFinishReason.COMPLETED
					: UnifiedFinishReason.UPSTREAM_ERROR,
			hasError: currentJob.status !== "completed",
			errorDetails: currentJob.error
				? {
						statusCode: 502,
						statusText: currentJob.status,
						responseText: currentJob.error.message,
					}
				: null,
			cost: requestCost,
			requestCost,
			estimatedCost: false,
			mode: currentJob.mode,
			usedMode: currentJob.usedMode,
			rawResponse: responsePayload,
			upstreamResponse: currentJob.upstreamStatusResponse,
			processedAt: null,
			dataStorageCost: "0",
		});

		await db
			.update(tables.videoJob)
			.set({
				resultLoggedAt: now,
			})
			.where(eq(tables.videoJob.id, currentJob.id));

		currentJob = {
			...currentJob,
			resultLoggedAt: now,
		};
	}

	if (
		currentJob.callbackUrl &&
		currentJob.callbackSecret &&
		currentJob.callbackStatus === "pending" &&
		!currentJob.callbackEventId
	) {
		const eventId = `evt_${currentJob.id}`;
		const eventType =
			currentJob.status === "completed" ? "video.completed" : "video.failed";

		await db.transaction(async (tx) => {
			await tx
				.update(tables.videoJob)
				.set({
					callbackEventId: eventId,
					callbackEventType: eventType,
				})
				.where(eq(tables.videoJob.id, currentJob.id));

			await tx.insert(tables.webhookDeliveryLog).values({
				videoJobId: currentJob.id,
				eventId,
				eventType,
				targetUrl: currentJob.callbackUrl!,
				attempt: 1,
				status: "pending",
				nextRetryAt: new Date(),
			});
		});
	}
}

async function fetchUpstreamStatus(
	job: VideoJobRecord,
): Promise<Record<string, unknown>> {
	const url = joinUrl(job.providerBaseUrl, `/v1/videos/${job.upstreamId}`);
	const response = await fetch(url, {
		method: "GET",
		headers: {
			Authorization: `Bearer ${job.providerToken}`,
		},
	});
	const text = await response.text();

	let body: Record<string, unknown> = {};
	if (text.length > 0) {
		try {
			body = JSON.parse(text) as Record<string, unknown>;
		} catch {
			body = {
				error: {
					message: text,
				},
			};
		}
	}

	if (!response.ok) {
		throw new Error(
			typeof body.error === "object" &&
			body.error &&
			"message" in body.error &&
			typeof body.error.message === "string"
				? body.error.message
				: `Upstream status request failed with status ${response.status}`,
		);
	}

	return body;
}

export async function processPendingVideoJobs(): Promise<void> {
	const now = new Date();
	const jobsToPoll = await db
		.select()
		.from(tables.videoJob)
		.where(
			and(
				inArray(tables.videoJob.status, [...ACTIVE_VIDEO_STATUSES]),
				lte(tables.videoJob.nextPollAt, now),
			),
		)
		.limit(25);

	for (const job of jobsToPoll) {
		try {
			const upstreamStatus = await fetchUpstreamStatus(job);
			const status = normalizeVideoStatus(upstreamStatus.status);
			const progress = extractProgress(upstreamStatus);
			const isTerminal = TERMINAL_VIDEO_STATUSES.has(status);
			const completedAt =
				parseTimestamp(upstreamStatus.completed_at) ??
				(isTerminal ? new Date() : job.completedAt);

			const updatedJob = await db
				.update(tables.videoJob)
				.set({
					status,
					progress,
					error: extractError(upstreamStatus),
					contentUrl: extractContentUrl(upstreamStatus) ?? job.contentUrl,
					contentType:
						typeof upstreamStatus.mime_type === "string"
							? upstreamStatus.mime_type
							: job.contentType,
					completedAt,
					expiresAt: parseTimestamp(upstreamStatus.expires_at) ?? job.expiresAt,
					lastPolledAt: now,
					nextPollAt: isTerminal ? now : new Date(Date.now() + 5_000),
					pollAttemptCount: job.pollAttemptCount + 1,
					upstreamStatusResponse: upstreamStatus,
				})
				.where(eq(tables.videoJob.id, job.id))
				.returning()
				.then((rows) => rows[0]);

			if (TERMINAL_VIDEO_STATUSES.has(updatedJob.status)) {
				await finalizeVideoJob(updatedJob);
			}
		} catch (error) {
			logger.error(
				"Error polling video job",
				error instanceof Error ? error : new Error(String(error)),
				{
					videoJobId: job.id,
					upstreamId: job.upstreamId,
				},
			);

			await db
				.update(tables.videoJob)
				.set({
					lastPolledAt: now,
					nextPollAt: new Date(Date.now() + 10_000),
					pollAttemptCount: job.pollAttemptCount + 1,
				})
				.where(eq(tables.videoJob.id, job.id));
		}
	}

	const terminalJobsToFinalize = await db
		.select()
		.from(tables.videoJob)
		.where(
			and(
				inArray(tables.videoJob.status, TERMINAL_VIDEO_STATUS_VALUES),
				or(
					isNull(tables.videoJob.resultLoggedAt),
					and(
						eq(tables.videoJob.callbackStatus, "pending"),
						isNull(tables.videoJob.callbackEventId),
					),
				),
			),
		)
		.limit(25);

	for (const job of terminalJobsToFinalize) {
		await finalizeVideoJob(job);
	}
}

async function markVideoCallbackFailed(videoJobId: string): Promise<void> {
	const pendingOrRetrying = await db
		.select({ id: tables.webhookDeliveryLog.id })
		.from(tables.webhookDeliveryLog)
		.where(
			and(
				eq(tables.webhookDeliveryLog.videoJobId, videoJobId),
				inArray(tables.webhookDeliveryLog.status, ["pending", "retrying"]),
			),
		)
		.limit(1);

	if (pendingOrRetrying.length === 0) {
		await db
			.update(tables.videoJob)
			.set({
				callbackStatus: "failed",
			})
			.where(eq(tables.videoJob.id, videoJobId));
	}
}

async function deliverWebhook(
	delivery: WebhookDeliveryRecord,
	job: VideoJobRecord,
): Promise<void> {
	const timestamp = Math.floor(Date.now() / 1000).toString();
	const payload = JSON.stringify({
		object: "event",
		id: delivery.eventId,
		type: delivery.eventType,
		created_at: Math.floor(Date.now() / 1000),
		data: serializeVideoJob(job),
	});

	const headers = {
		"Content-Type": "application/json",
		"webhook-id": delivery.eventId,
		"webhook-timestamp": timestamp,
		"webhook-signature": createWebhookSignature(
			delivery.eventId,
			timestamp,
			payload,
			job.callbackSecret!,
		),
	};

	const startedAt = new Date();

	try {
		const response = await fetch(delivery.targetUrl, {
			method: "POST",
			headers,
			body: payload,
			redirect: "manual",
		});
		const responseText = (await response.text()).slice(0, 4000);

		if (response.ok) {
			await db.transaction(async (tx) => {
				await tx
					.update(tables.webhookDeliveryLog)
					.set({
						status: "delivered",
						lastTriedAt: startedAt,
						deliveredAt: new Date(),
						requestHeaders: headers,
						requestBody: JSON.parse(payload) as Record<string, unknown>,
						responseStatus: response.status,
						responseBody: responseText,
						error: null,
					})
					.where(eq(tables.webhookDeliveryLog.id, delivery.id));

				await tx
					.update(tables.videoJob)
					.set({
						callbackStatus: "delivered",
						callbackDeliveredAt: new Date(),
					})
					.where(eq(tables.videoJob.id, job.id));
			});

			return;
		}

		if (delivery.attempt >= MAX_WEBHOOK_ATTEMPTS) {
			await db
				.update(tables.webhookDeliveryLog)
				.set({
					status: "failed",
					lastTriedAt: startedAt,
					requestHeaders: headers,
					requestBody: JSON.parse(payload) as Record<string, unknown>,
					responseStatus: response.status,
					responseBody: responseText,
					error: `Unexpected webhook response status ${response.status}`,
				})
				.where(eq(tables.webhookDeliveryLog.id, delivery.id));

			await markVideoCallbackFailed(job.id);
			return;
		}

		await db.transaction(async (tx) => {
			await tx
				.update(tables.webhookDeliveryLog)
				.set({
					status: "retrying",
					lastTriedAt: startedAt,
					requestHeaders: headers,
					requestBody: JSON.parse(payload) as Record<string, unknown>,
					responseStatus: response.status,
					responseBody: responseText,
					error: `Unexpected webhook response status ${response.status}`,
				})
				.where(eq(tables.webhookDeliveryLog.id, delivery.id));

			await tx.insert(tables.webhookDeliveryLog).values({
				videoJobId: job.id,
				eventId: delivery.eventId,
				eventType: delivery.eventType,
				targetUrl: delivery.targetUrl,
				attempt: delivery.attempt + 1,
				status: "pending",
				nextRetryAt: calculateNextWebhookRetryAt(delivery.attempt),
			});
		});
	} catch (error) {
		const message =
			error instanceof Error ? error.message : "Unknown webhook delivery error";

		if (delivery.attempt >= MAX_WEBHOOK_ATTEMPTS) {
			await db
				.update(tables.webhookDeliveryLog)
				.set({
					status: "failed",
					lastTriedAt: startedAt,
					requestHeaders: headers,
					requestBody: JSON.parse(payload) as Record<string, unknown>,
					error: message,
				})
				.where(eq(tables.webhookDeliveryLog.id, delivery.id));

			await markVideoCallbackFailed(job.id);
			return;
		}

		await db.transaction(async (tx) => {
			await tx
				.update(tables.webhookDeliveryLog)
				.set({
					status: "retrying",
					lastTriedAt: startedAt,
					requestHeaders: headers,
					requestBody: JSON.parse(payload) as Record<string, unknown>,
					error: message,
				})
				.where(eq(tables.webhookDeliveryLog.id, delivery.id));

			await tx.insert(tables.webhookDeliveryLog).values({
				videoJobId: job.id,
				eventId: delivery.eventId,
				eventType: delivery.eventType,
				targetUrl: delivery.targetUrl,
				attempt: delivery.attempt + 1,
				status: "pending",
				nextRetryAt: calculateNextWebhookRetryAt(delivery.attempt),
			});
		});
	}
}

export async function processPendingWebhookDeliveries(): Promise<void> {
	const dueDeliveries = await db
		.select()
		.from(tables.webhookDeliveryLog)
		.where(
			and(
				eq(tables.webhookDeliveryLog.status, "pending"),
				lte(tables.webhookDeliveryLog.nextRetryAt, new Date()),
			),
		)
		.limit(25);

	for (const delivery of dueDeliveries) {
		const job = await db
			.select()
			.from(tables.videoJob)
			.where(eq(tables.videoJob.id, delivery.videoJobId))
			.limit(1)
			.then((rows) => rows[0]);

		if (!job || !job.callbackUrl || !job.callbackSecret) {
			await db
				.update(tables.webhookDeliveryLog)
				.set({
					status: "failed",
					lastTriedAt: new Date(),
					error: "Video job or callback configuration no longer exists",
				})
				.where(eq(tables.webhookDeliveryLog.id, delivery.id));
			if (job) {
				await markVideoCallbackFailed(job.id);
			}
			continue;
		}

		await deliverWebhook(delivery, job);
	}
}
