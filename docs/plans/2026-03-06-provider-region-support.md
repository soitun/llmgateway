# Provider Region Support Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add per-model region support so the gateway can route Alibaba Cloud requests to the correct regional endpoint (Singapore, US Virginia, or China Beijing) based on env vars or provider key options.

**Architecture:** Add `regions?: string[]` to `ProviderModelMapping` to declare region availability per model. Region selection happens at runtime via `LLM_ALIBABA_REGION` env var (credits mode) or `alibaba_region` in `ProviderKeyOptions` (api-keys mode). The gateway validates the configured region against the model's `regions` array and constructs the correct DashScope regional URL.

**Tech Stack:** TypeScript, Drizzle ORM, Hono, Next.js (React), Radix UI

---

### Task 1: Add `regions` field to `ProviderModelMapping`

**Files:**
- Modify: `packages/models/src/models.ts:176-182`

**Step 1: Add the field**

Add before the closing `}` of `ProviderModelMapping` (after `imageGenerations`):

```typescript
	/**
	 * Available regions for this model/provider combination.
	 * When set, the gateway validates the configured region against this list.
	 * When unset, the provider uses its default endpoint.
	 */
	regions?: string[];
```

**Step 2: Verify build**

Run: `pnpm build --filter @llmgateway/models`
Expected: SUCCESS (new optional field, no breaking changes)

**Step 3: Commit**

```bash
git add packages/models/src/models.ts
git commit -m "feat: add regions field to ProviderModelMapping"
```

---

### Task 2: Add optional region env var to Alibaba provider definition

**Files:**
- Modify: `packages/models/src/providers.ts:198-213`

**Step 1: Add optional region to Alibaba env config**

Change the Alibaba provider definition from:

```typescript
	env: {
		required: {
			apiKey: "LLM_ALIBABA_API_KEY",
		},
	},
```

To:

```typescript
	env: {
		required: {
			apiKey: "LLM_ALIBABA_API_KEY",
		},
		optional: {
			region: "LLM_ALIBABA_REGION",
		},
	},
```

**Step 2: Verify build**

Run: `pnpm build --filter @llmgateway/models`
Expected: SUCCESS

**Step 3: Commit**

```bash
git add packages/models/src/providers.ts
git commit -m "feat: add LLM_ALIBABA_REGION env var"
```

---

### Task 3: Add `alibaba_region` to `ProviderKeyOptions`

**Files:**
- Modify: `packages/db/src/schema.ts:388-394`

**Step 1: Add the field**

Change `ProviderKeyOptions` from:

```typescript
export interface ProviderKeyOptions {
	aws_bedrock_region_prefix?: "us." | "global." | "eu.";
	azure_resource?: string;
	azure_api_version?: string;
	azure_deployment_type?: "openai" | "ai-foundry";
	azure_validation_model?: string;
}
```

To:

```typescript
export interface ProviderKeyOptions {
	aws_bedrock_region_prefix?: "us." | "global." | "eu.";
	azure_resource?: string;
	azure_api_version?: string;
	azure_deployment_type?: "openai" | "ai-foundry";
	azure_validation_model?: string;
	alibaba_region?: string;
}
```

**Step 2: Verify build**

Run: `pnpm build --filter @llmgateway/db`
Expected: SUCCESS (JSONB field, no migration needed)

**Step 3: Commit**

```bash
git add packages/db/src/schema.ts
git commit -m "feat: add alibaba_region to ProviderKeyOptions"
```

---

### Task 4: Add `regions` to Alibaba model definitions

**Files:**
- Modify: `packages/models/src/models/alibaba.ts`

**Step 1: Add `regions` to every `providerId: "alibaba"` mapping**

For every provider mapping in `alibaba.ts` that has `providerId: "alibaba"`, add:

```typescript
regions: ["singapore", "us-virginia", "cn-beijing"],
```

This applies to the following models (all with `providerId: "alibaba"`):
- qwen-max
- qwen-max-latest
- qwen-plus
- qwen-plus-latest
- qwen-flash
- qwen-omni-turbo
- qwen-turbo
- qwen3-coder-plus
- qwen-vl-max
- qwen-vl-plus
- qwen3-next-80b-a3b-thinking
- qwen3-next-80b-a3b-instruct
- qwen3-max (uses modelName "qwen3-max-preview")
- qwen3-coder-flash
- qwen3-vl-plus
- qwen3-vl-flash
- qwen3-vl-235b-a22b-instruct
- qwen3-vl-235b-a22b-thinking
- qwen2-5-vl-32b-instruct
- qwen3-max-2026-01-23
- qwq-plus
- qwen-coder-plus
- qwen35-397b-a17b
- qwen-image-plus
- qwen-image-max
- qwen-image
- qwen-image-max-2025-12-30
- qwen-image-edit-plus
- qwen-image-edit-max

Do NOT add `regions` to mappings with other `providerId` values (nebius, novita, cerebras, canopywave).

Place the `regions` field right after `providerId` and `modelName` for readability. Example:

```typescript
{
	providerId: "alibaba",
	modelName: "qwen-plus",
	regions: ["singapore", "us-virginia", "cn-beijing"],
	discount: 0.2,
	// ...rest
}
```

**Step 2: Verify build**

Run: `pnpm build --filter @llmgateway/models`
Expected: SUCCESS

**Step 3: Commit**

```bash
git add packages/models/src/models/alibaba.ts
git commit -m "feat: add regions to Alibaba model definitions"
```

---

### Task 5: Region-aware endpoint URL resolution for Alibaba

**Files:**
- Modify: `packages/actions/src/get-provider-endpoint.ts:102-109`

**Step 1: Replace the hardcoded Alibaba case**

Change from:

```typescript
		case "alibaba":
			// Use different base URL for image generation vs chat completions
			if (imageGenerations) {
				url = "https://dashscope-intl.aliyuncs.com";
			} else {
				url = "https://dashscope-intl.aliyuncs.com/compatible-mode";
			}
			break;
```

To:

```typescript
		case "alibaba": {
			const alibabaRegion =
				providerKeyOptions?.alibaba_region ??
				getProviderEnvValue("alibaba", "region", configIndex, "singapore") ??
				"singapore";

			const alibabaBaseUrls: Record<string, string> = {
				singapore: "https://dashscope-intl.aliyuncs.com",
				"us-virginia": "https://dashscope-us.aliyuncs.com",
				"cn-beijing": "https://dashscope.aliyuncs.com",
			};

			const alibabaBase =
				alibabaBaseUrls[alibabaRegion] ?? alibabaBaseUrls.singapore;

			if (imageGenerations) {
				url = alibabaBase;
			} else {
				url = `${alibabaBase}/compatible-mode`;
			}
			break;
		}
```

**Step 2: Verify build**

Run: `pnpm build --filter @llmgateway/actions`
Expected: SUCCESS

**Step 3: Commit**

```bash
git add packages/actions/src/get-provider-endpoint.ts
git commit -m "feat: region-aware Alibaba endpoint resolution"
```

---

### Task 6: Region validation in resolve-provider-context

**Files:**
- Modify: `apps/gateway/src/chat/tools/resolve-provider-context.ts:177-189`

**Step 1: Add region validation after provider mapping lookup**

After the existing block at line ~180 (`const providerMappingForSelected = ...`) and before the reasoning check, add:

```typescript
	// --- Region validation ---
	const providerMappingRegions = (providerMappingForSelected as ProviderModelMapping)?.regions;
	if (providerMappingRegions && providerMappingRegions.length > 0) {
		const configuredRegion =
			providerKey?.options?.alibaba_region ??
			(usedProvider === "alibaba"
				? getProviderEnvValue("alibaba", "region", configIndex, "singapore") ?? "singapore"
				: undefined);

		if (configuredRegion && !providerMappingRegions.includes(configuredRegion)) {
			throw new HTTPException(400, {
				message: `Model ${usedModel} is not available in region "${configuredRegion}". Available regions: ${providerMappingRegions.join(", ")}`,
			});
		}
	}
```

You'll need to add `getProviderEnvValue` to the imports from `@llmgateway/models` if not already imported.

**Step 2: Check imports**

Verify `getProviderEnvValue` is imported. Check the existing imports at the top of the file. If missing, add it to the import from `@llmgateway/models`.

**Step 3: Verify build**

Run: `pnpm build --filter gateway`
Expected: SUCCESS

**Step 4: Commit**

```bash
git add apps/gateway/src/chat/tools/resolve-provider-context.ts
git commit -m "feat: validate region against model regions"
```

---

### Task 7: UI - Alibaba region dropdown in provider key dialog

**Files:**
- Modify: `apps/ui/src/components/provider-keys/create-provider-key-dialog.tsx`

**Step 1: Add state variable**

After the existing `azureValidationModel` state (line ~74), add:

```typescript
	const [alibabaRegion, setAlibabaRegion] = useState("singapore");
```

**Step 2: Add `alibaba_region` to payload type**

In the payload type definition (line ~158), add `alibaba_region` to the `options` type:

```typescript
		options?: {
			aws_bedrock_region_prefix?: "us." | "global." | "eu.";
			azure_resource?: string;
			azure_api_version?: string;
			azure_deployment_type?: "openai" | "ai-foundry";
			azure_validation_model?: string;
			alibaba_region?: string;
		};
```

**Step 3: Add payload construction**

After the Azure payload block (after line ~197), add:

```typescript
		if (selectedProvider === "alibaba") {
			payload.options = {
				alibaba_region: alibabaRegion,
			};
		}
```

**Step 4: Add the dropdown JSX**

After the AWS Bedrock section (after the closing `)}` at line ~347) and before the Azure section, add:

```tsx
				{selectedProvider === "alibaba" && (
					<div className="space-y-2">
						<Label htmlFor="alibaba-region">Region</Label>
						<Select
							value={alibabaRegion}
							onValueChange={setAlibabaRegion}
						>
							<SelectTrigger id="alibaba-region">
								<SelectValue placeholder="Select region" />
							</SelectTrigger>
							<SelectContent>
								<SelectItem value="singapore">
									Singapore (default)
								</SelectItem>
								<SelectItem value="us-virginia">
									US (Virginia)
								</SelectItem>
								<SelectItem value="cn-beijing">
									China (Beijing)
								</SelectItem>
							</SelectContent>
						</Select>
						<p className="text-sm text-muted-foreground">
							Region for Alibaba Cloud DashScope API. API keys are
							region-specific.
						</p>
					</div>
				)}
```

**Step 5: Add reset in handleClose**

In the `handleClose` function (line ~224), add after the `setAzureValidationModel` reset:

```typescript
			setAlibabaRegion("singapore");
```

**Step 6: Verify build**

Run: `pnpm build --filter ui`
Expected: SUCCESS

**Step 7: Commit**

```bash
git add apps/ui/src/components/provider-keys/create-provider-key-dialog.tsx
git commit -m "feat(ui): add Alibaba region dropdown to provider key dialog"
```

---

### Task 8: Final build and format

**Step 1: Format all code**

Run: `pnpm format`

**Step 2: Full build**

Run: `pnpm build`
Expected: SUCCESS across all packages

**Step 3: Run unit tests**

Run: `pnpm test:unit`
Expected: All tests pass

**Step 4: Commit any formatting changes**

```bash
git add -A
git commit -m "chore: format"
```

---

### Task 9: Update API types (if needed)

**Files:**
- Check: `apps/ui/src/lib/api/v1.d.ts`

**Step 1: Check if API types are auto-generated**

The `v1.d.ts` file may be auto-generated from the API schema. Check if there's a generation command.

Run: `grep -r "openapi\|generate.*types\|v1.d.ts" package.json packages/*/package.json apps/*/package.json | head -20`

If auto-generated, run the generation command. If manually maintained, add `alibaba_region?: string` to the relevant options type.

**Step 2: Verify build**

Run: `pnpm build`
Expected: SUCCESS

**Step 3: Commit if changes were needed**

```bash
git add apps/ui/src/lib/api/v1.d.ts
git commit -m "chore: update API types for alibaba_region"
```
