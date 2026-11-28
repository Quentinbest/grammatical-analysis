import { useEffect, useMemo, useRef, useState } from "react";
import {
  type VendorId,
  formatShortModelLabel,
  resolveVendorId,
} from "../../domain/modelCatalog.ts";
import type { ProviderConfig, ProviderId } from "../../domain/types.ts";
import { VendorIcon } from "../VendorIcon/index.ts";

export interface ModelPickerProps {
  readonly providers: readonly ProviderConfig[];
  readonly selectedProviderId: ProviderId | null;
  readonly configuredKeyRefs: readonly string[];
  readonly onSelectProvider: (id: ProviderId) => void;
}

interface ProviderGroup {
  readonly label: string;
  readonly items: readonly { readonly provider: ProviderConfig; readonly vendor: VendorId }[];
}

const GROUP_DEFINITIONS: readonly {
  readonly label: string;
  readonly match: (vendor: VendorId) => boolean;
}[] = [
  { label: "OpenAI", match: (v) => v === "openai" },
  { label: "Anthropic", match: (v) => v === "anthropic" },
  { label: "DeepSeek", match: (v) => v === "deepseek" },
  { label: "Google Gemini", match: (v) => v === "gemini" },
  { label: "OpenRouter", match: (v) => v === "openrouter" },
  { label: "Local", match: (v) => v === "ollama" || v === "lmstudio" },
  { label: "Custom", match: (v) => v === "custom" },
];

function isProviderReady(
  provider: ProviderConfig,
  configuredKeyRefs: readonly string[],
): boolean {
  if (provider.kind === "ollama") {
    return true;
  }
  const ref = provider.apiKeyRef.trim();
  if (ref.length === 0) {
    return true;
  }
  return configuredKeyRefs.includes(ref);
}

export function ModelPicker({
  providers,
  selectedProviderId,
  configuredKeyRefs,
  onSelectProvider,
}: ModelPickerProps) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement | null>(null);

  const activeProvider = useMemo(
    () => providers.find((p) => p.id === selectedProviderId) ?? providers[0] ?? null,
    [providers, selectedProviderId],
  );
  const activeVendor: VendorId = activeProvider ? resolveVendorId(activeProvider) : "custom";

  const groups = useMemo((): readonly ProviderGroup[] => {
    const tagged = providers.map((provider) => ({
      provider,
      vendor: resolveVendorId(provider),
    }));
    const result: ProviderGroup[] = [];
    for (const def of GROUP_DEFINITIONS) {
      const items = tagged.filter((entry) => def.match(entry.vendor));
      if (items.length > 0) {
        result.push({ label: def.label, items });
      }
    }
    return result;
  }, [providers]);

  useEffect(() => {
    if (!open) {
      return undefined;
    }
    const onPointerDown = (event: MouseEvent) => {
      const root = rootRef.current;
      if (root && event.target instanceof Node && !root.contains(event.target)) {
        setOpen(false);
      }
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.stopPropagation();
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", onPointerDown);
    window.addEventListener("keydown", onKeyDown, true);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      window.removeEventListener("keydown", onKeyDown, true);
    };
  }, [open]);

  return (
    <div ref={rootRef} className="model-picker">
      <button
        type="button"
        className="model-picker-trigger"
        aria-label="Provider"
        aria-haspopup="listbox"
        aria-expanded={open}
        onClick={() => setOpen((prev) => !prev)}
      >
        <VendorIcon vendor={activeVendor} size={15} />
        <span className="model-picker-label">
          {activeProvider ? formatShortModelLabel(activeProvider) : "Select model"}
        </span>
        <svg
          className={`model-picker-chevron${open ? " open" : ""}`}
          width="12"
          height="12"
          viewBox="0 0 12 12"
          fill="none"
          aria-hidden="true"
        >
          <path
            d="M3 4.5 6 7.5 9 4.5"
            stroke="currentColor"
            strokeWidth="1.4"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
        </svg>
      </button>

      {open ? (
        <div className="model-picker-popover" role="listbox" aria-label="Provider models">
          {groups.map((group) => (
            <div key={group.label} className="model-picker-group">
              <div className="model-picker-group-label">{group.label}</div>
              {group.items.map(({ provider, vendor }) => {
                const selected = activeProvider?.id === provider.id;
                const ready = isProviderReady(provider, configuredKeyRefs);
                return (
                  <button
                    key={provider.id}
                    type="button"
                    role="option"
                    aria-selected={selected}
                    className={`model-picker-option${selected ? " selected" : ""}`}
                    data-provider-id={provider.id}
                    onClick={() => {
                      onSelectProvider(provider.id);
                      setOpen(false);
                    }}
                  >
                    <VendorIcon vendor={vendor} size={15} />
                    <span className="model-picker-opt-title">
                      {formatShortModelLabel(provider)}
                    </span>
                    <span className="model-picker-opt-slug">{provider.model}</span>
                    <span
                      className={`model-picker-key-dot${ready ? " ready" : " missing"}`}
                      title={ready ? "Ready" : "API key not configured"}
                      aria-hidden="true"
                    />
                  </button>
                );
              })}
            </div>
          ))}
        </div>
      ) : null}
    </div>
  );
}
