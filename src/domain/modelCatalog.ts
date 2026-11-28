import { type ProviderConfig, type ProviderId, isProviderId } from "./types.ts";

export type VendorId =
  | "openai"
  | "anthropic"
  | "deepseek"
  | "gemini"
  | "openrouter"
  | "ollama"
  | "lmstudio"
  | "custom";

export interface VendorMeta {
  readonly id: VendorId;
  readonly label: string;
  readonly groupLabel: string;
  readonly defaultKind: ProviderConfig["kind"];
  readonly defaultBaseUrl: string;
  readonly defaultApiKeyRef: string;
}

export const VENDOR_META: Readonly<Record<VendorId, VendorMeta>> = {
  openai: {
    id: "openai",
    label: "OpenAI",
    groupLabel: "OpenAI",
    defaultKind: "openai-compatible",
    defaultBaseUrl: "https://api.openai.com/v1",
    defaultApiKeyRef: "key:openai",
  },
  anthropic: {
    id: "anthropic",
    label: "Anthropic",
    groupLabel: "Anthropic",
    defaultKind: "anthropic",
    defaultBaseUrl: "https://api.anthropic.com",
    defaultApiKeyRef: "key:anthropic",
  },
  deepseek: {
    id: "deepseek",
    label: "DeepSeek",
    groupLabel: "DeepSeek",
    defaultKind: "openai-compatible",
    defaultBaseUrl: "https://api.deepseek.com/v1",
    defaultApiKeyRef: "deepseek",
  },
  gemini: {
    id: "gemini",
    label: "Gemini",
    groupLabel: "Google Gemini",
    defaultKind: "openai-compatible",
    defaultBaseUrl: "https://generativelanguage.googleapis.com/v1beta/openai",
    defaultApiKeyRef: "gemini",
  },
  openrouter: {
    id: "openrouter",
    label: "OpenRouter",
    groupLabel: "OpenRouter",
    defaultKind: "openai-compatible",
    defaultBaseUrl: "https://openrouter.ai/api/v1",
    defaultApiKeyRef: "openrouter",
  },
  ollama: {
    id: "ollama",
    label: "Ollama",
    groupLabel: "Local",
    defaultKind: "ollama",
    defaultBaseUrl: "http://localhost:11434",
    defaultApiKeyRef: "",
  },
  lmstudio: {
    id: "lmstudio",
    label: "LM Studio",
    groupLabel: "Local",
    defaultKind: "openai-compatible",
    defaultBaseUrl: "http://localhost:1234/v1",
    defaultApiKeyRef: "",
  },
  custom: {
    id: "custom",
    label: "Custom",
    groupLabel: "Custom",
    defaultKind: "openai-compatible",
    defaultBaseUrl: "https://api.openai.com/v1",
    defaultApiKeyRef: "",
  },
};

export interface ModelPreset {
  readonly id: ProviderId;
  readonly vendor: VendorId;
  readonly shortLabel: string;
  readonly kind: ProviderConfig["kind"];
  readonly name: string;
  readonly baseUrl: string;
  readonly apiKeyRef: string;
  readonly model: string;
}

function mustProviderId(raw: string): ProviderId {
  if (!isProviderId(raw)) {
    throw new Error(`Invalid built-in ProviderId: ${raw}`);
  }
  return raw;
}

export const MODEL_PRESETS: readonly ModelPreset[] = [
  {
    id: mustProviderId("prov-ollama"),
    vendor: "ollama",
    shortLabel: "Qwen 2.5 14B",
    kind: "ollama",
    name: "Ollama · Qwen 2.5 14B",
    baseUrl: "http://localhost:11434",
    apiKeyRef: "",
    model: "qwen2.5:14b",
  },
  {
    id: mustProviderId("prov-lmstudio"),
    vendor: "lmstudio",
    shortLabel: "Qwen 2.5 14B (LM Studio)",
    kind: "openai-compatible",
    name: "LM Studio · Qwen 2.5 14B",
    baseUrl: "http://localhost:1234/v1",
    apiKeyRef: "",
    model: "qwen2.5-14b-instruct",
  },
  {
    id: mustProviderId("prov-openai"),
    vendor: "openai",
    shortLabel: "GPT-6.2 Sol",
    kind: "openai-compatible",
    name: "OpenAI · GPT-6.2 Sol",
    baseUrl: "https://api.openai.com/v1",
    apiKeyRef: "key:openai",
    model: "gpt-6.2-sol",
  },
  {
    id: mustProviderId("prov-openai-gpt6-luna"),
    vendor: "openai",
    shortLabel: "GPT-6 Luna",
    kind: "openai-compatible",
    name: "OpenAI · GPT-6 Luna",
    baseUrl: "https://api.openai.com/v1",
    apiKeyRef: "key:openai",
    model: "gpt-6-luna",
  },
  {
    id: mustProviderId("prov-openai-gpt62-astra"),
    vendor: "openai",
    shortLabel: "GPT-6.2 Astra",
    kind: "openai-compatible",
    name: "OpenAI · GPT-6.2 Astra",
    baseUrl: "https://api.openai.com/v1",
    apiKeyRef: "key:openai",
    model: "gpt-6.2-astra",
  },
  {
    id: mustProviderId("prov-anthropic"),
    vendor: "anthropic",
    shortLabel: "Claude Sonnet 5.5",
    kind: "anthropic",
    name: "Anthropic · Claude Sonnet 5.5",
    baseUrl: "https://api.anthropic.com",
    apiKeyRef: "key:anthropic",
    model: "claude-sonnet-5-5",
  },
  {
    id: mustProviderId("prov-anthropic-haiku-55"),
    vendor: "anthropic",
    shortLabel: "Claude Haiku 5.5",
    kind: "anthropic",
    name: "Anthropic · Claude Haiku 5.5",
    baseUrl: "https://api.anthropic.com",
    apiKeyRef: "key:anthropic",
    model: "claude-haiku-5-5",
  },
  {
    id: mustProviderId("prov-anthropic-opus-55"),
    vendor: "anthropic",
    shortLabel: "Claude Opus 5.5",
    kind: "anthropic",
    name: "Anthropic · Claude Opus 5.5",
    baseUrl: "https://api.anthropic.com",
    apiKeyRef: "key:anthropic",
    model: "claude-opus-5-5",
  },
  {
    id: mustProviderId("provider-deepseek"),
    vendor: "deepseek",
    shortLabel: "DeepSeek V4.1 Flash",
    kind: "openai-compatible",
    name: "DeepSeek · DeepSeek V4.1 Flash",
    baseUrl: "https://api.deepseek.com/v1",
    apiKeyRef: "deepseek",
    model: "deepseek-v4.1-flash-1017",
  },
  {
    id: mustProviderId("prov-deepseek-v4-pro"),
    vendor: "deepseek",
    shortLabel: "DeepSeek V4 Pro",
    kind: "openai-compatible",
    name: "DeepSeek · DeepSeek V4 Pro",
    baseUrl: "https://api.deepseek.com/v1",
    apiKeyRef: "deepseek",
    model: "deepseek-v4-pro",
  },
  {
    id: mustProviderId("provider-gemini"),
    vendor: "gemini",
    shortLabel: "Gemini 3.8 Flash",
    kind: "openai-compatible",
    name: "Google · Gemini 3.8 Flash",
    baseUrl: "https://generativelanguage.googleapis.com/v1beta/openai",
    apiKeyRef: "gemini",
    model: "gemini-3.8-flash",
  },
  {
    id: mustProviderId("prov-gemini-4-hydrogen"),
    vendor: "gemini",
    shortLabel: "Gemini 4 Hydrogen",
    kind: "openai-compatible",
    name: "Google · Gemini 4 Hydrogen",
    baseUrl: "https://generativelanguage.googleapis.com/v1beta/openai",
    apiKeyRef: "gemini",
    model: "gemini-4-hydrogen",
  },
  {
    id: mustProviderId("prov-gemini-4-carbon"),
    vendor: "gemini",
    shortLabel: "Gemini 4 Carbon",
    kind: "openai-compatible",
    name: "Google · Gemini 4 Carbon",
    baseUrl: "https://generativelanguage.googleapis.com/v1beta/openai",
    apiKeyRef: "gemini",
    model: "gemini-4-carbon",
  },
  {
    id: mustProviderId("provider-openrouter"),
    vendor: "openrouter",
    shortLabel: "Claude Sonnet 5.5",
    kind: "openai-compatible",
    name: "OpenRouter · Claude Sonnet 5.5",
    baseUrl: "https://openrouter.ai/api/v1",
    apiKeyRef: "openrouter",
    model: "anthropic/claude-sonnet-5.5",
  },
];

export function presetToProviderConfig(preset: ModelPreset): ProviderConfig {
  switch (preset.kind) {
    case "ollama":
      return {
        kind: "ollama",
        id: preset.id,
        name: preset.name,
        baseUrl: preset.baseUrl,
        model: preset.model,
      };
    case "openai-compatible":
      return {
        kind: "openai-compatible",
        id: preset.id,
        name: preset.name,
        baseUrl: preset.baseUrl,
        apiKeyRef: preset.apiKeyRef,
        model: preset.model,
      };
    case "anthropic":
      return {
        kind: "anthropic",
        id: preset.id,
        name: preset.name,
        baseUrl: preset.baseUrl,
        apiKeyRef: preset.apiKeyRef,
        model: preset.model,
      };
    default: {
      const _exhaustive: never = preset.kind;
      return _exhaustive;
    }
  }
}

export function buildDefaultProviders(): readonly ProviderConfig[] {
  return MODEL_PRESETS.map(presetToProviderConfig);
}

export function resolveVendorId(
  provider: {
    readonly id: string;
    readonly baseUrl: string;
    readonly model: string;
    readonly kind: ProviderConfig["kind"];
  },
): VendorId {
  const url = provider.baseUrl.trim().toLowerCase();
  const id = provider.id.trim().toLowerCase();

  if (url.includes("openrouter.ai") || id.includes("openrouter")) {
    return "openrouter";
  }
  if (url.includes("api.deepseek.com") || id.includes("deepseek")) {
    return "deepseek";
  }
  if (url.includes("generativelanguage.googleapis.com") || id.includes("gemini")) {
    return "gemini";
  }
  if (provider.kind === "anthropic" || url.includes("api.anthropic.com") || id.includes("anthropic")) {
    return "anthropic";
  }
  if (provider.kind === "ollama" || url.includes(":11434") || id === "prov-ollama") {
    return "ollama";
  }
  if (url.includes(":1234") || id === "prov-lmstudio") {
    return "lmstudio";
  }
  if (url.includes("api.openai.com") && !id.startsWith("prov-custom-")) {
    return "openai";
  }
  if (url.includes("api.openai.com") && provider.model.toLowerCase().startsWith("gpt-")) {
    return "openai";
  }
  return "custom";
}

export function formatShortModelLabel(provider: ProviderConfig): string {
  const exactPreset = MODEL_PRESETS.find(
    (preset) => preset.id === provider.id && preset.model === provider.model,
  );
  if (exactPreset) {
    return exactPreset.shortLabel;
  }

  const vendor = resolveVendorId(provider);
  const modelPreset = MODEL_PRESETS.find(
    (preset) => preset.vendor === vendor && preset.model === provider.model,
  );
  if (modelPreset) {
    return modelPreset.shortLabel;
  }

  const dotIdx = provider.name.indexOf("·");
  if (dotIdx !== -1) {
    const afterDot = provider.name.slice(dotIdx + 1).trim();
    if (afterDot.length > 0) {
      return afterDot;
    }
  }

  return provider.name.trim() || provider.model.trim();
}

export function getModelsForVendor(vendor: VendorId): readonly ModelPreset[] {
  return MODEL_PRESETS.filter((preset) => preset.vendor === vendor);
}
