import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { type FormEvent, useEffect, useState } from "react";
import {
  type VendorId,
  getModelsForVendor,
  resolveVendorId,
} from "../../domain/modelCatalog.ts";
import {
  type ActionId,
  type CardId,
  type CardTheme,
  type CustomAction,
  type ProviderConfig,
  type ProviderId,
  isActionId,
  isProviderId,
} from "../../domain/types.ts";
import { VendorIcon } from "../VendorIcon/index.ts";
import "./SettingsDrawer.css";

export interface HistoryEntry {
  readonly id: CardId;
  readonly sourceText: string;
  readonly providerId: ProviderId;
  readonly actionId: ActionId;
  readonly rawJson: string;
  readonly latencyMs: number;
  readonly createdAt: number;
}

export interface SettingsDrawerProps {
  readonly isOpen: boolean;
  readonly theme: CardTheme;
  readonly providers: readonly ProviderConfig[];
  readonly configuredKeyRefs: readonly string[];
  readonly actions: readonly CustomAction[];
  readonly history: readonly HistoryEntry[];
  readonly onClose: () => void;
  readonly onSaveProvider: (config: ProviderConfig, apiKey?: string) => Promise<void>;
  readonly onSaveAction: (action: CustomAction) => Promise<void>;
  readonly onSelectHistory: (entry: HistoryEntry) => void;
  readonly onRefreshHistory: () => Promise<void>;
}

type DrawerTab = "providers" | "actions" | "history";
type ProviderKind = ProviderConfig["kind"];
type RenderMode = CustomAction["renderMode"];

interface ProviderDraft {
  readonly id: string;
  readonly kind: ProviderKind;
  readonly name: string;
  readonly baseUrl: string;
  readonly model: string;
  readonly apiKeyRef: string;
  readonly apiKeySecret: string;
}

interface ActionDraft {
  readonly id: string;
  readonly name: string;
  readonly systemPrompt: string;
  readonly isBuiltIn: boolean;
  readonly renderMode: RenderMode;
  readonly htmlTemplate: string;
  readonly cssTemplate: string;
}

function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

function parseSelectionAssistantEnabled(raw: unknown): boolean | null {
  if (typeof raw === "boolean") {
    return raw;
  }
  if (typeof raw === "object" && raw !== null) {
    if ("enabled" in raw && typeof raw.enabled === "boolean") {
      return raw.enabled;
    }
    if ("popupEnabled" in raw && typeof raw.popupEnabled === "boolean") {
      return raw.popupEnabled;
    }
  }
  return null;
}

function providerToDraft(provider: ProviderConfig): ProviderDraft {
  switch (provider.kind) {
    case "ollama":
      return {
        id: provider.id,
        kind: "ollama",
        name: provider.name,
        baseUrl: provider.baseUrl,
        model: provider.model,
        apiKeyRef: "",
        apiKeySecret: "",
      };
    case "openai-compatible":
      return {
        id: provider.id,
        kind: "openai-compatible",
        name: provider.name,
        baseUrl: provider.baseUrl,
        model: provider.model,
        apiKeyRef: provider.apiKeyRef,
        apiKeySecret: "",
      };
    case "anthropic":
      return {
        id: provider.id,
        kind: "anthropic",
        name: provider.name,
        baseUrl: provider.baseUrl,
        model: provider.model,
        apiKeyRef: provider.apiKeyRef,
        apiKeySecret: "",
      };
    default: {
      const _exhaustive: never = provider;
      return _exhaustive;
    }
  }
}

interface ProviderPreset {
  readonly id: string;
  readonly vendor: VendorId;
  readonly kind: ProviderKind;
  readonly label: string;
  readonly name: string;
  readonly baseUrl: string;
  readonly model: string;
  readonly apiKeyRef: string;
}

const PROVIDER_PRESETS: readonly ProviderPreset[] = [
  {
    id: "prov-openai",
    vendor: "openai",
    kind: "openai-compatible",
    label: "OpenAI",
    name: "OpenAI · GPT-6.2 Sol",
    baseUrl: "https://api.openai.com/v1",
    model: "gpt-6.2-sol",
    apiKeyRef: "key:openai",
  },
  {
    id: "prov-anthropic",
    vendor: "anthropic",
    kind: "anthropic",
    label: "Anthropic",
    name: "Anthropic · Claude Sonnet 5.5",
    baseUrl: "https://api.anthropic.com",
    model: "claude-sonnet-5-5",
    apiKeyRef: "key:anthropic",
  },
  {
    id: "provider-deepseek",
    vendor: "deepseek",
    kind: "openai-compatible",
    label: "DeepSeek",
    name: "DeepSeek · DeepSeek V4.1 Flash",
    baseUrl: "https://api.deepseek.com/v1",
    model: "deepseek-v4.1-flash-1017",
    apiKeyRef: "deepseek",
  },
  {
    id: "provider-gemini",
    vendor: "gemini",
    kind: "openai-compatible",
    label: "Gemini",
    name: "Google · Gemini 3.8 Flash",
    baseUrl: "https://generativelanguage.googleapis.com/v1beta/openai",
    model: "gemini-3.8-flash",
    apiKeyRef: "gemini",
  },
  {
    id: "provider-openrouter",
    vendor: "openrouter",
    kind: "openai-compatible",
    label: "OpenRouter",
    name: "OpenRouter · Claude Sonnet 5.5",
    baseUrl: "https://openrouter.ai/api/v1",
    model: "anthropic/claude-sonnet-5.5",
    apiKeyRef: "openrouter",
  },
  {
    id: "prov-ollama",
    vendor: "ollama",
    kind: "ollama",
    label: "Ollama",
    name: "Ollama · Qwen 2.5 14B",
    baseUrl: "http://localhost:11434",
    model: "qwen2.5:14b",
    apiKeyRef: "",
  },
];

function carryDraftSecret(prevApiKeyRef: string, nextApiKeyRef: string, prevSecret: string): string {
  const prevRef = prevApiKeyRef.trim();
  const nextRef = nextApiKeyRef.trim();
  return prevRef.length > 0 && prevRef === nextRef ? prevSecret : "";
}

function newProviderDraft(): ProviderDraft {
  return {
    id: `prov-custom-${Date.now()}`,
    kind: "openai-compatible",
    name: "",
    baseUrl: "https://api.openai.com/v1",
    model: "",
    apiKeyRef: `key:custom-${Date.now()}`,
    apiKeySecret: "",
  };
}

function actionToDraft(action: CustomAction): ActionDraft {
  return {
    id: action.id,
    name: action.name,
    systemPrompt: action.systemPrompt,
    isBuiltIn: action.isBuiltIn,
    renderMode: action.renderMode,
    htmlTemplate: action.htmlTemplate ?? "",
    cssTemplate: action.cssTemplate ?? "",
  };
}

function newActionDraft(): ActionDraft {
  return {
    id: `act-custom-${Date.now()}`,
    name: "",
    systemPrompt: "Provide a concise grammatical explanation in Simplified Chinese.",
    isBuiltIn: false,
    renderMode: "custom-html",
    htmlTemplate:
      '<div class="custom-card"><h4>{{sourceText}}</h4><div>{{content}}</div></div>',
    cssTemplate:
      ".custom-card { padding: 14px; border-radius: 10px; line-height: 1.6; }\n.custom-card h4 { margin: 0 0 8px; font-size: 14px; }",
  };
}

function formatTimestamp(ms: number): string {
  if (!Number.isFinite(ms) || ms <= 0) {
    return "";
  }
  return new Date(ms).toLocaleString();
}

export function SettingsDrawer({
  isOpen,
  theme,
  providers,
  configuredKeyRefs,
  actions,
  history,
  onClose,
  onSaveProvider,
  onSaveAction,
  onSelectHistory,
  onRefreshHistory,
}: SettingsDrawerProps) {
  const [tab, setTab] = useState<DrawerTab>("providers");
  const [providerDraft, setProviderDraft] = useState<ProviderDraft>(() =>
    providers[0] ? providerToDraft(providers[0]) : newProviderDraft(),
  );
  const [actionDraft, setActionDraft] = useState<ActionDraft>(() =>
    actions[0] ? actionToDraft(actions[0]) : newActionDraft(),
  );
  const [providerDirty, setProviderDirty] = useState(false);
  const [actionDirty, setActionDirty] = useState(false);
  const [statusMessage, setStatusMessage] = useState<string | null>(null);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [selectionPopupEnabled, setSelectionPopupEnabled] = useState(true);

  useEffect(() => {
    if (providerDirty) {
      return;
    }
    setProviderDraft((prev) => {
      const matched = providers.find((p) => p.id === prev.id) ?? providers[0];
      return matched ? providerToDraft(matched) : prev;
    });
  }, [providers, providerDirty]);

  useEffect(() => {
    if (actionDirty) {
      return;
    }
    setActionDraft((prev) => {
      const matched = actions.find((a) => a.id === prev.id) ?? actions[0];
      return matched ? actionToDraft(matched) : prev;
    });
  }, [actions, actionDirty]);

  useEffect(() => {
    if (!isOpen) {
      setProviderDirty(false);
      setActionDirty(false);
      return undefined;
    }
    setStatusMessage(null);
    setErrorMessage(null);
    if (!isTauriRuntime()) {
      return undefined;
    }
    let active = true;
    let unlistenStatus: (() => void) | null = null;
    void invoke<unknown>("get_selection_assistant_status")
      .then((raw) => {
        if (!active) {
          return;
        }
        const parsed = parseSelectionAssistantEnabled(raw);
        if (parsed !== null) {
          setSelectionPopupEnabled(parsed);
        }
      })
      .catch(() => {});
    void listen<unknown>("assistant://status", (event) => {
      const parsed = parseSelectionAssistantEnabled(event.payload);
      if (parsed !== null) {
        setSelectionPopupEnabled(parsed);
      }
    })
      .then((fn) => {
        if (active) {
          unlistenStatus = fn;
        } else {
          fn();
        }
      })
      .catch(() => {});
    return () => {
      active = false;
      unlistenStatus?.();
    };
  }, [isOpen, tab]);

  if (!isOpen) {
    return null;
  }

  const hasSavedKey =
    providerDraft.apiKeyRef.trim().length > 0 &&
    configuredKeyRefs.includes(providerDraft.apiKeyRef.trim());

  const activeDraftVendor = resolveVendorId({
    id: providerDraft.id,
    baseUrl: providerDraft.baseUrl,
    model: providerDraft.model,
    kind: providerDraft.kind,
  });
  const vendorModelPresets = getModelsForVendor(activeDraftVendor);

  const handleToggleSelectionPopup = async () => {
    const next = !selectionPopupEnabled;
    setSelectionPopupEnabled(next);
    if (isTauriRuntime()) {
      try {
        await invoke("set_selection_popup_enabled", { enabled: next });
      } catch {
        setSelectionPopupEnabled(!next);
      }
    }
  };

  const handleProviderSubmit = async (event: FormEvent) => {
    event.preventDefault();
    setErrorMessage(null);
    setStatusMessage(null);

    const id = providerDraft.id.trim();
    if (!isProviderId(id)) {
      setErrorMessage("Provider ID is required.");
      return;
    }

    const name = providerDraft.name.trim();
    const baseUrl = providerDraft.baseUrl.trim();
    const model = providerDraft.model.trim();
    if (!name || !baseUrl || !model) {
      setErrorMessage("Name, Base URL, and Model are required.");
      return;
    }

    const secret = providerDraft.apiKeySecret.trim();
    const effectiveApiKeyRef =
      providerDraft.kind === "ollama"
        ? ""
        : providerDraft.apiKeyRef.trim() || (secret.length > 0 ? `key:${id}` : "");

    let config: ProviderConfig;
    switch (providerDraft.kind) {
      case "ollama":
        config = { kind: "ollama", id, name, baseUrl, model };
        break;
      case "openai-compatible":
        config = {
          kind: "openai-compatible",
          id,
          name,
          baseUrl,
          apiKeyRef: effectiveApiKeyRef,
          model,
        };
        break;
      case "anthropic":
        config = {
          kind: "anthropic",
          id,
          name,
          baseUrl,
          apiKeyRef: effectiveApiKeyRef,
          model,
        };
        break;
      default: {
        const _exhaustive: never = providerDraft.kind;
        return _exhaustive;
      }
    }

    setSaving(true);
    try {
      await onSaveProvider(config, secret.length > 0 ? secret : undefined);
      setProviderDraft((prev) => ({
        ...prev,
        apiKeyRef: effectiveApiKeyRef,
        apiKeySecret: "",
      }));
      setProviderDirty(false);
      setStatusMessage(`Saved provider "${name}".`);
    } catch (err) {
      setErrorMessage(err instanceof Error ? err.message : String(err));
    } finally {
      setSaving(false);
    }
  };

  const handleActionSubmit = async (event: FormEvent) => {
    event.preventDefault();
    setErrorMessage(null);
    setStatusMessage(null);

    const id = actionDraft.id.trim();
    if (!isActionId(id)) {
      setErrorMessage("Action ID is required.");
      return;
    }

    const name = actionDraft.name.trim();
    if (!name) {
      setErrorMessage("Action name is required.");
      return;
    }

    const action: CustomAction = {
      id,
      name,
      systemPrompt: actionDraft.systemPrompt,
      isBuiltIn: actionDraft.isBuiltIn,
      renderMode: actionDraft.renderMode,
      htmlTemplate:
        actionDraft.renderMode === "custom-html" && actionDraft.htmlTemplate.trim().length > 0
          ? actionDraft.htmlTemplate
          : null,
      cssTemplate:
        actionDraft.renderMode === "custom-html" && actionDraft.cssTemplate.trim().length > 0
          ? actionDraft.cssTemplate
          : null,
    };

    setSaving(true);
    try {
      await onSaveAction(action);
      setActionDirty(false);
      setStatusMessage(`Saved action "${name}".`);
    } catch (err) {
      setErrorMessage(err instanceof Error ? err.message : String(err));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div
      className="drawer-backdrop"
      data-card-theme={theme}
      role="dialog"
      aria-modal="true"
      aria-label="Settings"
      onClick={(event) => {
        if (event.target === event.currentTarget) {
          onClose();
        }
      }}
    >
      <aside className="drawer-panel">
        <header className="drawer-header">
          <h2>Settings &amp; History</h2>
          <button type="button" className="app-btn" onClick={onClose}>
            Close
          </button>
        </header>

        <nav className="drawer-tabs" aria-label="Settings sections">
          <button
            type="button"
            className={`app-btn${tab === "providers" ? " primary" : ""}`}
            onClick={() => setTab("providers")}
          >
            Providers ({providers.length})
          </button>
          <button
            type="button"
            className={`app-btn${tab === "actions" ? " primary" : ""}`}
            onClick={() => setTab("actions")}
          >
            Actions ({actions.length})
          </button>
          <button
            type="button"
            className={`app-btn${tab === "history" ? " primary" : ""}`}
            onClick={() => {
              setTab("history");
              void onRefreshHistory();
            }}
          >
            History ({history.length})
          </button>
        </nav>

        <div className="drawer-body">
          <div className="drawer-assistant-row">
            <div className="drawer-assistant-info">
              <span className="drawer-assistant-title">Background Selection Assistant</span>
              <span className="drawer-assistant-desc">
                Show floating confirmation pill when selecting English sentences
              </span>
            </div>
            <button
              type="button"
              role="switch"
              aria-checked={selectionPopupEnabled}
              aria-label="Background Selection Assistant"
              className={`app-btn${selectionPopupEnabled ? " primary" : ""}`}
              onClick={() => void handleToggleSelectionPopup()}
            >
              {selectionPopupEnabled ? "Enabled" : "Paused"}
            </button>
          </div>

          {tab === "providers" ? (
            <>
              <div className="drawer-list">
                {providers.map((provider) => {
                  const vendor = resolveVendorId(provider);
                  return (
                    <button
                      key={provider.id}
                      type="button"
                      className={`drawer-item${providerDraft.id === provider.id ? " active" : ""}`}
                      onClick={() => {
                        setProviderDraft(providerToDraft(provider));
                        setProviderDirty(false);
                        setStatusMessage(null);
                        setErrorMessage(null);
                      }}
                    >
                      <span className="drawer-item-title">
                        <span className="drawer-item-brand">
                          <VendorIcon vendor={vendor} size={15} />
                          <span>{provider.name}</span>
                        </span>
                        <span>{provider.kind}</span>
                      </span>
                      <span className="drawer-item-meta">
                        {provider.model} · {provider.baseUrl}
                      </span>
                    </button>
                  );
                })}
                <button
                  type="button"
                  className="app-btn"
                  onClick={() => {
                    setProviderDraft(newProviderDraft());
                    setProviderDirty(true);
                  }}
                >
                  + Add Provider
                </button>
              </div>

              <form className="drawer-form" onSubmit={(e) => void handleProviderSubmit(e)}>
                <div className="drawer-presets">
                  <span className="drawer-presets-label">Quick Presets:</span>
                  {PROVIDER_PRESETS.map((preset) => (
                    <button
                      key={preset.label}
                      type="button"
                      className="app-btn"
                      onClick={() => {
                        const existing = providers.find((p) => p.id === preset.id);
                        setProviderDraft((prev) => {
                          const nextDraft = existing
                            ? providerToDraft(existing)
                            : {
                                id: preset.id,
                                kind: preset.kind,
                                name: preset.name,
                                baseUrl: preset.baseUrl,
                                model: preset.model,
                                apiKeyRef: preset.apiKeyRef,
                                apiKeySecret: "",
                              };
                          return {
                            ...nextDraft,
                            apiKeySecret: carryDraftSecret(
                              prev.apiKeyRef,
                              nextDraft.apiKeyRef,
                              prev.apiKeySecret,
                            ),
                          };
                        });
                        setProviderDirty(true);
                        setStatusMessage(null);
                        setErrorMessage(null);
                      }}
                    >
                      <VendorIcon vendor={preset.vendor} size={14} />
                      {preset.label}
                    </button>
                  ))}
                </div>

                {vendorModelPresets.length > 0 ? (
                  <div className="drawer-model-chips" aria-label="Vendor models">
                    <span className="drawer-presets-label">Models:</span>
                    {vendorModelPresets.map((modelPreset) => (
                      <button
                        key={modelPreset.id}
                        type="button"
                        className={`app-btn${providerDraft.model === modelPreset.model ? " primary" : ""}`}
                        data-model-preset={modelPreset.id}
                        onClick={() => {
                          const existing = providers.find((p) => p.id === modelPreset.id);
                          setProviderDraft((prev) => {
                            const nextDraft = existing
                              ? providerToDraft(existing)
                              : {
                                  id: modelPreset.id,
                                  kind: modelPreset.kind,
                                  name: modelPreset.name,
                                  baseUrl: modelPreset.baseUrl,
                                  model: modelPreset.model,
                                  apiKeyRef: modelPreset.apiKeyRef,
                                  apiKeySecret: "",
                                };
                            return {
                              ...nextDraft,
                              apiKeySecret: carryDraftSecret(
                                prev.apiKeyRef,
                                nextDraft.apiKeyRef,
                                prev.apiKeySecret,
                              ),
                            };
                          });
                          setProviderDirty(true);
                          setStatusMessage(null);
                          setErrorMessage(null);
                        }}
                      >
                        {modelPreset.shortLabel}
                      </button>
                    ))}
                  </div>
                ) : null}

                <label className="drawer-field">
                  Provider Kind
                  <select
                    value={providerDraft.kind}
                    onChange={(e) => {
                      const nextKind = e.target.value;
                      if (
                        nextKind === "ollama" ||
                        nextKind === "openai-compatible" ||
                        nextKind === "anthropic"
                      ) {
                        setProviderDirty(true);
                        setProviderDraft((prev) => ({ ...prev, kind: nextKind }));
                      }
                    }}
                  >
                    <option value="ollama">Ollama (Local)</option>
                    <option value="openai-compatible">OpenAI-Compatible (Local / Cloud)</option>
                    <option value="anthropic">Anthropic (Cloud)</option>
                  </select>
                </label>

                <label className="drawer-field">
                  Display Name
                  <input
                    type="text"
                    value={providerDraft.name}
                    onChange={(e) => {
                      setProviderDirty(true);
                      setProviderDraft((prev) => ({ ...prev, name: e.target.value }));
                    }}
                    placeholder="e.g. OpenAI · GPT-6.2 Sol"
                  />
                </label>

                <label className="drawer-field">
                  Base URL
                  <input
                    type="text"
                    value={providerDraft.baseUrl}
                    onChange={(e) => {
                      setProviderDirty(true);
                      setProviderDraft((prev) => ({ ...prev, baseUrl: e.target.value }));
                    }}
                    placeholder="http://localhost:11434"
                  />
                </label>

                <label className="drawer-field">
                  Model
                  <input
                    type="text"
                    value={providerDraft.model}
                    onChange={(e) => {
                      setProviderDirty(true);
                      setProviderDraft((prev) => ({ ...prev, model: e.target.value }));
                    }}
                    placeholder="qwen2.5:14b"
                  />
                </label>

                {providerDraft.kind !== "ollama" ? (
                  <>
                    <label className="drawer-field">
                      API Key Reference
                      <input
                        type="text"
                        value={providerDraft.apiKeyRef}
                        onChange={(e) => {
                          setProviderDirty(true);
                          setProviderDraft((prev) => ({ ...prev, apiKeyRef: e.target.value }));
                        }}
                        placeholder="key:openai (leave empty for local LM Studio)"
                      />
                    </label>
                    <label className="drawer-field">
                      API Key {hasSavedKey ? "(Saved — enter new value to replace)" : "(Optional)"}
                      <input
                        type="password"
                        value={providerDraft.apiKeySecret}
                        onChange={(e) => {
                          setProviderDirty(true);
                          setProviderDraft((prev) => ({ ...prev, apiKeySecret: e.target.value }));
                        }}
                        placeholder="sk-..."
                      />
                    </label>
                  </>
                ) : null}

                {statusMessage ? <div className="drawer-status">{statusMessage}</div> : null}
                {errorMessage ? <div className="drawer-error">{errorMessage}</div> : null}

                <button type="submit" className="app-btn primary" disabled={saving}>
                  {saving ? "Saving..." : "Save Provider"}
                </button>
              </form>
            </>
          ) : null}

          {tab === "actions" ? (
            <>
              <div className="drawer-list">
                {actions.map((action) => (
                  <button
                    key={action.id}
                    type="button"
                    className={`drawer-item${actionDraft.id === action.id ? " active" : ""}`}
                    onClick={() => {
                      setActionDraft(actionToDraft(action));
                      setActionDirty(false);
                      setStatusMessage(null);
                      setErrorMessage(null);
                    }}
                  >
                    <span className="drawer-item-title">
                      <span>{action.name}</span>
                      <span>{action.renderMode}</span>
                    </span>
                    <span className="drawer-item-meta">{action.systemPrompt}</span>
                  </button>
                ))}
                <button
                  type="button"
                  className="app-btn"
                  onClick={() => {
                    setActionDraft(newActionDraft());
                    setActionDirty(true);
                  }}
                >
                  + Add Custom Action
                </button>
              </div>

              <form className="drawer-form" onSubmit={(e) => void handleActionSubmit(e)}>
                <label className="drawer-field">
                  Action Name
                  <input
                    type="text"
                    value={actionDraft.name}
                    onChange={(e) => {
                      setActionDirty(true);
                      setActionDraft((prev) => ({ ...prev, name: e.target.value }));
                    }}
                    placeholder="e.g. Compact HTML Card"
                  />
                </label>

                <label className="drawer-field">
                  Render Mode
                  <select
                    value={actionDraft.renderMode}
                    onChange={(e) => {
                      const nextMode = e.target.value;
                      if (nextMode === "unified-3layer" || nextMode === "custom-html") {
                        setActionDirty(true);
                        setActionDraft((prev) => ({ ...prev, renderMode: nextMode }));
                      }
                    }}
                  >
                    <option value="unified-3layer">Unified 3-Layer Card</option>
                    <option value="custom-html">Custom HTML Sandbox (DOMPurify + Shadow DOM)</option>
                  </select>
                </label>

                <label className="drawer-field">
                  System Prompt
                  <textarea
                    value={actionDraft.systemPrompt}
                    onChange={(e) => {
                      setActionDirty(true);
                      setActionDraft((prev) => ({ ...prev, systemPrompt: e.target.value }));
                    }}
                  />
                </label>

                {actionDraft.renderMode === "custom-html" ? (
                  <>
                    <label className="drawer-field">
                      HTML Template (supports {"{{sourceText}}"} and {"{{content}}"})
                      <textarea
                        value={actionDraft.htmlTemplate}
                        onChange={(e) => {
                          setActionDirty(true);
                          setActionDraft((prev) => ({ ...prev, htmlTemplate: e.target.value }));
                        }}
                      />
                    </label>
                    <label className="drawer-field">
                      CSS Template (scoped inside Shadow DOM)
                      <textarea
                        value={actionDraft.cssTemplate}
                        onChange={(e) => {
                          setActionDirty(true);
                          setActionDraft((prev) => ({ ...prev, cssTemplate: e.target.value }));
                        }}
                      />
                    </label>
                  </>
                ) : null}

                {statusMessage ? <div className="drawer-status">{statusMessage}</div> : null}
                {errorMessage ? <div className="drawer-error">{errorMessage}</div> : null}

                <button type="submit" className="app-btn primary" disabled={saving}>
                  {saving ? "Saving..." : "Save Action"}
                </button>
              </form>
            </>
          ) : null}

          {tab === "history" ? (
            <div className="drawer-list">
              {history.length === 0 ? (
                <div className="empty-state">No saved analyses yet.</div>
              ) : (
                history.map((entry) => (
                  <button
                    key={entry.id}
                    type="button"
                    className="drawer-item"
                    onClick={() => {
                      onSelectHistory(entry);
                      onClose();
                    }}
                  >
                    <span className="drawer-item-title">
                      <span>{entry.sourceText}</span>
                    </span>
                    <span className="drawer-item-meta">
                      {entry.latencyMs}ms · {formatTimestamp(entry.createdAt)}
                    </span>
                  </button>
                ))
              )}
            </div>
          ) : null}
        </div>
      </aside>
    </div>
  );
}
