import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { type FormEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { CustomTemplateSandbox } from "./components/CustomTemplateSandbox/index.ts";
import { type HistoryEntry, SettingsDrawer } from "./components/SettingsDrawer/index.ts";
import { UnifiedCard } from "./components/UnifiedCard/index.ts";
import {
  type ActionId,
  type CardTheme,
  type CustomAction,
  type ProviderConfig,
  type ProviderId,
  isActionId,
  isProviderId,
} from "./domain/types.ts";
import { type CompletedAnalysisPayload, useAnalysisStream } from "./hooks/useAnalysisStream.ts";
import "./styles/tokens.css";

interface SelectionCapturedEvent {
  readonly text: string;
  readonly hotkey: string;
  readonly latencyMs: number;
}

interface SelectionErrorEvent {
  readonly code: string;
  readonly message: string;
  readonly hotkey: string;
}

function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

function buildFallbackProviders(): readonly ProviderConfig[] {
  const ollamaId = "prov-ollama";
  const lmstudioId = "prov-lmstudio";
  const openaiId = "prov-openai";
  const anthropicId = "prov-anthropic";
  const openrouterId = "provider-openrouter";
  const deepseekId = "provider-deepseek";
  const geminiId = "provider-gemini";
  if (
    !isProviderId(ollamaId) ||
    !isProviderId(lmstudioId) ||
    !isProviderId(openaiId) ||
    !isProviderId(anthropicId) ||
    !isProviderId(openrouterId) ||
    !isProviderId(deepseekId) ||
    !isProviderId(geminiId)
  ) {
    return [];
  }
  return [
    {
      kind: "ollama",
      id: ollamaId,
      name: "Local Ollama",
      baseUrl: "http://localhost:11434",
      model: "qwen2.5:14b",
    },
    {
      kind: "openai-compatible",
      id: lmstudioId,
      name: "Local LM Studio",
      baseUrl: "http://localhost:1234/v1",
      apiKeyRef: "",
      model: "qwen2.5-14b-instruct",
    },
    {
      kind: "openai-compatible",
      id: openaiId,
      name: "Cloud OpenAI",
      baseUrl: "https://api.openai.com/v1",
      apiKeyRef: "key:openai",
      model: "gpt-4.1-mini",
    },
    {
      kind: "anthropic",
      id: anthropicId,
      name: "Cloud Anthropic",
      baseUrl: "https://api.anthropic.com",
      apiKeyRef: "key:anthropic",
      model: "claude-sonnet-4-5",
    },
    {
      kind: "openai-compatible",
      id: openrouterId,
      name: "Cloud · OpenRouter (Claude Sonnet 4)",
      baseUrl: "https://openrouter.ai/api/v1",
      apiKeyRef: "openrouter",
      model: "anthropic/claude-sonnet-4",
    },
    {
      kind: "openai-compatible",
      id: deepseekId,
      name: "Cloud · DeepSeek (deepseek-chat)",
      baseUrl: "https://api.deepseek.com/v1",
      apiKeyRef: "deepseek",
      model: "deepseek-chat",
    },
    {
      kind: "openai-compatible",
      id: geminiId,
      name: "Cloud · Gemini (gemini-2.5-flash)",
      baseUrl: "https://generativelanguage.googleapis.com/v1beta/openai",
      apiKeyRef: "gemini",
      model: "gemini-2.5-flash",
    },
  ];
}

function buildFallbackActions(): readonly CustomAction[] {
  const structureId = "act-english-structure";
  if (!isActionId(structureId)) {
    return [];
  }
  return [
    {
      id: structureId,
      name: "English Structure Analysis",
      systemPrompt: "Analyze English sentence syntax and return StructuredGrammarCard JSON.",
      isBuiltIn: true,
      renderMode: "unified-3layer",
      htmlTemplate: null,
      cssTemplate: null,
    },
  ];
}

function renderCustomHtmlTemplate(
  action: CustomAction,
  sourceText: string,
  rawOutput: string,
  meaning: string,
): string {
  const template =
    action.htmlTemplate ??
    '<section class="custom-output"><h4>{{sourceText}}</h4><div>{{content}}</div></section>';
  return template
    .replaceAll("{{sourceText}}", sourceText)
    .replaceAll("{{meaning}}", meaning)
    .replaceAll("{{content}}", rawOutput);
}

export function App() {
  const [theme, setTheme] = useState<CardTheme>("dark-ink");
  const [sentence, setSentence] = useState("");
  const [providers, setProviders] = useState<readonly ProviderConfig[]>(buildFallbackProviders);
  const [configuredKeyRefs, setConfiguredKeyRefs] = useState<readonly string[]>([]);
  const [actions, setActions] = useState<readonly CustomAction[]>(buildFallbackActions);
  const [history, setHistory] = useState<readonly HistoryEntry[]>([]);
  const [selectedProviderId, setSelectedProviderId] = useState<ProviderId | null>(
    () => buildFallbackProviders()[0]?.id ?? null,
  );
  const [selectedActionId, setSelectedActionId] = useState<ActionId | null>(
    () => buildFallbackActions()[0]?.id ?? null,
  );
  const [drawerOpen, setDrawerOpen] = useState(false);
  const [captureError, setCaptureError] = useState<SelectionErrorEvent | null>(null);

  const selectedProviderRef = useRef(selectedProviderId);
  selectedProviderRef.current = selectedProviderId;
  const selectedActionRef = useRef(selectedActionId);
  selectedActionRef.current = selectedActionId;

  const refreshHistory = useCallback(async () => {
    if (!isTauriRuntime()) {
      return;
    }
    try {
      const entries = await invoke<HistoryEntry[]>("list_history", { limit: 50, offset: 0 });
      setHistory(entries);
    } catch (err) {
      const detail = err instanceof Error ? err.message : String(err);
      setCaptureError({
        code: "history-load-failed",
        message: `无法加载历史记录（Failed to load history: ${detail}）`,
        hotkey: "",
      });
    }
  }, []);

  const handleAnalysisCompleted = useCallback(
    (payload: CompletedAnalysisPayload) => {
      if (!isTauriRuntime()) {
        return;
      }
      void invoke("save_history", { entry: payload })
        .then(() => refreshHistory())
        .catch(() => {});
    },
    [refreshHistory],
  );

  const { state, start, cancel, loadSnapshot } = useAnalysisStream({
    onCompleted: handleAnalysisCompleted,
  });

  useEffect(() => {
    if (!isTauriRuntime()) {
      return;
    }
    let cancelled = false;
    void Promise.all([
      invoke<ProviderConfig[]>("list_providers"),
      invoke<CustomAction[]>("list_actions"),
      invoke<HistoryEntry[]>("list_history", { limit: 50, offset: 0 }),
      invoke<string[]>("list_api_key_refs"),
    ])
      .then(([loadedProviders, loadedActions, loadedHistory, loadedRefs]) => {
        if (cancelled) {
          return;
        }
        setProviders(loadedProviders);
        setActions(loadedActions);
        setHistory(loadedHistory);
        setConfiguredKeyRefs(loadedRefs);
        const firstProvider = loadedProviders[0];
        if (firstProvider) {
          setSelectedProviderId((prev) =>
            prev && loadedProviders.some((p) => p.id === prev) ? prev : firstProvider.id,
          );
        }
        const firstAction = loadedActions[0];
        if (firstAction) {
          setSelectedActionId((prev) =>
            prev && loadedActions.some((a) => a.id === prev) ? prev : firstAction.id,
          );
        }
      })
      .catch(() => {});

    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (!isTauriRuntime()) {
      return;
    }
    let unlistenCaptured: (() => void) | null = null;
    let unlistenError: (() => void) | null = null;
    let active = true;

    void listen<SelectionCapturedEvent>("selection://captured", (event) => {
      const capturedText = event.payload?.text?.trim();
      if (!capturedText) {
        return;
      }
      setCaptureError(null);
      setSentence(capturedText);
      const providerId = selectedProviderRef.current;
      const actionId = selectedActionRef.current;
      if (providerId && actionId) {
        void start({ sourceText: capturedText, providerId, actionId });
      }
    }).then((fn) => {
      if (active) {
        unlistenCaptured = fn;
      } else {
        fn();
      }
    });

    void listen<SelectionErrorEvent>("selection://error", (event) => {
      if (event.payload) {
        setCaptureError(event.payload);
      }
    }).then((fn) => {
      if (active) {
        unlistenError = fn;
      } else {
        fn();
      }
    });

    return () => {
      active = false;
      unlistenCaptured?.();
      unlistenError?.();
    };
  }, [start]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && drawerOpen) {
        setDrawerOpen(false);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [drawerOpen]);

  const activeAction = useMemo(
    () => actions.find((a) => a.id === selectedActionId) ?? actions[0] ?? null,
    [actions, selectedActionId],
  );

  const handleSubmit = (event: FormEvent) => {
    event.preventDefault();
    if (!selectedProviderId || !selectedActionId || sentence.trim().length === 0) {
      return;
    }
    setCaptureError(null);
    void start({
      sourceText: sentence,
      providerId: selectedProviderId,
      actionId: selectedActionId,
    });
  };

  const handleAnalyzeClipboard = useCallback(async () => {
    try {
      const raw = isTauriRuntime()
        ? await invoke<string | null>("read_clipboard_text")
        : await navigator.clipboard.readText();
      const text = raw?.trim() ?? "";
      if (!text) {
        setCaptureError({
          code: "clipboard-empty",
          hotkey: "Clipboard",
          message:
            "当前剪贴板为空（Clipboard is empty）。请先复制英文句子，或直接在上方输入框粘贴。",
        });
        return;
      }
      setCaptureError(null);
      setSentence(text);
      const providerId = selectedProviderRef.current;
      const actionId = selectedActionRef.current;
      if (providerId && actionId) {
        void start({ sourceText: text, providerId, actionId });
      }
    } catch (err) {
      setCaptureError({
        code: "clipboard-read-failed",
        hotkey: "Clipboard",
        message: `读取剪贴板失败（${err instanceof Error ? err.message : String(err)}），请直接粘贴到上方输入框。`,
      });
    }
  }, [start]);

  const handleSaveProvider = async (config: ProviderConfig, apiKey?: string) => {
    if (isTauriRuntime()) {
      await invoke("upsert_provider", { config });
      if (apiKey !== undefined && config.kind !== "ollama" && config.apiKeyRef.trim().length > 0) {
        await invoke("set_api_key", {
          keyRef: config.apiKeyRef.trim(),
          secretValue: apiKey,
        });
      }
      const [loadedProviders, loadedRefs] = await Promise.all([
        invoke<ProviderConfig[]>("list_providers"),
        invoke<string[]>("list_api_key_refs"),
      ]);
      setProviders(loadedProviders);
      setConfiguredKeyRefs(loadedRefs);
    } else {
      setProviders((prev) => {
        const exists = prev.some((p) => p.id === config.id);
        return exists ? prev.map((p) => (p.id === config.id ? config : p)) : [...prev, config];
      });
    }
    setSelectedProviderId(config.id);
  };

  const handleSaveAction = async (action: CustomAction) => {
    if (isTauriRuntime()) {
      await invoke("upsert_action", { action });
      const loadedActions = await invoke<CustomAction[]>("list_actions");
      setActions(loadedActions);
    } else {
      setActions((prev) => {
        const exists = prev.some((a) => a.id === action.id);
        return exists ? prev.map((a) => (a.id === action.id ? action : a)) : [...prev, action];
      });
    }
    setSelectedActionId(action.id);
  };

  const handleSelectHistory = (entry: HistoryEntry) => {
    setSentence(entry.sourceText);
    if (providers.some((p) => p.id === entry.providerId)) {
      setSelectedProviderId(entry.providerId);
    }
    if (actions.some((a) => a.id === entry.actionId)) {
      setSelectedActionId(entry.actionId);
    }
    loadSnapshot({
      sourceText: entry.sourceText,
      rawJson: entry.rawJson,
      latencyMs: entry.latencyMs,
    });
  };

  const handleSpeak = (text: string) => {
    if (typeof window !== "undefined" && "speechSynthesis" in window) {
      window.speechSynthesis.cancel();
      const utterance = new SpeechSynthesisUtterance(text);
      utterance.lang = "en-US";
      window.speechSynthesis.speak(utterance);
    }
  };

  return (
    <div className="app-shell" data-card-theme={theme}>
      <header className="app-topbar">
        <form className="topbar-input-row" onSubmit={handleSubmit}>
          <input
            type="text"
            className="sentence-input"
            value={sentence}
            onChange={(e) => setSentence(e.target.value)}
            placeholder="Enter an English sentence or press Alt+E / Cmd+Shift+E anywhere..."
            aria-label="English sentence to analyze"
          />
          {state.kind === "streaming" ? (
            <button type="button" className="app-btn" onClick={() => void cancel()}>
              Cancel
            </button>
          ) : (
            <button
              type="submit"
              className="app-btn primary"
              disabled={!selectedProviderId || !selectedActionId || sentence.trim().length === 0}
            >
              Analyze
            </button>
          )}
        </form>

        <div className="topbar-controls-row">
          <select
            className="topbar-select"
            aria-label="Provider"
            value={selectedProviderId ?? ""}
            onChange={(e) => {
              if (isProviderId(e.target.value)) {
                setSelectedProviderId(e.target.value);
              }
            }}
          >
            {providers.map((provider) => (
              <option key={provider.id} value={provider.id}>
                {provider.name} ({provider.model})
              </option>
            ))}
          </select>

          <select
            className="topbar-select"
            aria-label="Action"
            value={selectedActionId ?? ""}
            onChange={(e) => {
              if (isActionId(e.target.value)) {
                setSelectedActionId(e.target.value);
              }
            }}
          >
            {actions.map((action) => (
              <option key={action.id} value={action.id}>
                {action.name}
              </option>
            ))}
          </select>

          <span className="topbar-spacer" />

          <button
            type="button"
            className="app-btn"
            onClick={() => setTheme((prev) => (prev === "dark-ink" ? "warm-paper" : "dark-ink"))}
          >
            {theme === "dark-ink" ? "Warm Paper" : "Dark Ink"}
          </button>

          <button type="button" className="app-btn" onClick={() => setDrawerOpen(true)}>
            Settings
          </button>
        </div>
      </header>

      <main className="app-stage">
        {captureError ? (
          <div className="capture-error-banner" role="alert">
            <div className="capture-error-body">
              <div>
                <strong>[{captureError.hotkey}]</strong> {captureError.message}
              </div>
              <div className="capture-error-hint">
                macOS 权限路径：<code>系统设置 → 隐私与安全性 → 辅助功能</code>（授权后可使用{" "}
                <kbd>Alt+E</kbd> 或 <kbd>Cmd+Shift+E</kbd> 全局取词，也可点击右侧按钮直接分析剪贴板或手动粘贴）。
              </div>
            </div>
            <div className="capture-error-actions">
              <button
                type="button"
                className="app-btn primary"
                onClick={() => void handleAnalyzeClipboard()}
              >
                分析当前剪贴板
              </button>
              <button
                type="button"
                className="app-btn"
                onClick={() => setCaptureError(null)}
              >
                关闭
              </button>
            </div>
          </div>
        ) : null}

        {state.kind === "idle" ? (
          <section className="stage-panel empty-state">
            <p>
              Select an English sentence in any desktop app and press <kbd>Alt+E</kbd> or{" "}
              <kbd>Cmd+Shift+E</kbd>, or type a sentence above to stream its syntactic analysis.
            </p>
            <div className="empty-state-actions">
              <button
                type="button"
                className="app-btn"
                onClick={() => void handleAnalyzeClipboard()}
              >
                分析当前剪贴板
              </button>
            </div>
          </section>
        ) : null}

        {state.kind === "streaming" ? (
          <section className="stage-panel" aria-live="polite">
            <div className="stream-status">
              <span>Streaming analysis...</span>
              <span>{state.streamedText.length} chars</span>
            </div>
            {state.hits.length > 0 ? (
              <div className="stream-hits">
                {state.hits.map((hit, idx) => (
                  <span key={`${hit.chapter}-${idx}`} className="book-chip">
                    {hit.chapter} · {hit.section}
                  </span>
                ))}
              </div>
            ) : null}
            {state.streamedText.length > 0 ? (
              <pre className="stream-preview">{state.streamedText}</pre>
            ) : null}
          </section>
        ) : null}

        {state.kind === "error" ? (
          <div className="error-banner" role="alert">
            <span>
              <strong>[{state.code}]</strong> {state.message}
            </span>
            <button
              type="button"
              className="app-btn"
              onClick={() => {
                if (selectedProviderId && selectedActionId) {
                  void start({
                    sourceText: state.sourceText,
                    providerId: selectedProviderId,
                    actionId: selectedActionId,
                  });
                }
              }}
            >
              Retry
            </button>
          </div>
        ) : null}

        {state.kind === "ready" || state.kind === "fallback" ? (
          activeAction?.renderMode === "custom-html" ? (
            <section className="stage-panel">
              <CustomTemplateSandbox
                html={renderCustomHtmlTemplate(
                  activeAction,
                  state.sourceText,
                  state.rawJson,
                  state.card.meaning,
                )}
                css={activeAction.cssTemplate}
                theme={theme}
              />
            </section>
          ) : (
            <UnifiedCard card={state.card} theme={theme} onSpeak={handleSpeak} />
          )
        ) : null}
      </main>

      <SettingsDrawer
        isOpen={drawerOpen}
        theme={theme}
        providers={providers}
        configuredKeyRefs={configuredKeyRefs}
        actions={actions}
        history={history}
        onClose={() => setDrawerOpen(false)}
        onSaveProvider={handleSaveProvider}
        onSaveAction={handleSaveAction}
        onSelectHistory={handleSelectHistory}
        onRefreshHistory={refreshHistory}
      />
    </div>
  );
}
