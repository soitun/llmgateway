import { createRoute, OpenAPIHono, z } from "@hono/zod-openapi";
import { HTTPException } from "hono/http-exception";

import { getProviderEnv } from "@/chat/tools/get-provider-env.js";
import {
	findApiKeyByToken,
	findOrganizationById,
	findProjectById,
	findProviderKey,
} from "@/lib/cached-queries.js";
import { validateModelAccess } from "@/lib/iam.js";

import { getProviderHeaders } from "@llmgateway/actions";
import {
	and,
	db,
	eq,
	shortid,
	tables,
	type InferSelectModel,
} from "@llmgateway/db";
import { logger } from "@llmgateway/logger";
import {
	getProviderEnvValue,
	hasProviderEnvironmentToken,
	models,
	type ModelDefinition,
	type Provider,
	type ProviderModelMapping,
} from "@llmgateway/models";
import {
	buildVertexVideoOutputStorageUri,
	createSignedGcsReadUrl,
	getGoogleVertexVideoOutputBucket,
	getGoogleVertexVideoOutputPrefix,
	parseGcsUri,
} from "@llmgateway/shared/gcs";

import type { ServerTypes } from "@/vars.js";
import type { Context } from "hono";

const TERMINAL_VIDEO_STATUSES = new Set([
	"completed",
	"failed",
	"canceled",
	"expired",
]);
const MIN_VIDEO_GENERATION_BALANCE = 1;
const DEFAULT_VIDEO_SIZE = "1280x720";
const SUPPORTED_VEO_VIDEO_SIZES = {
	"1280x720": {
		size: "1280x720",
		width: 1280,
		height: 720,
		resolution: "720p",
		orientation: "landscape",
	},
	"720x1280": {
		size: "720x1280",
		width: 720,
		height: 1280,
		resolution: "720p",
		orientation: "portrait",
	},
	"1920x1080": {
		size: "1920x1080",
		width: 1920,
		height: 1080,
		resolution: "1080p",
		orientation: "landscape",
	},
	"1080x1920": {
		size: "1080x1920",
		width: 1080,
		height: 1920,
		resolution: "1080p",
		orientation: "portrait",
	},
	"3840x2160": {
		size: "3840x2160",
		width: 3840,
		height: 2160,
		resolution: "4k",
		orientation: "landscape",
	},
	"2160x3840": {
		size: "2160x3840",
		width: 2160,
		height: 3840,
		resolution: "4k",
		orientation: "portrait",
	},
} as const;

type SupportedVeoVideoSize = keyof typeof SUPPORTED_VEO_VIDEO_SIZES;
type VideoSizeConfig =
	(typeof SUPPORTED_VEO_VIDEO_SIZES)[SupportedVeoVideoSize];

const createVideoRequestSchema = z
	.object({
		model: z.string().default("veo-3.1-generate-preview").openapi({
			description:
				"The video generation model to use. Supported values: veo-3.1-generate-preview, veo-3.1-fast-generate-preview, and their obsidian/, avalanche/, or google-vertex/ prefixed variants.",
			example: "veo-3.1-generate-preview",
		}),
		prompt: z.string().min(1).openapi({
			description: "Text prompt describing the video to generate.",
			example:
				"A cinematic drone shot flying through a neon-lit futuristic city at night",
		}),
		size: z.string().optional().openapi({
			description:
				"Output resolution in OpenAI widthxheight format. Obsidian supports 1280x720 and 720x1280. Avalanche supports 1920x1080, 1080x1920, 3840x2160, and 2160x3840. Google Vertex supports 1280x720, 720x1280, 1920x1080, 1080x1920, 3840x2160, and 2160x3840.",
			example: "1280x720",
		}),
		callback_url: z.string().url().optional().openapi({
			description:
				"LLMGateway extension. When set, a signed webhook is delivered after the job reaches a terminal state.",
			example: "https://example.com/webhooks/video",
		}),
		callback_secret: z.string().min(1).optional().openapi({
			description:
				"LLMGateway extension. Shared secret used to sign webhook deliveries with HMAC-SHA256.",
			example: "whsec_test_secret",
		}),
		input_reference: z.unknown().optional(),
		seconds: z.number().int().openapi({
			description:
				"Output duration in seconds. Veo 3.1 supports 4, 6, or 8 seconds on Google Vertex. Other providers currently only support the default 8-second output.",
			example: 8,
		}),
		n: z.number().int().optional(),
		image: z.unknown().optional(),
	})
	.superRefine((value, ctx) => {
		const hasCallbackUrl = value.callback_url !== undefined;
		const hasCallbackSecret = value.callback_secret !== undefined;

		if (hasCallbackUrl !== hasCallbackSecret) {
			ctx.addIssue({
				code: z.ZodIssueCode.custom,
				message:
					"callback_url and callback_secret must either both be provided or both be omitted",
				path: hasCallbackUrl ? ["callback_secret"] : ["callback_url"],
			});
		}

		if (value.n !== undefined && value.n !== 1) {
			ctx.addIssue({
				code: z.ZodIssueCode.custom,
				message: "Only n=1 is supported for Veo 3.1 preview models",
				path: ["n"],
			});
		}

		if (
			value.size !== undefined &&
			!(value.size in SUPPORTED_VEO_VIDEO_SIZES)
		) {
			ctx.addIssue({
				code: z.ZodIssueCode.custom,
				message:
					"size must be one of 1280x720, 720x1280, 1920x1080, 1080x1920, 3840x2160, or 2160x3840",
				path: ["size"],
			});
		}

		if (value.seconds !== undefined && ![4, 6, 8].includes(value.seconds)) {
			ctx.addIssue({
				code: z.ZodIssueCode.custom,
				message: "seconds must be one of 4, 6, or 8",
				path: ["seconds"],
			});
		}

		for (const key of ["input_reference", "image"] as const) {
			if (value[key] !== undefined) {
				ctx.addIssue({
					code: z.ZodIssueCode.custom,
					message: `${key} is not supported for Veo 3.1 preview models yet`,
					path: [key],
				});
			}
		}
	});

const videoErrorSchema = z.object({
	code: z.string().optional(),
	message: z.string(),
	details: z.unknown().optional(),
});

const videoContentSchema = z.array(
	z.object({
		type: z.literal("video"),
		url: z.string().url(),
		mime_type: z.string().nullable().optional(),
	}),
);

const videoResponseSchema = z.object({
	id: z.string(),
	object: z.literal("video"),
	model: z.string(),
	status: z.enum([
		"queued",
		"in_progress",
		"completed",
		"failed",
		"canceled",
		"expired",
	]),
	progress: z.number().int().min(0).max(100).nullable(),
	created_at: z.number(),
	completed_at: z.number().nullable(),
	expires_at: z.number().nullable(),
	error: videoErrorSchema.nullable(),
	content: videoContentSchema.optional(),
});

const createVideo = createRoute({
	operationId: "v1_videos_create",
	summary: "Create video",
	description:
		"Creates a new asynchronous video generation job using an OpenAI-compatible request format.",
	method: "post",
	path: "/",
	security: [
		{
			bearerAuth: [],
		},
	],
	request: {
		body: {
			content: {
				"application/json": {
					schema: createVideoRequestSchema,
				},
			},
		},
	},
	responses: {
		200: {
			content: {
				"application/json": {
					schema: videoResponseSchema,
				},
			},
			description: "Video job created.",
		},
	},
});

const getVideo = createRoute({
	operationId: "v1_videos_retrieve",
	summary: "Retrieve video",
	description: "Retrieves the current state of a video generation job.",
	method: "get",
	path: "/{video_id}",
	security: [
		{
			bearerAuth: [],
		},
	],
	request: {
		params: z.object({
			video_id: z.string(),
		}),
	},
	responses: {
		200: {
			content: {
				"application/json": {
					schema: videoResponseSchema,
				},
			},
			description: "Video job state.",
		},
	},
});

const getVideoContent = createRoute({
	operationId: "v1_videos_content",
	summary: "Video content",
	description:
		"Streams the generated video content once the job has completed successfully.",
	method: "get",
	path: "/{video_id}/content",
	security: [
		{
			bearerAuth: [],
		},
	],
	request: {
		params: z.object({
			video_id: z.string(),
		}),
	},
	responses: {
		200: {
			content: {
				"video/mp4": {
					schema: z.any(),
				},
				"application/octet-stream": {
					schema: z.any(),
				},
			},
			description: "Video bytes.",
		},
	},
});

type VideoJobRecord = InferSelectModel<typeof tables.videoJob>;

interface RequestContext {
	apiKey: InferSelectModel<typeof tables.apiKey>;
	project: InferSelectModel<typeof tables.project>;
	organization: InferSelectModel<typeof tables.organization>;
	requestId: string;
}

interface ProviderContext {
	providerId: Provider;
	baseUrl: string;
	token: string;
	usedMode: "api-keys" | "credits";
	vertexProjectId?: string;
	vertexRegion?: string;
}

interface ResolvedVideoExecution {
	providerMapping: ProviderModelMapping;
	providerContext: ProviderContext;
	upstreamModelName: string;
}

interface ParsedVideoRequest {
	rawBody: unknown;
	request: z.infer<typeof createVideoRequestSchema>;
}

function getAvailableCredits(
	organization: InferSelectModel<typeof tables.organization>,
): number {
	const regularCredits = parseFloat(organization.credits ?? "0");
	const devPlanCreditsRemaining =
		organization.devPlan !== "none"
			? parseFloat(organization.devPlanCreditsLimit ?? "0") -
				parseFloat(organization.devPlanCreditsUsed ?? "0")
			: 0;
	return regularCredits + devPlanCreditsRemaining;
}

function extractToken(c: Context): string {
	const auth = c.req.header("Authorization");
	const xApiKey = c.req.header("x-api-key");

	if (auth) {
		const split = auth.split("Bearer ");
		if (split.length === 2 && split[1]) {
			return split[1];
		}
	}

	if (xApiKey) {
		return xApiKey;
	}

	throw new HTTPException(401, {
		message:
			"Unauthorized: No API key provided. Expected 'Authorization: Bearer your-api-token' header or 'x-api-key: your-api-token' header",
	});
}

async function requireRequestContext(c: Context): Promise<RequestContext> {
	const token = extractToken(c);
	const apiKey = await findApiKeyByToken(token);

	if (!apiKey || apiKey.status !== "active") {
		throw new HTTPException(401, {
			message:
				"Unauthorized: Invalid LLMGateway API token. Please make sure the token is not deleted or disabled. Go to the LLMGateway 'API Keys' page to generate a new token.",
		});
	}

	if (apiKey.usageLimit && Number(apiKey.usage) >= Number(apiKey.usageLimit)) {
		throw new HTTPException(401, {
			message: "Unauthorized: LLMGateway API key reached its usage limit.",
		});
	}

	const project = await findProjectById(apiKey.projectId);
	if (!project) {
		throw new HTTPException(500, {
			message: "Could not find project",
		});
	}

	if (project.status === "deleted") {
		throw new HTTPException(410, {
			message: "Project has been archived and is no longer accessible",
		});
	}

	const organization = await findOrganizationById(project.organizationId);
	if (!organization) {
		throw new HTTPException(500, {
			message: "Could not find organization",
		});
	}

	return {
		apiKey,
		project,
		organization,
		requestId: c.req.header("x-request-id") ?? shortid(40),
	};
}

function getVideoModel(model: string): {
	normalizedModel: string;
	requestedProvider: string | undefined;
} {
	const supportedModels = new Set([
		"veo-3.1-generate-preview",
		"veo-3.1-fast-generate-preview",
	]);

	if (supportedModels.has(model)) {
		return {
			normalizedModel: model,
			requestedProvider: undefined,
		};
	}

	for (const providerId of [
		"obsidian",
		"avalanche",
		"google-vertex",
	] as const) {
		if (model.startsWith(`${providerId}/`)) {
			const normalizedModel = model.slice(`${providerId}/`.length);
			if (supportedModels.has(normalizedModel)) {
				return {
					normalizedModel,
					requestedProvider: providerId,
				};
			}
		}
	}

	throw new HTTPException(400, {
		message:
			"Unsupported video model. Only veo-3.1-generate-preview, veo-3.1-fast-generate-preview, and their obsidian/, avalanche/, or google-vertex/ prefixed variants are supported right now.",
	});
}

function getVideoSizeConfig(size: string | undefined): VideoSizeConfig {
	const normalizedSize = size ?? DEFAULT_VIDEO_SIZE;
	return SUPPORTED_VEO_VIDEO_SIZES[normalizedSize as SupportedVeoVideoSize];
}

function getEligibleVideoProviderMappings(
	modelInfo: ModelDefinition,
	requestedProvider: string | undefined,
	videoSize: VideoSizeConfig,
): ProviderModelMapping[] {
	const candidateProviders = modelInfo.providers.filter((provider) => {
		if (!provider.videoGenerations) {
			return false;
		}

		if (requestedProvider && provider.providerId !== requestedProvider) {
			return false;
		}

		return true;
	});

	const matchingProviders = candidateProviders.filter((provider) => {
		if (!provider.supportedVideoSizes?.length) {
			return true;
		}

		return provider.supportedVideoSizes.includes(videoSize.size);
	});

	if (matchingProviders.length === 0) {
		const providerLabel =
			requestedProvider ??
			candidateProviders.map((provider) => provider.providerId).join(", ") ??
			"the available providers";
		throw new HTTPException(400, {
			message: `Requested size ${videoSize.size} is not supported for model ${modelInfo.id} on ${providerLabel}.`,
		});
	}

	return matchingProviders;
}

function getObsidianVideoModelName(
	baseModelName: string,
	videoSize: VideoSizeConfig,
): string {
	if (videoSize.orientation === "landscape") {
		return `${baseModelName}-landscape`;
	}

	return baseModelName;
}

function getAvalancheVideoModelName(baseModelName: string): string {
	return baseModelName;
}

function getVideoUpstreamModelName(
	providerId: Provider,
	baseModelName: string,
	videoSize: VideoSizeConfig,
): string {
	switch (providerId) {
		case "obsidian":
			return getObsidianVideoModelName(baseModelName, videoSize);
		case "avalanche":
			return getAvalancheVideoModelName(baseModelName);
		case "google-vertex":
			return baseModelName;
		default:
			return baseModelName;
	}
}

function getAvalancheAspectRatio(videoSize: VideoSizeConfig): "16:9" | "9:16" {
	return videoSize.orientation === "portrait" ? "9:16" : "16:9";
}

function getVertexAspectRatio(videoSize: VideoSizeConfig): "16:9" | "9:16" {
	return videoSize.orientation === "portrait" ? "9:16" : "16:9";
}

function getVertexResolution(
	videoSize: VideoSizeConfig,
): "720p" | "1080p" | "4k" {
	switch (videoSize.resolution) {
		case "1080p":
			return "1080p";
		case "4k":
			return "4k";
		default:
			return "720p";
	}
}

function getDefaultVideoProviderBaseUrl(providerId: Provider): string | null {
	switch (providerId) {
		case "google-vertex":
			return "https://us-central1-aiplatform.googleapis.com";
		default:
			return null;
	}
}

function addRequestedVideoMetadata(
	body: Record<string, unknown>,
	videoSize: VideoSizeConfig,
): Record<string, unknown> {
	return {
		...body,
		size:
			typeof body.size === "string" && body.size.length > 0
				? body.size
				: videoSize.size,
		resolution:
			typeof body.resolution === "string" && body.resolution.length > 0
				? body.resolution
				: videoSize.resolution,
		width:
			typeof body.width === "number" && Number.isFinite(body.width)
				? body.width
				: videoSize.width,
		height:
			typeof body.height === "number" && Number.isFinite(body.height)
				? body.height
				: videoSize.height,
	};
}

async function resolveProviderContext(
	providerId: Provider,
	project: InferSelectModel<typeof tables.project>,
	organizationId: string,
): Promise<ProviderContext> {
	const defaultBaseUrl = getDefaultVideoProviderBaseUrl(providerId);
	const sharedVertexProjectId =
		providerId === "google-vertex"
			? getProviderEnvValue("google-vertex", "project")
			: undefined;
	const sharedVertexRegion =
		providerId === "google-vertex"
			? (getProviderEnvValue(
					"google-vertex",
					"region",
					undefined,
					"us-central1",
				) ?? "us-central1")
			: undefined;

	if (project.mode === "api-keys") {
		const providerKey = await findProviderKey(organizationId, providerId);
		if (!providerKey) {
			throw new HTTPException(400, {
				message: `No API key set for provider: ${providerId}. Please add a provider key in your settings or add credits and switch to credits or hybrid mode.`,
			});
		}

		const baseUrl =
			providerKey.baseUrl ??
			getProviderEnvValue(providerId, "baseUrl") ??
			defaultBaseUrl;
		if (!baseUrl) {
			throw new HTTPException(400, {
				message: `No base URL set for provider: ${providerId}`,
			});
		}

		if (providerId === "google-vertex" && !sharedVertexProjectId) {
			throw new HTTPException(500, {
				message:
					"LLM_GOOGLE_CLOUD_PROJECT environment variable is required for google-vertex video generation",
			});
		}

		return {
			providerId,
			baseUrl,
			token: providerKey.token,
			usedMode: "api-keys",
			vertexProjectId: sharedVertexProjectId,
			vertexRegion: sharedVertexRegion,
		};
	}

	if (project.mode === "credits") {
		const env = getProviderEnv(providerId);
		const baseUrl =
			getProviderEnvValue(providerId, "baseUrl", env.configIndex) ??
			defaultBaseUrl;
		if (!baseUrl) {
			throw new HTTPException(500, {
				message: `Base URL environment variable is required for ${providerId} provider`,
			});
		}

		const vertexProjectId =
			providerId === "google-vertex"
				? getProviderEnvValue("google-vertex", "project", env.configIndex)
				: undefined;
		const vertexRegion =
			providerId === "google-vertex"
				? (getProviderEnvValue(
						"google-vertex",
						"region",
						env.configIndex,
						"us-central1",
					) ?? "us-central1")
				: undefined;

		if (providerId === "google-vertex" && !vertexProjectId) {
			throw new HTTPException(500, {
				message:
					"LLM_GOOGLE_CLOUD_PROJECT environment variable is required for google-vertex video generation",
			});
		}

		return {
			providerId,
			baseUrl,
			token: env.token,
			usedMode: "credits",
			vertexProjectId,
			vertexRegion,
		};
	}

	const providerKey = await findProviderKey(organizationId, providerId);
	if (providerKey) {
		const baseUrl =
			providerKey.baseUrl ??
			getProviderEnvValue(providerId, "baseUrl") ??
			defaultBaseUrl;
		if (!baseUrl) {
			throw new HTTPException(400, {
				message: `No base URL set for provider: ${providerId}`,
			});
		}

		if (providerId === "google-vertex" && !sharedVertexProjectId) {
			throw new HTTPException(500, {
				message:
					"LLM_GOOGLE_CLOUD_PROJECT environment variable is required for google-vertex video generation",
			});
		}

		return {
			providerId,
			baseUrl,
			token: providerKey.token,
			usedMode: "api-keys",
			vertexProjectId: sharedVertexProjectId,
			vertexRegion: sharedVertexRegion,
		};
	}

	if (!hasProviderEnvironmentToken(providerId)) {
		throw new HTTPException(400, {
			message: `No provider key or environment token set for provider: ${providerId}. Please add the provider key in the settings or switch the project mode to credits or hybrid.`,
		});
	}

	const env = getProviderEnv(providerId);
	const baseUrl =
		getProviderEnvValue(providerId, "baseUrl", env.configIndex) ??
		defaultBaseUrl;
	if (!baseUrl) {
		throw new HTTPException(500, {
			message: `Base URL environment variable is required for ${providerId} provider`,
		});
	}

	const vertexProjectId =
		providerId === "google-vertex"
			? getProviderEnvValue("google-vertex", "project", env.configIndex)
			: undefined;
	const vertexRegion =
		providerId === "google-vertex"
			? (getProviderEnvValue(
					"google-vertex",
					"region",
					env.configIndex,
					"us-central1",
				) ?? "us-central1")
			: undefined;

	if (providerId === "google-vertex" && !vertexProjectId) {
		throw new HTTPException(500, {
			message:
				"LLM_GOOGLE_CLOUD_PROJECT environment variable is required for google-vertex video generation",
		});
	}

	return {
		providerId,
		baseUrl,
		token: env.token,
		usedMode: "credits",
		vertexProjectId,
		vertexRegion,
	};
}

async function hasVideoProviderConfiguration(
	providerId: Provider,
	project: InferSelectModel<typeof tables.project>,
	organizationId: string,
): Promise<boolean> {
	const defaultBaseUrl = getDefaultVideoProviderBaseUrl(providerId);

	if (project.mode === "api-keys") {
		const providerKey = await findProviderKey(organizationId, providerId);
		return Boolean(
			providerKey &&
				(providerKey.baseUrl ??
					getProviderEnvValue(providerId, "baseUrl") ??
					defaultBaseUrl) &&
				(providerId !== "google-vertex" ||
					Boolean(getProviderEnvValue("google-vertex", "project"))),
		);
	}

	if (project.mode === "credits") {
		return Boolean(
			hasProviderEnvironmentToken(providerId) &&
				(getProviderEnvValue(
					providerId,
					"baseUrl",
					getProviderEnv(providerId).configIndex,
				) ??
					defaultBaseUrl) &&
				(providerId !== "google-vertex" ||
					Boolean(
						getProviderEnvValue(
							"google-vertex",
							"project",
							getProviderEnv(providerId).configIndex,
						),
					)),
		);
	}

	const providerKey = await findProviderKey(organizationId, providerId);
	if (providerKey) {
		return Boolean(
			(providerKey.baseUrl ??
				getProviderEnvValue(providerId, "baseUrl") ??
				defaultBaseUrl) &&
				(providerId !== "google-vertex" ||
					Boolean(getProviderEnvValue("google-vertex", "project"))),
		);
	}

	return Boolean(
		hasProviderEnvironmentToken(providerId) &&
			(getProviderEnvValue(
				providerId,
				"baseUrl",
				getProviderEnv(providerId).configIndex,
			) ??
				defaultBaseUrl) &&
			(providerId !== "google-vertex" ||
				Boolean(
					getProviderEnvValue(
						"google-vertex",
						"project",
						getProviderEnv(providerId).configIndex,
					),
				)),
	);
}

async function resolveVideoExecution(
	modelInfo: ModelDefinition,
	requestedProvider: string | undefined,
	videoSize: VideoSizeConfig,
	project: InferSelectModel<typeof tables.project>,
	organizationId: string,
): Promise<ResolvedVideoExecution> {
	const eligibleMappings = getEligibleVideoProviderMappings(
		modelInfo,
		requestedProvider,
		videoSize,
	);
	const errors: string[] = [];

	for (const providerMapping of eligibleMappings) {
		try {
			const providerContext = await resolveProviderContext(
				providerMapping.providerId as Provider,
				project,
				organizationId,
			);
			return {
				providerMapping,
				providerContext,
				upstreamModelName: getVideoUpstreamModelName(
					providerMapping.providerId as Provider,
					providerMapping.modelName,
					videoSize,
				),
			};
		} catch (error) {
			errors.push(error instanceof Error ? error.message : String(error));
		}
	}

	if (!requestedProvider) {
		const configuredProviders: Provider[] = [];
		for (const providerMapping of modelInfo.providers) {
			const providerId = providerMapping.providerId as Provider;
			if (
				providerMapping.videoGenerations &&
				(await hasVideoProviderConfiguration(
					providerId,
					project,
					organizationId,
				))
			) {
				configuredProviders.push(providerId);
			}
		}

		if (configuredProviders.length > 0) {
			const configuredEligibleMappings = eligibleMappings.filter((provider) =>
				configuredProviders.includes(provider.providerId as Provider),
			);
			if (configuredEligibleMappings.length === 0) {
				throw new HTTPException(400, {
					message: `Requested size ${videoSize.size} is not supported by the configured providers for model ${modelInfo.id}. Configured providers: ${configuredProviders.join(", ")}.`,
				});
			}
		}
	}

	throw new HTTPException(400, {
		message:
			errors[0] ??
			`No configured provider is available for model ${modelInfo.id} and size ${videoSize.size}.`,
	});
}

function joinUrl(baseUrl: string, path: string): string {
	const normalizedBaseUrl = baseUrl.endsWith("/") ? baseUrl : `${baseUrl}/`;
	const normalizedPath = path.startsWith("/") ? path.slice(1) : path;
	return new URL(normalizedPath, normalizedBaseUrl).toString();
}

function appendQueryParam(url: string, key: string, value: string): string {
	const resolvedUrl = new URL(url);
	resolvedUrl.searchParams.set(key, value);
	return resolvedUrl.toString();
}

function getVideoDurationSeconds(seconds: number): number {
	return seconds;
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
		case "succeeded":
		case "success":
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
		const asNumber = Number(value);
		if (!Number.isNaN(asNumber)) {
			return new Date(
				asNumber > 1_000_000_000_000 ? asNumber : asNumber * 1000,
			);
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

function extractStorageUri(body: Record<string, unknown>): string | null {
	const candidates = [
		body.gcsUri,
		body.storage_uri,
		body.storageUri,
		body.output_gcs_uri,
	];

	for (const candidate of candidates) {
		if (typeof candidate === "string" && candidate.startsWith("gs://")) {
			return candidate;
		}
	}

	const response =
		body.response && typeof body.response === "object"
			? (body.response as Record<string, unknown>)
			: null;
	const videos =
		response && Array.isArray(response.videos) ? response.videos : null;
	const firstVideo =
		videos && videos[0] && typeof videos[0] === "object"
			? (videos[0] as Record<string, unknown>)
			: null;

	return firstVideo && typeof firstVideo.gcsUri === "string"
		? firstVideo.gcsUri
		: null;
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

async function getExternalVideoContentUrl(
	job: VideoJobRecord,
): Promise<string | null> {
	if (job.storageUri) {
		try {
			return await createSignedGcsReadUrl(job.storageUri);
		} catch (error) {
			logger.error(
				"Failed to create signed URL for video job",
				error instanceof Error ? error : new Error(String(error)),
				{
					videoJobId: job.id,
					storageUri: job.storageUri,
				},
			);
		}
	}

	return job.contentUrl;
}

async function serializeVideoJob(job: VideoJobRecord) {
	const contentUrl = await getExternalVideoContentUrl(job);

	return {
		id: job.id,
		object: "video" as const,
		model: job.model,
		status: job.status,
		progress: TERMINAL_VIDEO_STATUSES.has(job.status)
			? job.status === "completed"
				? 100
				: job.progress
			: job.progress,
		created_at: Math.floor(job.createdAt.getTime() / 1000),
		completed_at: toUnixTimestamp(job.completedAt),
		expires_at: toUnixTimestamp(job.expiresAt),
		error: job.error ?? null,
		content: contentUrl
			? [
					{
						type: "video" as const,
						url: contentUrl,
						mime_type: job.contentType ?? null,
					},
				]
			: undefined,
	};
}

function getGoogleVertexInlineVideo(
	job: VideoJobRecord,
): { bytesBase64Encoded: string; mimeType: string } | null {
	const candidates = [job.upstreamStatusResponse, job.upstreamCreateResponse];

	for (const candidate of candidates) {
		if (!candidate || typeof candidate !== "object") {
			continue;
		}

		const response =
			"response" in candidate &&
			candidate.response &&
			typeof candidate.response === "object"
				? (candidate.response as Record<string, unknown>)
				: null;
		const videos =
			response && "videos" in response && Array.isArray(response.videos)
				? response.videos
				: null;
		const firstVideo =
			videos && videos[0] && typeof videos[0] === "object"
				? (videos[0] as Record<string, unknown>)
				: null;

		if (
			firstVideo &&
			typeof firstVideo.bytesBase64Encoded === "string" &&
			firstVideo.bytesBase64Encoded.length > 0
		) {
			return {
				bytesBase64Encoded: firstVideo.bytesBase64Encoded,
				mimeType:
					typeof firstVideo.mimeType === "string" &&
					firstVideo.mimeType.length > 0
						? firstVideo.mimeType
						: "video/mp4",
			};
		}
	}

	return null;
}

async function requireVideoJobForProject(
	projectId: string,
	videoId: string,
): Promise<VideoJobRecord> {
	const job = await db
		.select()
		.from(tables.videoJob)
		.where(
			and(
				eq(tables.videoJob.id, videoId),
				eq(tables.videoJob.projectId, projectId),
			),
		)
		.limit(1)
		.then((rows) => rows[0]);

	if (!job) {
		throw new HTTPException(404, {
			message: "Video not found",
		});
	}

	return job;
}

async function parseJsonBody(c: Context): Promise<ParsedVideoRequest> {
	let rawBody: unknown;
	try {
		rawBody = await c.req.json();
	} catch {
		throw new HTTPException(400, {
			message: "Invalid JSON in request body",
		});
	}

	const validationResult = createVideoRequestSchema.safeParse(rawBody);
	if (!validationResult.success) {
		throw new HTTPException(400, {
			message: `Invalid request parameters: ${validationResult.error.issues.map((issue) => `${issue.path.join(".")}: ${issue.message}`).join(", ")}`,
		});
	}

	return {
		rawBody,
		request: validationResult.data,
	};
}

function isDebugMode(c: Context): boolean {
	return (
		c.req.header("x-debug") === "true" ||
		process.env.FORCE_DEBUG_MODE === "true" ||
		process.env.NODE_ENV !== "production"
	);
}

async function fetchUpstreamJson(
	url: string,
	init: RequestInit,
): Promise<Record<string, unknown>> {
	const response = await fetch(url, init);
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
		logger.warn("Upstream video request failed", {
			url,
			status: response.status,
			body,
		});
		throw new HTTPException(
			response.status as
				| 400
				| 401
				| 403
				| 404
				| 409
				| 422
				| 429
				| 500
				| 502
				| 503
				| 504,
			{
				message:
					typeof body.error === "object" &&
					body.error &&
					"message" in body.error &&
					typeof body.error.message === "string"
						? body.error.message
						: `Upstream provider error (${response.status})`,
			},
		);
	}

	return body;
}

function extractUpstreamVideoId(body: Record<string, unknown>): string | null {
	const data =
		body.data && typeof body.data === "object"
			? (body.data as Record<string, unknown>)
			: null;

	for (const value of [
		body.name,
		body.id,
		body.video_id,
		body.job_id,
		body.taskId,
		data?.taskId,
		data?.id,
	]) {
		if (typeof value === "string" && value.length > 0) {
			return value;
		}
	}

	return null;
}

async function createObsidianVideoJob(
	providerContext: ProviderContext,
	providerMapping: ProviderModelMapping,
	videoSize: VideoSizeConfig,
	prompt: string,
): Promise<{
	upstreamId: string;
	upstreamRequest: Record<string, unknown>;
	upstreamResponse: Record<string, unknown>;
}> {
	const upstreamUrl = joinUrl(providerContext.baseUrl, "/v1/videos");
	const upstreamModelName = getVideoUpstreamModelName(
		"obsidian",
		providerMapping.modelName,
		videoSize,
	);
	const upstreamRequest = {
		model: upstreamModelName,
		prompt,
		size: videoSize.size,
	};
	const upstreamResponse = addRequestedVideoMetadata(
		await fetchUpstreamJson(upstreamUrl, {
			method: "POST",
			headers: {
				"Content-Type": "application/json",
				...getProviderHeaders("obsidian", providerContext.token),
			},
			body: JSON.stringify(upstreamRequest),
		}),
		videoSize,
	);
	const upstreamId = extractUpstreamVideoId(upstreamResponse);
	if (!upstreamId) {
		throw new HTTPException(502, {
			message: "Upstream video response did not include an id",
		});
	}

	return { upstreamId, upstreamRequest, upstreamResponse };
}

async function createAvalancheVideoJob(
	providerContext: ProviderContext,
	providerMapping: ProviderModelMapping,
	videoSize: VideoSizeConfig,
	prompt: string,
): Promise<{
	upstreamId: string;
	upstreamRequest: Record<string, unknown>;
	upstreamResponse: Record<string, unknown>;
}> {
	const upstreamUrl = joinUrl(providerContext.baseUrl, "/generate");
	const upstreamModelName = getVideoUpstreamModelName(
		"avalanche",
		providerMapping.modelName,
		videoSize,
	);
	const upstreamRequest = {
		prompt,
		model: upstreamModelName,
		aspect_ratio: getAvalancheAspectRatio(videoSize),
		generationType: "TEXT_2_VIDEO",
		enableFallback: false,
	};
	const rawResponse = await fetchUpstreamJson(upstreamUrl, {
		method: "POST",
		headers: {
			"Content-Type": "application/json",
			...getProviderHeaders("avalanche", providerContext.token),
		},
		body: JSON.stringify(upstreamRequest),
	});
	const upstreamResponse = addRequestedVideoMetadata(
		{
			...rawResponse,
			status: "queued",
			duration: 8,
			aspect_ratio: upstreamRequest.aspect_ratio,
		},
		videoSize,
	);
	const upstreamId = extractUpstreamVideoId(upstreamResponse);
	if (!upstreamId) {
		throw new HTTPException(502, {
			message: "Avalanche video response did not include a task id",
		});
	}

	return { upstreamId, upstreamRequest, upstreamResponse };
}

async function createGoogleVertexVideoJob(
	providerContext: ProviderContext,
	providerMapping: ProviderModelMapping,
	videoSize: VideoSizeConfig,
	prompt: string,
	durationSeconds: number,
	videoJobId: string,
	organizationId: string,
	projectId: string,
): Promise<{
	upstreamId: string;
	upstreamRequest: Record<string, unknown>;
	upstreamResponse: Record<string, unknown>;
}> {
	if (!providerContext.vertexProjectId || !providerContext.vertexRegion) {
		throw new HTTPException(500, {
			message:
				"Google Vertex video generation requires project and region metadata",
		});
	}

	const upstreamModelName = getVideoUpstreamModelName(
		"google-vertex",
		providerMapping.modelName,
		videoSize,
	);
	const outputBucket = getGoogleVertexVideoOutputBucket();
	const outputStorageUri = outputBucket
		? buildVertexVideoOutputStorageUri({
				bucket: outputBucket,
				prefix: getGoogleVertexVideoOutputPrefix(),
				organizationId,
				projectId,
				videoJobId,
			})
		: null;
	const upstreamUrl = joinUrl(
		providerContext.baseUrl,
		`/v1/projects/${providerContext.vertexProjectId}/locations/${providerContext.vertexRegion}/publishers/google/models/${upstreamModelName}:predictLongRunning`,
	);
	const authenticatedUpstreamUrl = appendQueryParam(
		upstreamUrl,
		"key",
		providerContext.token,
	);
	const upstreamRequest = {
		instances: [
			{
				prompt,
			},
		],
		parameters: {
			aspectRatio: getVertexAspectRatio(videoSize),
			durationSeconds,
			generateAudio: true,
			resolution: getVertexResolution(videoSize),
			sampleCount: 1,
			...(outputStorageUri ? { storageUri: outputStorageUri } : {}),
		},
	};
	const rawResponse = await fetchUpstreamJson(authenticatedUpstreamUrl, {
		method: "POST",
		headers: {
			"Content-Type": "application/json",
		},
		body: JSON.stringify(upstreamRequest),
	});
	const upstreamId =
		typeof rawResponse.name === "string" && rawResponse.name.length > 0
			? rawResponse.name
			: extractUpstreamVideoId(rawResponse);
	if (!upstreamId) {
		throw new HTTPException(502, {
			message: "Google Vertex video response did not include an operation name",
		});
	}

	return {
		upstreamId,
		upstreamRequest,
		upstreamResponse: addRequestedVideoMetadata(
			{
				...rawResponse,
				name: upstreamId,
				status: rawResponse.done === true ? "completed" : "queued",
				duration: durationSeconds,
				google_vertex_project_id: providerContext.vertexProjectId,
				google_vertex_region: providerContext.vertexRegion,
				google_vertex_model_name: upstreamModelName,
				google_vertex_generate_audio: true,
				...(outputStorageUri
					? {
							google_vertex_output_storage_uri: outputStorageUri,
						}
					: {}),
			},
			videoSize,
		),
	};
}

async function createUpstreamVideoJob(
	providerContext: ProviderContext,
	providerMapping: ProviderModelMapping,
	videoSize: VideoSizeConfig,
	prompt: string,
	durationSeconds: number,
	videoJobId: string,
	organizationId: string,
	projectId: string,
): Promise<{
	upstreamId: string;
	upstreamRequest: Record<string, unknown>;
	upstreamResponse: Record<string, unknown>;
}> {
	switch (providerContext.providerId) {
		case "obsidian":
			return await createObsidianVideoJob(
				providerContext,
				providerMapping,
				videoSize,
				prompt,
			);
		case "avalanche":
			return await createAvalancheVideoJob(
				providerContext,
				providerMapping,
				videoSize,
				prompt,
			);
		case "google-vertex":
			return await createGoogleVertexVideoJob(
				providerContext,
				providerMapping,
				videoSize,
				prompt,
				durationSeconds,
				videoJobId,
				organizationId,
				projectId,
			);
		default:
			throw new HTTPException(500, {
				message: `Unsupported video provider: ${providerContext.providerId}`,
			});
	}
}

export const videos = new OpenAPIHono<ServerTypes>();

videos.openapi(createVideo, async (c) => {
	const { rawBody, request } = await parseJsonBody(c);
	const { apiKey, project, organization, requestId } =
		await requireRequestContext(c);
	const { normalizedModel, requestedProvider } = getVideoModel(request.model);
	const videoSize = getVideoSizeConfig(request.size);
	const videoDurationSeconds = getVideoDurationSeconds(request.seconds);
	const debugMode = isDebugMode(c);

	if (getAvailableCredits(organization) < MIN_VIDEO_GENERATION_BALANCE) {
		throw new HTTPException(402, {
			message:
				"Video generation requires at least $1.00 in available credits. Please add credits and try again.",
		});
	}

	const modelInfo = models.find((model) => model.id === normalizedModel);
	if (!modelInfo) {
		throw new HTTPException(400, {
			message: `Model ${normalizedModel} not found`,
		});
	}

	const iamValidation = await validateModelAccess(
		apiKey.id,
		normalizedModel,
		requestedProvider,
		modelInfo,
	);

	if (!iamValidation.allowed) {
		throw new HTTPException(403, {
			message: iamValidation.reason ?? "Access to this model is not allowed",
		});
	}

	const { providerMapping, providerContext, upstreamModelName } =
		await resolveVideoExecution(
			modelInfo,
			requestedProvider,
			videoSize,
			project,
			organization.id,
		);
	if (providerContext.providerId !== "google-vertex" && request.seconds !== 8) {
		throw new HTTPException(400, {
			message: `Requested duration ${request.seconds}s is not supported for model ${normalizedModel} on ${providerContext.providerId}.`,
		});
	}

	const videoId = shortid();
	const { upstreamId, upstreamRequest, upstreamResponse } =
		await createUpstreamVideoJob(
			providerContext,
			providerMapping,
			videoSize,
			request.prompt,
			videoDurationSeconds,
			videoId,
			organization.id,
			project.id,
		);
	const storageUri = extractStorageUri(upstreamResponse);
	const parsedStorageUri = parseGcsUri(storageUri);

	const initialStatus = normalizeVideoStatus(upstreamResponse.status);
	const created = await db
		.insert(tables.videoJob)
		.values({
			id: videoId,
			requestId,
			organizationId: organization.id,
			projectId: project.id,
			apiKeyId: apiKey.id,
			mode: project.mode,
			usedMode: providerContext.usedMode,
			model: normalizedModel,
			requestedProvider: requestedProvider ?? null,
			usedProvider: providerContext.providerId,
			usedModel: upstreamModelName,
			providerToken: providerContext.token,
			providerBaseUrl: providerContext.baseUrl,
			upstreamId,
			prompt: request.prompt,
			status: initialStatus,
			progress: extractProgress(upstreamResponse),
			error: extractError(upstreamResponse),
			contentUrl: extractContentUrl(upstreamResponse),
			storageProvider: parsedStorageUri ? "gcs" : null,
			storageBucket: parsedStorageUri?.bucket ?? null,
			storageObjectPath: parsedStorageUri?.objectPath ?? null,
			storageUri,
			storageExpiresAt: null,
			contentType:
				typeof upstreamResponse.mime_type === "string"
					? upstreamResponse.mime_type
					: "video/mp4",
			completedAt: parseTimestamp(upstreamResponse.completed_at),
			expiresAt: parseTimestamp(upstreamResponse.expires_at),
			lastPolledAt: null,
			nextPollAt: new Date(),
			pollAttemptCount: 0,
			callbackUrl: request.callback_url ?? null,
			callbackSecret: request.callback_secret ?? null,
			callbackStatus: request.callback_url ? "pending" : "none",
			upstreamCreateResponse: {
				...upstreamResponse,
				...(debugMode
					? {
							llmgateway_raw_request: rawBody,
							llmgateway_upstream_request: upstreamRequest,
						}
					: {}),
			},
			upstreamStatusResponse: upstreamResponse,
		})
		.returning()
		.then((rows) => rows[0]);

	logger.info("Created video job", {
		videoId: created.id,
		upstreamId,
		projectId: project.id,
		organizationId: organization.id,
		model: normalizedModel,
		usedProvider: providerContext.providerId,
	});

	return c.json(await serializeVideoJob(created));
});

videos.openapi(getVideo, async (c) => {
	const { project } = await requireRequestContext(c);
	const { video_id: videoId } = c.req.valid("param");
	const job = await requireVideoJobForProject(project.id, videoId);
	return c.json(await serializeVideoJob(job));
});

videos.openapi(getVideoContent, async (c) => {
	const { project } = await requireRequestContext(c);
	const { video_id: videoId } = c.req.valid("param");
	const job = await requireVideoJobForProject(project.id, videoId);

	if (job.status !== "completed") {
		throw new HTTPException(409, {
			message: `Video is not ready yet. Current status: ${job.status}`,
		});
	}

	if (!job.contentUrl && !job.storageUri) {
		const inlineVideo = getGoogleVertexInlineVideo(job);
		if (!inlineVideo) {
			throw new HTTPException(404, {
				message: "Video content is not available",
			});
		}

		const bytes = Uint8Array.from(
			Buffer.from(inlineVideo.bytesBase64Encoded, "base64"),
		);
		return new Response(bytes, {
			status: 200,
			headers: {
				"Content-Type": inlineVideo.mimeType,
			},
		});
	}

	const contentUrl = job.contentUrl ?? (await getExternalVideoContentUrl(job));
	if (!contentUrl) {
		const inlineVideo = getGoogleVertexInlineVideo(job);
		if (inlineVideo) {
			const bytes = Uint8Array.from(
				Buffer.from(inlineVideo.bytesBase64Encoded, "base64"),
			);
			return new Response(bytes, {
				status: 200,
				headers: {
					"Content-Type": inlineVideo.mimeType,
				},
			});
		}

		throw new HTTPException(404, {
			message: "Video content is not available",
		});
	}

	const upstreamResponse = await fetch(contentUrl);
	if (!upstreamResponse.ok || !upstreamResponse.body) {
		throw new HTTPException(502, {
			message: "Failed to fetch video content from upstream provider",
		});
	}

	const headers = new Headers();
	headers.set(
		"Content-Type",
		upstreamResponse.headers.get("Content-Type") ??
			job.contentType ??
			"video/mp4",
	);

	const contentLength = upstreamResponse.headers.get("Content-Length");
	if (contentLength) {
		headers.set("Content-Length", contentLength);
	}

	return new Response(upstreamResponse.body, {
		status: 200,
		headers,
	});
});
