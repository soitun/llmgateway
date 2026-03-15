#!/usr/bin/env tsx
/**
 * Benchmark: Rust Gateway vs TypeScript Gateway
 *
 * Runs identical requests against both gateways and compares:
 * - Non-streaming latency (p50, p95, p99, avg)
 * - Streaming time-to-first-token
 * - Throughput (requests/sec)
 *
 * Usage (from repo root):
 *   npx tsx apps/gateway-rs/scripts/benchmark.ts
 *
 * Expects:
 *   - TS gateway on port 4001
 *   - Rust gateway on port 4002
 */

const TS_URL = "http://localhost:4001";
const RS_URL = "http://localhost:4002";
const AUTH = "test-token";

const MODELS = [
	{ id: "anthropic/claude-3-haiku", label: "Anthropic Claude 3 Haiku" },
	{ id: "google-ai-studio/gemini-2.5-flash-lite", label: "Google Gemini 2.5 Flash Lite" },
];

const WARMUP_ROUNDS = 2;
const BENCHMARK_ROUNDS = 10;

interface TimingResult {
	latencyMs: number;
	ttftMs?: number; // time to first token (streaming)
	status: number;
	error?: string;
}

async function makeNonStreamingRequest(
	baseUrl: string,
	model: string,
): Promise<TimingResult> {
	const start = performance.now();
	try {
		const res = await fetch(`${baseUrl}/v1/chat/completions`, {
			method: "POST",
			headers: {
				"Content-Type": "application/json",
				Authorization: `Bearer ${AUTH}`,
			},
			body: JSON.stringify({
				model,
				messages: [{ role: "user", content: "Say hello in one word." }],
				max_tokens: 10,
				temperature: 0,
			}),
		});
		const latencyMs = performance.now() - start;
		if (!res.ok) {
			const text = await res.text();
			return { latencyMs, status: res.status, error: text.slice(0, 200) };
		}
		await res.json(); // consume body
		return { latencyMs, status: res.status };
	} catch (e: any) {
		return {
			latencyMs: performance.now() - start,
			status: 0,
			error: e.message,
		};
	}
}

async function makeStreamingRequest(
	baseUrl: string,
	model: string,
): Promise<TimingResult> {
	const start = performance.now();
	try {
		const res = await fetch(`${baseUrl}/v1/chat/completions`, {
			method: "POST",
			headers: {
				"Content-Type": "application/json",
				Authorization: `Bearer ${AUTH}`,
			},
			body: JSON.stringify({
				model,
				messages: [{ role: "user", content: "Say hello in one word." }],
				max_tokens: 10,
				temperature: 0,
				stream: true,
			}),
		});

		if (!res.ok || !res.body) {
			const text = await res.text();
			return {
				latencyMs: performance.now() - start,
				status: res.status,
				error: text.slice(0, 200),
			};
		}

		const reader = res.body.getReader();
		const decoder = new TextDecoder();
		let ttftMs: number | undefined;
		let firstDataSeen = false;

		while (true) {
			const { done, value } = await reader.read();
			if (done) break;
			const text = decoder.decode(value, { stream: true });
			if (!firstDataSeen && text.includes("data:")) {
				// Check it's actual content, not just a comment
				const lines = text.split("\n");
				for (const line of lines) {
					if (line.startsWith("data:") && !line.includes("[DONE]")) {
						ttftMs = performance.now() - start;
						firstDataSeen = true;
						break;
					}
				}
			}
		}

		return {
			latencyMs: performance.now() - start,
			ttftMs,
			status: res.status,
		};
	} catch (e: any) {
		return {
			latencyMs: performance.now() - start,
			status: 0,
			error: e.message,
		};
	}
}

function percentile(sorted: number[], p: number): number {
	const idx = Math.ceil((p / 100) * sorted.length) - 1;
	return sorted[Math.max(0, idx)];
}

function stats(values: number[]) {
	const sorted = [...values].sort((a, b) => a - b);
	const avg = values.reduce((a, b) => a + b, 0) / values.length;
	return {
		avg: avg.toFixed(1),
		p50: percentile(sorted, 50).toFixed(1),
		p95: percentile(sorted, 95).toFixed(1),
		p99: percentile(sorted, 99).toFixed(1),
		min: sorted[0].toFixed(1),
		max: sorted[sorted.length - 1].toFixed(1),
	};
}

async function checkHealth(url: string, name: string): Promise<boolean> {
	try {
		const res = await fetch(`${url}/`, { signal: AbortSignal.timeout(3000) });
		if (res.ok) {
			console.log(`  ✓ ${name} is up at ${url}`);
			return true;
		}
		console.log(`  ✗ ${name} returned ${res.status}`);
		return false;
	} catch {
		console.log(`  ✗ ${name} is not reachable at ${url}`);
		return false;
	}
}

async function runBenchmark(
	label: string,
	baseUrl: string,
	model: string,
	rounds: number,
	streaming: boolean,
) {
	const results: TimingResult[] = [];

	for (let i = 0; i < rounds; i++) {
		const result = streaming
			? await makeStreamingRequest(baseUrl, model)
			: await makeNonStreamingRequest(baseUrl, model);
		results.push(result);
	}

	const successes = results.filter((r) => r.status === 200);
	const failures = results.filter((r) => r.status !== 200);

	return { label, results, successes, failures };
}

async function main() {
	console.log("╔══════════════════════════════════════════════════════╗");
	console.log("║   Gateway Benchmark: Rust vs TypeScript             ║");
	console.log("╚══════════════════════════════════════════════════════╝");
	console.log();

	// Health checks
	console.log("Health checks:");
	const tsUp = await checkHealth(TS_URL, "TypeScript Gateway");
	const rsUp = await checkHealth(RS_URL, "Rust Gateway");
	console.log();

	if (!tsUp && !rsUp) {
		console.error("Neither gateway is running. Start them first:");
		console.error("  TS:   pnpm dev  (runs on :4001)");
		console.error("  Rust: PORT=4002 cargo run --release  (in apps/gateway-rs)");
		process.exit(1);
	}

	const gateways: { name: string; url: string; up: boolean }[] = [
		{ name: "TypeScript", url: TS_URL, up: tsUp },
		{ name: "Rust", url: RS_URL, up: rsUp },
	];

	for (const model of MODELS) {
		console.log(`━━━ Model: ${model.label} (${model.id}) ━━━`);
		console.log();

		for (const streaming of [false, true]) {
			const mode = streaming ? "Streaming" : "Non-Streaming";
			console.log(`  ${mode}:`);

			const benchResults: Record<
				string,
				{ latencies: ReturnType<typeof stats>; ttfts?: ReturnType<typeof stats>; errors: number }
			> = {};

			for (const gw of gateways) {
				if (!gw.up) {
					console.log(`    ${gw.name}: SKIPPED (not running)`);
					continue;
				}

				// Warmup
				for (let i = 0; i < WARMUP_ROUNDS; i++) {
					if (streaming) {
						await makeStreamingRequest(gw.url, model.id);
					} else {
						await makeNonStreamingRequest(gw.url, model.id);
					}
				}

				// Benchmark
				const run = await runBenchmark(
					gw.name,
					gw.url,
					model.id,
					BENCHMARK_ROUNDS,
					streaming,
				);

				if (run.successes.length === 0) {
					const firstErr = run.failures[0];
					console.log(
						`    ${gw.name}: ALL FAILED (${run.failures.length}x) - ${firstErr?.status} ${firstErr?.error?.slice(0, 100)}`,
					);
					benchResults[gw.name] = {
						latencies: { avg: "-", p50: "-", p95: "-", p99: "-", min: "-", max: "-" },
						errors: run.failures.length,
					};
					continue;
				}

				const latencies = stats(run.successes.map((r) => r.latencyMs));
				const ttftValues = run.successes
				.filter((r) => r.ttftMs !== undefined)
				.map((r) => r.ttftMs!);
			const ttfts = streaming && ttftValues.length > 0
				? stats(ttftValues)
				: undefined;

				benchResults[gw.name] = { latencies, ttfts, errors: run.failures.length };

				if (streaming && ttfts) {
					console.log(
						`    ${gw.name.padEnd(12)} TTFT avg=${ttfts.avg}ms  p50=${ttfts.p50}ms  p95=${ttfts.p95}ms  total avg=${latencies.avg}ms  errors=${run.failures.length}`,
					);
				} else {
					console.log(
						`    ${gw.name.padEnd(12)} avg=${latencies.avg}ms  p50=${latencies.p50}ms  p95=${latencies.p95}ms  min=${latencies.min}ms  max=${latencies.max}ms  errors=${run.failures.length}`,
					);
				}
			}

			// Compare if both ran successfully
			const tsResult = benchResults["TypeScript"];
			const rsResult = benchResults["Rust"];
			if (tsResult && rsResult && tsResult.latencies.avg !== "-" && rsResult.latencies.avg !== "-") {
				const tsAvg = parseFloat(tsResult.latencies.avg);
				const rsAvg = parseFloat(rsResult.latencies.avg);
				const diff = ((tsAvg - rsAvg) / tsAvg) * 100;
				if (diff > 0) {
					console.log(`    → Rust is ${diff.toFixed(1)}% faster`);
				} else {
					console.log(`    → TypeScript is ${(-diff).toFixed(1)}% faster`);
				}
			}

			console.log();
		}
	}

	console.log("Done.");
}

main().catch(console.error);
