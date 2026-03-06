# Provider Region Support Design

## Problem

Some cloud providers host AI models in specific geographic regions with different API endpoints. Alibaba Cloud's DashScope has 3 regional endpoints (Singapore, US Virginia, China Beijing), each requiring region-specific API keys. Currently, the gateway hardcodes `dashscope-intl.aliyuncs.com` (Singapore). To support models that require different regions, we need a general region abstraction.

This also applies to AWS Bedrock (which has a hacky region prefix system today) and could extend to other providers in the future.

## Approach

Add a `regions` array to `ProviderModelMapping` that declares which regions each model supports. The actual region selection happens at runtime via environment variables (credits mode) or provider key options (api-keys mode).

## Data Model Changes

### `ProviderModelMapping` (packages/models/src/models.ts)

New optional field:

```typescript
/** Available regions for this model/provider combination.
  * When set, the gateway uses the configured region (env var or provider key)
  * and validates it against this list.
  * When unset, the provider uses its default endpoint. */
regions?: string[];
```

### `ProviderDefinition` (packages/models/src/providers.ts)

Add optional region env var to Alibaba's config:

```typescript
{
  id: "alibaba",
  env: {
    required: { apiKey: "LLM_ALIBABA_API_KEY" },
    optional: { region: "LLM_ALIBABA_REGION" },
  },
}
```

### `ProviderKeyOptions` (packages/db)

Add Alibaba region for user-managed provider keys:

```typescript
export interface ProviderKeyOptions {
  // ...existing fields
  alibaba_region?: string;
}
```

### Alibaba Model Definitions (packages/models/src/models/alibaba.ts)

Add `regions` to each `providerId: "alibaba"` mapping:

```typescript
{
  providerId: "alibaba",
  modelName: "qwen-plus",
  regions: ["singapore", "us-virginia", "cn-beijing"],
  // ...rest unchanged
}
```

## Endpoint URL Resolution

In `packages/actions/src/get-provider-endpoint.ts`, the Alibaba case changes from a hardcoded URL to region-aware resolution:

```typescript
case "alibaba": {
  const region =
    providerKeyOptions?.alibaba_region ??
    getProviderEnvValue("alibaba", "region", configIndex, "singapore") ??
    "singapore";

  const baseUrls: Record<string, string> = {
    "singapore": "https://dashscope-intl.aliyuncs.com",
    "us-virginia": "https://dashscope-us.aliyuncs.com",
    "cn-beijing": "https://dashscope.aliyuncs.com",
  };

  if (imageGenerations) {
    url = baseUrls[region] ?? baseUrls["singapore"];
  } else {
    url = `${baseUrls[region] ?? baseUrls["singapore"]}/compatible-mode`;
  }
  break;
}
```

## Request-Time Validation

In `apps/gateway/src/chat/tools/resolve-provider-context.ts`, after the provider mapping is found, validate the configured region:

```typescript
if (providerMapping.regions && providerMapping.regions.length > 0) {
  const configuredRegion = /* resolve from providerKeyOptions or env */;
  if (configuredRegion && !providerMapping.regions.includes(configuredRegion)) {
    throw new HTTPException(400, {
      message: `Model ${model} is not available in region ${configuredRegion}. Available regions: ${providerMapping.regions.join(", ")}`,
    });
  }
}
```

## UI Changes

In `apps/ui/src/components/provider-keys/create-provider-key-dialog.tsx`, add a region dropdown for Alibaba provider keys (same pattern as AWS Bedrock region prefix and Azure resource):

- State: `const [alibabaRegion, setAlibabaRegion] = useState<string>("singapore")`
- Dropdown options: Singapore (default), US (Virginia), China (Beijing)
- Saved to `payload.options.alibaba_region`

## Files to Modify

1. `packages/models/src/models.ts` - Add `regions?` to `ProviderModelMapping`
2. `packages/models/src/providers.ts` - Add optional region env var to Alibaba
3. `packages/models/src/models/alibaba.ts` - Add `regions` to all Alibaba provider mappings
4. `packages/db` - Add `alibaba_region` to `ProviderKeyOptions`
5. `packages/actions/src/get-provider-endpoint.ts` - Region-aware URL resolution for Alibaba
6. `apps/gateway/src/chat/tools/resolve-provider-context.ts` - Region validation at request time
7. `apps/ui/src/components/provider-keys/create-provider-key-dialog.tsx` - Alibaba region dropdown
8. `apps/ui/src/lib/api/v1.d.ts` - Update API types if needed

## Future Work

- Migrate AWS Bedrock from the hacky `aws_bedrock_region_prefix` to use the same `regions` pattern
- Add region support to other providers as needed (e.g., Google Vertex could use this instead of its custom env var)
