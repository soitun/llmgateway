#!/usr/bin/env tsx
/**
 * Export models and providers from @llmgateway/models to JSON
 * for the Rust gateway to consume.
 *
 * Usage (from repo root): npx tsx apps/gateway-rs/scripts/export-models.ts
 */

import * as fs from "fs";
import * as path from "path";

const scriptDir = __dirname;
const outputDir = path.resolve(scriptDir, "..");
const modelsPackage = path.resolve(
	scriptDir,
	"../../../packages/models/dist/index.js",
);

// Custom replacer to handle Date objects and Infinity
function replacer(_key: string, value: unknown): unknown {
	if (value instanceof Date) {
		return value.toISOString();
	}
	if (value === Infinity) {
		return 1e18; // Large number to represent Infinity
	}
	return value;
}

async function main() {
	const mod = await import(modelsPackage);
	const models = mod.models;
	const providers = mod.providers;

	// Export models
	const modelsJson = JSON.stringify(models, replacer, "\t");
	fs.writeFileSync(path.join(outputDir, "models.json"), modelsJson);
	console.log(`Exported ${models.length} models to models.json`);

	// Export providers
	const providersJson = JSON.stringify(providers, replacer, "\t");
	fs.writeFileSync(path.join(outputDir, "providers.json"), providersJson);
	console.log(`Exported ${providers.length} providers to providers.json`);
}

main().catch((err) => {
	console.error("Failed to export models:", err);
	process.exit(1);
});
