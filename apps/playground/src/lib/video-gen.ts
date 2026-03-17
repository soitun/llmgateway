export type VideoSize =
	| "1280x720"
	| "720x1280"
	| "1920x1080"
	| "1080x1920"
	| "3840x2160"
	| "2160x3840";

export type VideoDuration = 4 | 6 | 8;

export interface VideoJob {
	id: string;
	object: "video";
	model: string;
	status:
		| "queued"
		| "in_progress"
		| "completed"
		| "failed"
		| "canceled"
		| "expired";
	progress: number | null;
	created_at: number;
	completed_at: number | null;
	expires_at: number | null;
	error: { code?: string; message: string; details?: unknown } | null;
	content?: { type: "video"; url: string; mime_type?: string | null }[];
}

export interface VideoGalleryModelResult {
	modelId: string;
	modelName: string;
	job: VideoJob | null;
	videoUrl: string | null;
	error?: string;
	isLoading: boolean;
}

export interface VideoGalleryItem {
	id: string;
	prompt: string;
	timestamp: number;
	models: VideoGalleryModelResult[];
}

const VIDEO_SIZE_LABELS: Record<VideoSize, string> = {
	"1280x720": "720p Landscape",
	"720x1280": "720p Portrait",
	"1920x1080": "1080p Landscape",
	"1080x1920": "1080p Portrait",
	"3840x2160": "4K Landscape",
	"2160x3840": "4K Portrait",
};

export function getVideoSizeLabel(size: VideoSize): string {
	return VIDEO_SIZE_LABELS[size];
}

export function getVideoSizes(): VideoSize[] {
	return Object.keys(VIDEO_SIZE_LABELS) as VideoSize[];
}

export function downloadVideo(url: string, filename?: string) {
	const name = filename ?? `video-${Date.now()}.mp4`;
	const a = document.createElement("a");
	a.href = url;
	a.download = name;
	a.target = "_blank";
	a.rel = "noopener noreferrer";
	document.body.appendChild(a);
	a.click();
	document.body.removeChild(a);
}

const TERMINAL_STATUSES = new Set([
	"completed",
	"failed",
	"canceled",
	"expired",
]);

const MAX_POLL_DURATION_MS = 10 * 60 * 1000; // 10 minutes

export async function* pollVideoJob(
	videoId: string,
	signal?: AbortSignal,
): AsyncGenerator<VideoJob> {
	const startTime = Date.now();

	while (true) {
		if (signal?.aborted) {
			return;
		}

		const elapsed = Date.now() - startTime;
		if (elapsed > MAX_POLL_DURATION_MS) {
			yield {
				id: videoId,
				object: "video",
				model: "",
				status: "failed",
				progress: null,
				created_at: Math.floor(startTime / 1000),
				completed_at: null,
				expires_at: null,
				error: {
					message:
						"Video generation timed out. The video may still be processing - try refreshing the page.",
				},
			};
			return;
		}

		const response = await fetch(`/api/video/${videoId}?_t=${Date.now()}`, {
			signal,
			cache: "no-store",
		});
		if (!response.ok) {
			throw new Error(`Poll failed: ${response.status}`);
		}

		const job: VideoJob = await response.json();
		yield job;

		if (TERMINAL_STATUSES.has(job.status)) {
			return;
		}

		// If content URL is already available even though status isn't terminal,
		// treat it as completed
		if (job.content?.[0]?.url) {
			yield { ...job, status: "completed" };
			return;
		}

		const delay =
			elapsed < 30_000
				? 2_000
				: elapsed < 60_000
					? 3_000
					: elapsed < 120_000
						? 5_000
						: 10_000;

		await new Promise<void>((resolve, reject) => {
			const timer = setTimeout(resolve, delay);
			signal?.addEventListener(
				"abort",
				() => {
					clearTimeout(timer);
					reject(new DOMException("Aborted", "AbortError"));
				},
				{ once: true },
			);
		});
	}
}
