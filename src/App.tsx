import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { type Window, getCurrentWindow } from "@tauri-apps/api/window";
import { type FormEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { CustomTemplateSandbox } from "./components/CustomTemplateSandbox/index.ts";
import { ModelPicker } from "./components/ModelPicker/index.ts";
import { type HistoryEntry, SettingsDrawer } from "./components/SettingsDrawer/index.ts";
import { UnifiedCard } from "./components/UnifiedCard/index.ts";
import { buildDefaultProviders } from "./domain/modelCatalog.ts";
import {
  type ActionId,
  type CardTheme,
  type CustomAction,
  type ProviderConfig,
  type ProviderId,
  isActionId,
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

function currentTauriWindow(): Window | null {
  if (!isTauriRuntime()) {
    return null;
  }
  try {
    return getCurrentWindow();
  } catch {
    return null;
  }
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

function escapeHtml(text: string): string {
  return text
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
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
  const values: Record<string, string> = {
    "{{sourceText}}": escapeHtml(sourceText),
    "{{meaning}}": escapeHtml(meaning),
    "{{content}}": rawOutput,
  };
  return template.replace(
    /\{\{(sourceText|meaning|content)\}\}/g,
    (match) => values[match] ?? match,
  );
}

export function App() {
  const [theme, setTheme] = useState<CardTheme>("dark-ink");
  const [sentence, setSentence] = useState("");
  const [providers, setProviders] = useState<readonly ProviderConfig[]>(buildDefaultProviders);
  const [configuredKeyRefs, setConfiguredKeyRefs] = useState<readonly string[]>([]);
  const [actions, setActions] = useState<readonly CustomAction[]>(buildFallbackActions);
  const [history, setHistory] = useState<readonly HistoryEntry[]>([]);
  const [selectedProviderId, setSelectedProviderId] = useState<ProviderId | null>(
    () => buildDefaultProviders()[0]?.id ?? null,
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
    document.documentElement.dataset.cardTheme = theme;
    const appWindow = currentTauriWindow();
    if (!appWindow) {
      return;
    }
    const nativeTheme = theme === "dark-ink" ? "dark" : "light";
    const backgroundColor = theme === "dark-ink" ? "#141519" : "#f3eee5";
    void appWindow.setTheme(nativeTheme).catch(() => {});
    void appWindow.setBackgroundColor(backgroundColor).catch(() => {});
  }, [theme]);

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
      actionId: entry.actionId,
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
            placeholder="Paste an English sentence or select text in any app..."
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
      </header>

      <main className="app-stage">
        {captureError ? (
          <div className="capture-error-banner" role="alert">
            <div className="capture-error-body">
              <div>
                <strong>[{captureError.hotkey}]</strong> {captureError.message}
              </div>
              <div className="capture-error-hint">
                macOS 权限路径：<code>系统设置 → 隐私与安全性 → 辅助功能</code>（授权后可使用鼠标划词浮窗或{" "}
                <kbd>Alt+E</kbd> / <kbd>Cmd+Shift+E</kbd> 取词，也可直接分析剪贴板）。
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
              Select an English sentence in any app and click <strong>句型分析</strong>, press{" "}
              <kbd>Alt+E</kbd> / <kbd>Cmd+Shift+E</kbd>, or enter a sentence above.
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
              <span>
                {state.streamedText.length === 0
                  ? "Waiting for model response..."
                  : "Streaming analysis..."}
              </span>
              {state.streamedText.length > 0 ? (
                <span>{state.streamedText.length} chars</span>
              ) : null}
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

        {state.kind === "ready" || state.kind === "fallback"
          ? (() => {
              const resultAction =
                (state.actionId
                  ? actions.find((a) => a.id === state.actionId)
                  : null) ?? activeAction;
              return resultAction?.renderMode === "custom-html" ? (
                <section className="stage-panel">
                  <CustomTemplateSandbox
                    html={renderCustomHtmlTemplate(
                      resultAction,
                      state.sourceText,
                      state.rawJson,
                      state.card.meaning,
                    )}
                    css={resultAction.cssTemplate}
                    theme={theme}
                  />
                </section>
              ) : (
                <UnifiedCard card={state.card} theme={theme} onSpeak={handleSpeak} />
              );
            })()
          : null}
      </main>

      <footer className="app-footer-bar">
        <ModelPicker
          providers={providers}
          selectedProviderId={selectedProviderId}
          configuredKeyRefs={configuredKeyRefs}
          onSelectProvider={setSelectedProviderId}
        />

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

        <span className="footer-spacer" />

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
      </footer>

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
