import { serve } from "@hono/node-server";
import { Hono } from "hono";

// Create a mock OpenAI API server
export const mockOpenAIServer = new Hono();

// Sample response for chat completions
const sampleChatCompletionResponse = {
	id: "chatcmpl-123",
	object: "chat.completion",
	created: Math.floor(Date.now() / 1000),
	model: "gpt-4o-mini",
	choices: [
		{
			index: 0,
			message: {
				role: "assistant",
				content:
					"Hello! I'm a mock response from the test server. How can I help you today?",
			},
			finish_reason: "stop",
		},
	],
	usage: {
		prompt_tokens: 10,
		completion_tokens: 20,
		total_tokens: 30,
	},
};

// Sample error response
const sampleErrorResponse = {
	error: {
		message:
			"The server had an error processing your request. Sorry about that!",
		type: "server_error",
		param: null,
		code: "internal_server_error",
	},
};

// Helper to extract delay from message content (e.g., "TRIGGER_TIMEOUT_500" -> 500ms)
function extractTimeoutDelay(content: string): number | null {
	const match = content.match(/TRIGGER_TIMEOUT_(\d+)/);
	if (match) {
		return parseInt(match[1], 10);
	}
	if (content.includes("TRIGGER_TIMEOUT")) {
		// Default to 5 seconds if no specific delay is provided
		return 5000;
	}
	return null;
}

// Helper to extract a specific HTTP status code from message content
// e.g., "TRIGGER_STATUS_429" -> { statusCode: 429, errorResponse: {...} }
function extractStatusCodeTrigger(
	content: string,
): { statusCode: number; errorResponse: object } | null {
	const match = content.match(/TRIGGER_STATUS_(\d{3})/);
	if (!match) {
		return null;
	}
	const statusCode = parseInt(match[1], 10);

	const errorResponses: Record<number, object> = {
		429: {
			error: {
				message: "Rate limit exceeded. Please retry after 1 second.",
				type: "rate_limit_error",
				param: null,
				code: "rate_limit_exceeded",
			},
		},
		401: {
			error: {
				message: "Incorrect API key provided.",
				type: "authentication_error",
				param: null,
				code: "invalid_api_key",
			},
		},
		403: {
			error: {
				message: "You do not have access to this resource.",
				type: "permission_error",
				param: null,
				code: "forbidden",
			},
		},
		404: {
			error: {
				message: "The model 'nonexistent-model' does not exist.",
				type: "invalid_request_error",
				param: "model",
				code: "model_not_found",
			},
		},
		400: {
			error: {
				message: "Invalid request: malformed input.",
				type: "invalid_request_error",
				param: null,
				code: "invalid_request",
			},
		},
		503: {
			error: {
				message: "The server is temporarily unavailable.",
				type: "server_error",
				param: null,
				code: "service_unavailable",
			},
		},
	};

	return {
		statusCode,
		errorResponse: errorResponses[statusCode] || {
			error: {
				message: `Mock error with status ${statusCode}`,
				type: "server_error",
				param: null,
				code: `error_${statusCode}`,
			},
		},
	};
}

// Counter for TRIGGER_FAIL_ONCE - tracks how many times a request with this
// trigger has been received. First request fails with 500, subsequent succeed.
// NOTE: This is module-level mutable state shared across all tests using this server.
// Each test that relies on TRIGGER_FAIL_ONCE must call resetFailOnceCounter()
// in its beforeEach to avoid cross-test interference.
let failOnceCounter = 0;
let currentMockServerUrl = "http://localhost:3001";
let videoCounter = 0;

interface MockVideoJobState {
	id: string;
	object: "video";
	model: string;
	status: string;
	progress: number;
	size?: string;
	duration?: number;
	resolution?: string;
	width?: number;
	height?: number;
	created_at: number;
	completed_at: number | null;
	expires_at: number | null;
	error: { code?: string; message: string } | null;
	content?: Array<{
		type: "video";
		url: string;
		mime_type: string;
	}>;
}

interface MockWebhookDelivery {
	name: string;
	headers: Record<string, string>;
	body: unknown;
}

const videoJobs = new Map<string, MockVideoJobState>();
const webhookDeliveries: MockWebhookDelivery[] = [];
const webhookStatuses = new Map<string, number>();

function getMockVideoSizeMetadata(size: unknown): {
	size: string;
	resolution: string;
	width: number;
	height: number;
} {
	switch (size) {
		case "720x1280":
			return {
				size,
				resolution: "720p",
				width: 720,
				height: 1280,
			};
		case "1920x1080":
			return {
				size,
				resolution: "1080p",
				width: 1920,
				height: 1080,
			};
		case "1080x1920":
			return {
				size,
				resolution: "1080p",
				width: 1080,
				height: 1920,
			};
		case "3840x2160":
			return {
				size,
				resolution: "4k",
				width: 3840,
				height: 2160,
			};
		case "2160x3840":
			return {
				size,
				resolution: "4k",
				width: 2160,
				height: 3840,
			};
		case "1280x720":
		default:
			return {
				size: "1280x720",
				resolution: "720p",
				width: 1280,
				height: 720,
			};
	}
}

function getMockVertexVideoSizeMetadata(
	resolution: unknown,
	aspectRatio: unknown,
): {
	size: string;
	resolution: string;
	width: number;
	height: number;
} {
	if (resolution === "4k") {
		return aspectRatio === "9:16"
			? getMockVideoSizeMetadata("2160x3840")
			: getMockVideoSizeMetadata("3840x2160");
	}

	if (resolution === "1080p") {
		return aspectRatio === "9:16"
			? getMockVideoSizeMetadata("1080x1920")
			: getMockVideoSizeMetadata("1920x1080");
	}

	return aspectRatio === "9:16"
		? getMockVideoSizeMetadata("720x1280")
		: getMockVideoSizeMetadata("1280x720");
}

export function resetFailOnceCounter() {
	failOnceCounter = 0;
}

export function resetMockVideoState() {
	videoCounter = 0;
	videoJobs.clear();
	webhookDeliveries.length = 0;
	webhookStatuses.clear();
}

export function setMockVideoStatus(
	videoId: string,
	status: MockVideoJobState["status"],
	overrides: Partial<MockVideoJobState> = {},
) {
	const current = videoJobs.get(videoId);
	if (!current) {
		throw new Error(`Mock video job ${videoId} not found`);
	}

	const next: MockVideoJobState = {
		...current,
		status,
		progress: status === "completed" ? 100 : current.progress,
		completed_at:
			status === "completed"
				? Math.floor(Date.now() / 1000)
				: current.completed_at,
		error:
			status === "failed"
				? {
						message: "Mock video generation failed",
					}
				: null,
		...overrides,
	};

	if (status === "completed" && !next.content) {
		next.content = [
			{
				type: "video",
				url: `${currentMockServerUrl}/mock-assets/${videoId}`,
				mime_type: "video/mp4",
			},
		];
	}

	videoJobs.set(videoId, next);
}

export function getMockVideo(videoId: string): MockVideoJobState | undefined {
	return videoJobs.get(videoId);
}

export function setMockWebhookStatus(name: string, status: number) {
	webhookStatuses.set(name, status);
}

export function getMockWebhookDeliveries(name?: string): MockWebhookDelivery[] {
	return webhookDeliveries.filter((delivery) =>
		name ? delivery.name === name : true,
	);
}

// Helper to delay response
function delay(ms: number): Promise<void> {
	return new Promise((resolve) => {
		setTimeout(resolve, ms);
	});
}

// Handle OpenAI Responses API endpoint (for gpt-5 and other models with supportsResponsesApi)
mockOpenAIServer.post("/v1/responses", async (c) => {
	const body = await c.req.json();

	// Check if this request should trigger an error response
	const shouldError = body.input?.some?.(
		(msg: any) =>
			msg.role === "user" && msg.content?.includes?.("TRIGGER_ERROR"),
	);

	if (shouldError) {
		c.status(500);
		return c.json(sampleErrorResponse);
	}

	// Get the user's message to include in the response
	const userMessage =
		body.input?.find?.((msg: any) => msg.role === "user")?.content ?? "";

	// Create a Responses API format response
	const response = {
		id: "resp-123",
		object: "response",
		created_at: Math.floor(Date.now() / 1000),
		model: body.model ?? "gpt-5-nano",
		output: [
			{
				type: "message",
				role: "assistant",
				content: [
					{
						type: "output_text",
						text: `Hello! I received your message: "${userMessage}". This is a mock response from the test server.`,
					},
				],
			},
		],
		usage: {
			input_tokens: 10,
			output_tokens: 20,
			total_tokens: 30,
		},
		status: "completed",
	};

	return c.json(response);
});

// Handle chat completions endpoint
mockOpenAIServer.post("/v1/chat/completions", async (c) => {
	const body = await c.req.json();

	// Check if this request should trigger an error response
	const shouldError = body.messages.some(
		(msg: any) => msg.role === "user" && msg.content.includes("TRIGGER_ERROR"),
	);

	if (shouldError) {
		c.status(500);
		return c.json(sampleErrorResponse);
	}

	// Get the user's message to include in the response
	const userMessage =
		body.messages.find((msg: any) => msg.role === "user")?.content ?? "";

	// Check if this request should trigger a specific HTTP status code error
	const statusTrigger = extractStatusCodeTrigger(userMessage);
	if (statusTrigger) {
		// Hono's c.status() expects a narrow StatusCode union type; cast needed for dynamic status codes
		c.status(statusTrigger.statusCode as any);
		return c.json(statusTrigger.errorResponse);
	}

	// Check if this request should fail on the first attempt but succeed on retry
	if (userMessage.includes("TRIGGER_FAIL_ONCE")) {
		failOnceCounter++;
		if (failOnceCounter === 1) {
			c.status(500);
			return c.json({
				error: {
					message: "Temporary server error (will succeed on retry)",
					type: "server_error",
					param: null,
					code: "internal_server_error",
				},
			});
		}
		// Subsequent requests succeed - fall through to normal response
	}

	// Check if this request should trigger a timeout (delay response)
	const timeoutDelay = extractTimeoutDelay(userMessage);
	if (timeoutDelay) {
		await delay(timeoutDelay);
	}

	// Check if this request should trigger zero tokens response
	const shouldReturnZeroTokens = body.messages.some(
		(msg: any) => msg.role === "user" && msg.content.includes("ZERO_TOKENS"),
	);

	// Create a custom response that includes the user's message
	const response = {
		...sampleChatCompletionResponse,
		choices: [
			{
				...sampleChatCompletionResponse.choices[0],
				message: {
					role: "assistant",
					content: `Hello! I received your message: "${userMessage}". This is a mock response from the test server.`,
				},
			},
		],
		usage: shouldReturnZeroTokens
			? {
					prompt_tokens: 0,
					completion_tokens: 20,
					total_tokens: 20,
				}
			: sampleChatCompletionResponse.usage,
	};

	return c.json(response);
});

mockOpenAIServer.post("/v1/videos", async (c) => {
	const body = await c.req.json();
	videoCounter++;
	const id = `video_${videoCounter}`;
	const videoSize = getMockVideoSizeMetadata(body.size);
	const job: MockVideoJobState = {
		id,
		object: "video",
		model: body.model ?? "veo-3.1",
		status: "queued",
		progress: 0,
		size: videoSize.size,
		duration: 8,
		resolution: videoSize.resolution,
		width: videoSize.width,
		height: videoSize.height,
		created_at: Math.floor(Date.now() / 1000),
		completed_at: null,
		expires_at: null,
		error: null,
	};

	videoJobs.set(id, job);

	return c.json(job);
});

mockOpenAIServer.post("/api/v1/veo/generate", async (c) => {
	const body = await c.req.json();
	videoCounter++;
	const id = `avalanche_task_${videoCounter}`;
	const videoSize =
		body.aspect_ratio === "9:16"
			? {
					size: "1080x1920",
					resolution: "720p",
					width: 1080,
					height: 1920,
				}
			: {
					size: "1920x1080",
					resolution: "720p",
					width: 1920,
					height: 1080,
				};

	const job: MockVideoJobState = {
		id,
		object: "video",
		model: body.model ?? "veo3",
		status: "queued",
		progress: 0,
		size: videoSize.size,
		duration: 8,
		resolution: videoSize.resolution,
		width: videoSize.width,
		height: videoSize.height,
		created_at: Math.floor(Date.now() / 1000),
		completed_at: null,
		expires_at: null,
		error: null,
	};

	videoJobs.set(id, job);

	return c.json({
		code: 200,
		msg: "success",
		data: {
			taskId: id,
		},
	});
});

mockOpenAIServer.post(
	"/v1/projects/:project/locations/:location/publishers/google/models/*",
	async (c, next) => {
		const body = await c.req.json();
		const modelPath = c.req.path.split("/models/")[1] ?? "";
		const [modelName, action] = modelPath.split(":");

		if (action !== "predictLongRunning" && action !== "fetchPredictOperation") {
			return await next();
		}

		if (action === "predictLongRunning") {
			videoCounter++;
			const operationName = `projects/${c.req.param("project")}/locations/${c.req.param("location")}/publishers/google/models/${modelName}/operations/video_${videoCounter}`;
			const parameters =
				body.parameters && typeof body.parameters === "object"
					? body.parameters
					: {};
			const videoSize = getMockVertexVideoSizeMetadata(
				(parameters as Record<string, unknown>).resolution,
				(parameters as Record<string, unknown>).aspectRatio,
			);
			const job: MockVideoJobState = {
				id: operationName,
				object: "video",
				model: modelName || "veo-3.1-generate-preview",
				status: "queued",
				progress: 0,
				size: videoSize.size,
				duration: 8,
				resolution: videoSize.resolution,
				width: videoSize.width,
				height: videoSize.height,
				created_at: Math.floor(Date.now() / 1000),
				completed_at: null,
				expires_at: null,
				error: null,
			};

			videoJobs.set(operationName, job);

			return c.json({
				name: operationName,
				done: false,
			});
		}

		if (action === "fetchPredictOperation") {
			const operationName =
				body && typeof body === "object" ? body.operationName : undefined;

			if (typeof operationName !== "string" || operationName.length === 0) {
				c.status(400);
				return c.json({
					error: {
						message: "operationName is required",
					},
				});
			}

			const job = videoJobs.get(operationName);
			if (!job) {
				c.status(404);
				return c.json({
					error: {
						message: "Operation not found",
					},
				});
			}

			if (job.status === "failed") {
				return c.json({
					name: operationName,
					done: true,
					error: {
						code: 13,
						message: "Mock Vertex generation failed",
					},
				});
			}

			if (job.status !== "completed") {
				return c.json({
					name: operationName,
					done: false,
				});
			}

			return c.json({
				name: operationName,
				done: true,
				response: {
					videos: [
						{
							bytesBase64Encoded: Buffer.from(
								`mock-video-${operationName}`,
							).toString("base64"),
							mimeType: "video/mp4",
						},
					],
				},
			});
		}

		c.status(404);
		return c.json({
			error: {
				message: "Unsupported Google Vertex mock action",
			},
		});
	},
);

mockOpenAIServer.get("/v1/videos/:id", async (c) => {
	const id = c.req.param("id");
	const job = videoJobs.get(id);

	if (!job) {
		c.status(404);
		return c.json({
			error: {
				message: "Video job not found",
				code: "not_found",
			},
		});
	}

	return c.json(job);
});

mockOpenAIServer.get("/v1/videos/:id/content", async (c) => {
	const id = c.req.param("id");
	const job = videoJobs.get(id);

	if (!job) {
		c.status(404);
		return c.json({
			error: {
				message: "Video job not found",
				code: "not_found",
			},
		});
	}

	return c.json({
		url: `${currentMockServerUrl}/mock-assets/${id}`,
		mime_type: "video/mp4",
		size: job.size,
		duration: job.duration,
		resolution: job.resolution,
		width: job.width,
		height: job.height,
	});
});

mockOpenAIServer.get("/api/v1/veo/record-info", async (c) => {
	const taskId = c.req.query("taskId");
	if (!taskId) {
		c.status(400);
		return c.json({
			code: 400,
			msg: "taskId is required",
		});
	}

	const job = videoJobs.get(taskId);
	if (!job) {
		c.status(404);
		return c.json({
			code: 404,
			msg: "task not found",
		});
	}

	const successFlag =
		job.status === "completed" ? 1 : job.status === "failed" ? -1 : 0;

	return c.json({
		code: 200,
		msg: "success",
		data: {
			taskId,
			successFlag,
			createTime: job.created_at,
			completeTime: job.completed_at,
			response: {
				resultUrls:
					job.status === "completed"
						? [`${currentMockServerUrl}/mock-assets/${taskId}`]
						: [],
				resolution: job.resolution ?? "720p",
			},
		},
	});
});

mockOpenAIServer.get("/api/v1/veo/get-1080p-video", async (c) => {
	const taskId = c.req.query("taskId");
	if (!taskId) {
		c.status(400);
		return c.json({
			code: 400,
			msg: "taskId is required",
		});
	}

	const job = videoJobs.get(taskId);
	if (!job) {
		c.status(404);
		return c.json({
			code: 404,
			msg: "task not found",
		});
	}

	if (job.status !== "completed") {
		c.status(422);
		return c.json({
			code: 422,
			msg: "video is still processing",
		});
	}

	return c.json({
		code: 200,
		msg: "success",
		data: {
			taskId,
			resultUrl: `${currentMockServerUrl}/mock-assets/${taskId}-1080p`,
		},
	});
});

mockOpenAIServer.post("/api/v1/veo/get-4k-video", async (c) => {
	const body = await c.req.json();
	const taskId = body.taskId;

	if (typeof taskId !== "string" || taskId.length === 0) {
		c.status(400);
		return c.json({
			code: 400,
			msg: "taskId is required",
		});
	}

	const job = videoJobs.get(taskId);
	if (!job) {
		c.status(404);
		return c.json({
			code: 404,
			msg: "task not found",
		});
	}

	if (job.status !== "completed") {
		c.status(422);
		return c.json({
			code: 422,
			msg: "video is still processing",
		});
	}

	return c.json({
		code: 200,
		msg: "success",
		data: {
			taskId: `${taskId}_4k`,
			resultUrls: [`${currentMockServerUrl}/mock-assets/${taskId}-4k`],
		},
	});
});

mockOpenAIServer.get("/mock-assets/:id", async (c) => {
	const id = c.req.param("id");
	return c.body(`mock-video-${id}`, 200, {
		"Content-Type": "video/mp4",
	});
});

mockOpenAIServer.post("/mock-callback/:name", async (c) => {
	const name = c.req.param("name");
	const headers = Object.fromEntries(c.req.raw.headers.entries());
	const body = await c.req.json();

	webhookDeliveries.push({
		name,
		headers,
		body,
	});

	const status = webhookStatuses.get(name) ?? 200;
	c.status(status as any);
	return c.json({
		ok: status >= 200 && status < 300,
	});
});

// Handle Google Vertex AI generateContent endpoint (Gemini models via Vertex)
mockOpenAIServer.post(
	"/v1/projects/:project/locations/:location/publishers/google/models/:model\\:generateContent",
	async (c) => {
		const body = await c.req.json();

		const shouldError = body.contents?.some?.((content: any) =>
			content.parts?.some?.((part: any) =>
				part.text?.includes?.("TRIGGER_ERROR"),
			),
		);

		if (shouldError) {
			c.status(500);
			return c.json({
				error: {
					code: 500,
					message: "Internal server error",
					status: "INTERNAL",
				},
			});
		}

		const userMessage =
			body.contents?.find?.((ct: any) => ct.role === "user")?.parts?.[0]
				?.text ?? "";

		return c.json({
			candidates: [
				{
					content: {
						parts: [
							{
								text: `Hello! I received your message: "${userMessage}". This is a mock Google Vertex response.`,
							},
						],
						role: "model",
					},
					finishReason: "STOP",
					index: 0,
				},
			],
			usageMetadata: {
				promptTokenCount: 10,
				candidatesTokenCount: 20,
				totalTokenCount: 30,
			},
		});
	},
);

// Handle Google AI Studio generateContent endpoint (Gemini models)
mockOpenAIServer.post("/v1beta/models/:model\\:generateContent", async (c) => {
	const body = await c.req.json();

	// Check if this request should trigger an error response
	const shouldError = body.contents?.some?.((content: any) =>
		content.parts?.some?.((part: any) =>
			part.text?.includes?.("TRIGGER_ERROR"),
		),
	);

	if (shouldError) {
		c.status(500);
		return c.json({
			error: {
				code: 500,
				message: "Internal server error",
				status: "INTERNAL",
			},
		});
	}

	// Get the user's message
	const userMessage =
		body.contents?.find?.((c: any) => c.role === "user")?.parts?.[0]?.text ??
		"";

	// Return Google AI Studio format response
	return c.json({
		candidates: [
			{
				content: {
					parts: [
						{
							text: `Hello! I received your message: "${userMessage}". This is a mock Google AI response.`,
						},
					],
					role: "model",
				},
				finishReason: "STOP",
				index: 0,
			},
		],
		usageMetadata: {
			promptTokenCount: 10,
			candidatesTokenCount: 20,
			totalTokenCount: 30,
		},
	});
});

mockOpenAIServer.post("/model/:model/converse", async (c) => {
	const body = await c.req.json();
	const userMessage = body.messages?.[0]?.content?.[0]?.text ?? "";

	if (userMessage.includes("TRIGGER_BEDROCK_HEADER_ERROR")) {
		c.header(
			"x-amzn-errormessage",
			"The provided model identifier is invalid for this account.",
		);
		c.header("x-amzn-errortype", "ValidationException");
		c.status(400);
		return c.json({});
	}

	return c.json({
		output: {
			message: {
				role: "assistant",
				content: [{ text: `Bedrock mock response: ${userMessage}` }],
			},
		},
		stopReason: "end_turn",
		usage: {
			inputTokens: 10,
			outputTokens: 20,
			totalTokens: 30,
		},
	});
});

mockOpenAIServer.post("/model/:model/converse-stream", async (c) => {
	const body = await c.req.json();
	const userMessage = body.messages?.[0]?.content?.[0]?.text ?? "";

	if (userMessage.includes("TRIGGER_BEDROCK_HEADER_ERROR")) {
		c.header(
			"x-amzn-errormessage",
			"The provided model identifier is invalid for this account.",
		);
		c.header("x-amzn-errortype", "ValidationException");
		c.status(400);
		return c.json({});
	}

	c.header("content-type", "application/vnd.amazon.eventstream");
	return c.body("");
});

let server: any = null;

export function startMockServer(port = 3001): string {
	if (server) {
		return `http://localhost:${port}`;
	}

	currentMockServerUrl = `http://localhost:${port}`;

	server = serve({
		fetch: mockOpenAIServer.fetch,
		port,
	});

	console.log(`Mock OpenAI server started on port ${port}`);
	return `http://localhost:${port}`;
}

export function stopMockServer() {
	if (server) {
		server.close();
		server = null;
		console.log("Mock OpenAI server stopped");
	}
}
