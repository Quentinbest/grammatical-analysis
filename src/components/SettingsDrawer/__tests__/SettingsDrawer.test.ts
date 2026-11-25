// @vitest-environment jsdom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ActionId, CustomAction, ProviderConfig, ProviderId } from "../../../domain/types.ts";

const invokeMock = vi.fn();
const listenMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
  Channel: class MockChannel {
    onmessage: ((event: unknown) => void) | null = null;
  },
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: (...args: unknown[]) => listenMock(...args),
}));

import { App } from "../../../App.tsx";

function setNativeValue(el: HTMLInputElement | HTMLSelectElement, value: string): void {
  const proto =
    el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
  const setter = Object.getOwnPropertyDescriptor(proto, "value")?.set;
  setter?.call(el, value);
  el.dispatchEvent(new Event("input", { bubbles: true }));
  el.dispatchEvent(new Event("change", { bubbles: true }));
}

async function flushMicrotasks(): Promise<void> {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();
  });
}

describe("SettingsDrawer and App regressions", () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    (globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT =
      true;
    (window as unknown as { __TAURI_INTERNALS__: Record<string, unknown> }).__TAURI_INTERNALS__ =
      {};
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
    invokeMock.mockReset();
    listenMock.mockReset();
    listenMock.mockResolvedValue(() => {});
  });

  afterEach(() => {
    act(() => {
      root.unmount();
    });
    container.remove();
    delete (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
  });

  it("syncs SettingsDrawer draft when persisted SQLite providers load asynchronously", async () => {
    const persistedProviders: ProviderConfig[] = [
      {
        kind: "ollama",
        id: "prov-ollama" as ProviderId,
        name: "Local Ollama Custom",
        baseUrl: "http://127.0.0.1:11434",
        model: "llama3.2:latest",
      },
    ];
    const persistedActions: CustomAction[] = [
      {
        id: "act-english-structure" as ActionId,
        name: "English Structure Analysis",
        systemPrompt: "Persisted prompt",
        isBuiltIn: true,
        renderMode: "unified-3layer",
        htmlTemplate: null,
        cssTemplate: null,
      },
    ];
    const upserted: ProviderConfig[] = [];

    invokeMock.mockImplementation(async (cmd: string, args?: Record<string, unknown>) => {
      if (cmd === "list_providers") return persistedProviders;
      if (cmd === "list_actions") return persistedActions;
      if (cmd === "list_history") return [];
      if (cmd === "list_api_key_refs") return [];
      if (cmd === "upsert_provider") {
        upserted.push(args?.config as ProviderConfig);
        return undefined;
      }
      return undefined;
    });

    await act(async () => {
      root.render(createElement(App));
    });
    await flushMicrotasks();

    const settingsBtn = Array.from(container.querySelectorAll("button")).find(
      (b) => b.textContent?.trim() === "Settings",
    );
    await act(async () => {
      settingsBtn?.click();
    });

    const modelInput = container.querySelector(
      'input[placeholder="qwen2.5:14b"]',
    ) as HTMLInputElement | null;
    expect(modelInput?.value).toBe("llama3.2:latest");

    const form = container.querySelector(".drawer-form") as HTMLFormElement | null;
    await act(async () => {
      form?.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    });
    await flushMicrotasks();

    expect(upserted[0]?.model).toBe("llama3.2:latest");
  });

  it("Quick Preset selects preset provider ID instead of overwriting prov-ollama", async () => {
    const persistedProviders: ProviderConfig[] = [
      {
        kind: "ollama",
        id: "prov-ollama" as ProviderId,
        name: "Local Ollama",
        baseUrl: "http://localhost:11434",
        model: "qwen2.5:14b",
      },
      {
        kind: "openai-compatible",
        id: "provider-deepseek" as ProviderId,
        name: "Cloud · DeepSeek (deepseek-chat)",
        baseUrl: "https://api.deepseek.com/v1",
        apiKeyRef: "deepseek",
        model: "deepseek-chat",
      },
    ];
    const upsertedIds: string[] = [];

    invokeMock.mockImplementation(async (cmd: string, args?: Record<string, unknown>) => {
      if (cmd === "list_providers") return persistedProviders;
      if (cmd === "list_actions") return [];
      if (cmd === "list_history") return [];
      if (cmd === "list_api_key_refs") return [];
      if (cmd === "upsert_provider") {
        upsertedIds.push((args?.config as ProviderConfig).id);
        return undefined;
      }
      return undefined;
    });

    await act(async () => {
      root.render(createElement(App));
    });
    await flushMicrotasks();

    const settingsBtn = Array.from(container.querySelectorAll("button")).find(
      (b) => b.textContent?.trim() === "Settings",
    );
    await act(async () => {
      settingsBtn?.click();
    });

    const deepseekBtn = Array.from(container.querySelectorAll(".drawer-presets button")).find(
      (b) => b.textContent?.trim() === "DeepSeek",
    ) as HTMLButtonElement | undefined;
    await act(async () => {
      deepseekBtn?.click();
    });

    const form = container.querySelector(".drawer-form") as HTMLFormElement | null;
    await act(async () => {
      form?.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    });
    await flushMicrotasks();

    expect(upsertedIds[0]).toBe("provider-deepseek");
  });

  it("auto-assigns apiKeyRef and persists secret when saving API key for LM Studio", async () => {
    const persistedProviders: ProviderConfig[] = [
      {
        kind: "openai-compatible",
        id: "prov-lmstudio" as ProviderId,
        name: "Local LM Studio",
        baseUrl: "http://localhost:1234/v1",
        apiKeyRef: "",
        model: "qwen2.5-14b-instruct",
      },
    ];
    const setApiKeyCalls: Array<{ keyRef: string; secretValue: string }> = [];

    invokeMock.mockImplementation(async (cmd: string, args?: Record<string, unknown>) => {
      if (cmd === "list_providers") return persistedProviders;
      if (cmd === "list_actions") return [];
      if (cmd === "list_history") return [];
      if (cmd === "list_api_key_refs") return [];
      if (cmd === "set_api_key") {
        setApiKeyCalls.push({
          keyRef: String(args?.keyRef ?? ""),
          secretValue: String(args?.secretValue ?? ""),
        });
      }
      return undefined;
    });

    await act(async () => {
      root.render(createElement(App));
    });
    await flushMicrotasks();

    const settingsBtn = Array.from(container.querySelectorAll("button")).find(
      (b) => b.textContent?.trim() === "Settings",
    );
    await act(async () => {
      settingsBtn?.click();
    });

    const secretInput = container.querySelector('input[type="password"]') as HTMLInputElement | null;
    expect(secretInput).not.toBeNull();
    await act(async () => {
      if (secretInput) setNativeValue(secretInput, "sk-local-secret");
    });

    const form = container.querySelector(".drawer-form") as HTMLFormElement | null;
    await act(async () => {
      form?.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    });
    await flushMicrotasks();

    expect(setApiKeyCalls).toEqual([
      { keyRef: "key:prov-lmstudio", secretValue: "sk-local-secret" },
    ]);
  });

  it("preserves $', {{content}}, and <T> in custom HTML templates and keeps result bound to its action", async () => {
    const persistedProviders: ProviderConfig[] = [
      {
        kind: "ollama",
        id: "prov-ollama" as ProviderId,
        name: "Local Ollama",
        baseUrl: "http://localhost:11434",
        model: "qwen2.5:14b",
      },
    ];
    const persistedActions: CustomAction[] = [
      {
        id: "act-compact-html" as ActionId,
        name: "Compact HTML Card",
        systemPrompt: "Output HTML",
        isBuiltIn: true,
        renderMode: "custom-html",
        htmlTemplate:
          '<section><header class="hdr">{{sourceText}}</header><div class="body">{{content}}</div></section>',
        cssTemplate: null,
      },
      {
        id: "act-english-structure" as ActionId,
        name: "English Structure Analysis",
        systemPrompt: "Output JSON",
        isBuiltIn: true,
        renderMode: "unified-3layer",
        htmlTemplate: null,
        cssTemplate: null,
      },
    ];

    const trickySentence = "Cost is $' and placeholder {{content}} with <T> generic.";

    invokeMock.mockImplementation(async (cmd: string, args?: Record<string, unknown>) => {
      if (cmd === "list_providers") return persistedProviders;
      if (cmd === "list_actions") return persistedActions;
      if (cmd === "list_history") return [];
      if (cmd === "list_api_key_refs") return [];
      if (cmd === "run_analysis") {
        const channel = args?.channel as { onmessage?: (e: unknown) => void } | undefined;
        channel?.onmessage?.({
          kind: "completed",
          rawJson: "<p>INJECTED_LLM_OUTPUT</p>",
          latencyMs: 42,
        });
      }
      return undefined;
    });

    await act(async () => {
      root.render(createElement(App));
    });
    await flushMicrotasks();

    const actionSelect = container.querySelector(
      'select[aria-label="Action"]',
    ) as HTMLSelectElement | null;
    await act(async () => {
      if (actionSelect) setNativeValue(actionSelect, "act-compact-html");
    });

    const sentenceInput = container.querySelector(".sentence-input") as HTMLInputElement | null;
    await act(async () => {
      if (sentenceInput) setNativeValue(sentenceInput, trickySentence);
    });

    const topForm = container.querySelector(".topbar-input-row") as HTMLFormElement | null;
    await act(async () => {
      topForm?.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    });
    await flushMicrotasks();

    const sandboxHost = container.querySelector('[data-sandbox="custom-template"]');
    const headerText = sandboxHost?.shadowRoot?.querySelector(".hdr")?.textContent ?? "";
    expect(headerText).toBe(trickySentence);

    await act(async () => {
      if (actionSelect) setNativeValue(actionSelect, "act-english-structure");
    });

    expect(container.querySelector('[data-card-kind="fallback"]')).toBeNull();
    expect(container.querySelector('[data-sandbox="custom-template"]')).not.toBeNull();
  });
});
