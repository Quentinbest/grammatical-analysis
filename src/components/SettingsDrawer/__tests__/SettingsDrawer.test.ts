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
import { SelectionPill } from "../../SelectionPill/index.ts";

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

  it("supports 2026 vendor/model selection in footer ModelPicker, SettingsDrawer model chips, and Background Selection Assistant switch", async () => {
    const persistedProviders: ProviderConfig[] = [
      {
        kind: "openai-compatible",
        id: "prov-openai" as ProviderId,
        name: "OpenAI · GPT-6.2 Sol",
        baseUrl: "https://api.openai.com/v1",
        apiKeyRef: "key:openai",
        model: "gpt-6.2-sol",
      },
      {
        kind: "anthropic",
        id: "prov-anthropic-haiku-55" as ProviderId,
        name: "Anthropic · Claude Haiku 5.5",
        baseUrl: "https://api.anthropic.com",
        apiKeyRef: "key:anthropic",
        model: "claude-haiku-5-5",
      },
      {
        kind: "openai-compatible",
        id: "prov-gemini-4-hydrogen" as ProviderId,
        name: "Google · Gemini 4 Hydrogen",
        baseUrl: "https://generativelanguage.googleapis.com/v1beta/openai",
        apiKeyRef: "gemini",
        model: "gemini-4-hydrogen",
      },
    ];
    const persistedActions: CustomAction[] = [
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
    const analyzedProviders: string[] = [];
    const upsertedProviders: ProviderConfig[] = [];
    const setApiKeyCalls: Array<{ keyRef: string; secretValue: string }> = [];
    const popupToggleCalls: boolean[] = [];
    let assistantStatusHandler: ((event: { payload: unknown }) => void) | null = null;

    listenMock.mockImplementation(async (eventName: string, cb: (event: { payload: unknown }) => void) => {
      if (eventName === "assistant://status") {
        assistantStatusHandler = cb;
      }
      return () => {};
    });

    invokeMock.mockImplementation(async (cmd: string, args?: Record<string, unknown>) => {
      if (cmd === "list_providers") return persistedProviders;
      if (cmd === "list_actions") return persistedActions;
      if (cmd === "list_history") return [];
      if (cmd === "list_api_key_refs") return ["key:anthropic"];
      if (cmd === "get_selection_assistant_status") {
        return { popupEnabled: true, accessibilityTrusted: false, residentInTray: true };
      }
      if (cmd === "set_selection_popup_enabled") {
        popupToggleCalls.push(Boolean(args?.enabled));
        return { popupEnabled: Boolean(args?.enabled), accessibilityTrusted: false, residentInTray: true };
      }
      if (cmd === "upsert_provider") {
        upsertedProviders.push(args?.config as ProviderConfig);
        return undefined;
      }
      if (cmd === "set_api_key") {
        setApiKeyCalls.push({
          keyRef: String(args?.keyRef ?? ""),
          secretValue: String(args?.secretValue ?? ""),
        });
        return undefined;
      }
      if (cmd === "run_analysis") {
        const req = args?.request as { providerId?: string } | undefined;
        analyzedProviders.push(String(req?.providerId ?? ""));
      }
      return undefined;
    });

    await act(async () => {
      root.render(createElement(App));
    });
    await flushMicrotasks();

    // Footer ModelPicker displays short label + vendor SVG icon
    const pickerTrigger = container.querySelector<HTMLButtonElement>(
      '.app-footer-bar button[aria-label="Provider"]',
    );
    expect(pickerTrigger).not.toBeNull();
    expect(pickerTrigger?.textContent).toContain("GPT-6.2 Sol");
    expect(pickerTrigger?.querySelector('[data-vendor="openai"]')).not.toBeNull();

    await act(async () => {
      pickerTrigger?.click();
    });

    const haikuOption = container.querySelector<HTMLButtonElement>(
      '[data-provider-id="prov-anthropic-haiku-55"]',
    );
    expect(haikuOption?.textContent).toContain("Claude Haiku 5.5");
    await act(async () => {
      haikuOption?.click();
    });

    expect(pickerTrigger?.textContent).toContain("Claude Haiku 5.5");
    expect(pickerTrigger?.querySelector('[data-vendor="anthropic"]')).not.toBeNull();

    const sentenceInput = container.querySelector<HTMLInputElement>(".sentence-input");
    await act(async () => {
      if (sentenceInput) setNativeValue(sentenceInput, "Syntax matters.");
    });
    const topForm = container.querySelector<HTMLFormElement>(".topbar-input-row");
    await act(async () => {
      topForm?.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    });
    await flushMicrotasks();
    expect(analyzedProviders).toEqual(["prov-anthropic-haiku-55"]);

    // Open SettingsDrawer, test model chips & Background Selection Assistant toggle
    const settingsBtn = Array.from(
      container.querySelectorAll<HTMLButtonElement>(".app-footer-bar button"),
    ).find((b) => b.textContent?.trim() === "Settings");
    await act(async () => {
      settingsBtn?.click();
    });
    await flushMicrotasks();

    const assistantSwitch = container.querySelector<HTMLButtonElement>(
      'button[role="switch"][aria-label="Background Selection Assistant"]',
    );
    expect(assistantSwitch?.getAttribute("aria-checked")).toBe("true");
    await act(async () => {
      assistantSwitch?.click();
    });
    await flushMicrotasks();
    expect(popupToggleCalls).toEqual([false]);
    expect(assistantSwitch?.getAttribute("aria-checked")).toBe("false");

    await act(async () => {
      assistantStatusHandler?.({
        payload: { popupEnabled: true, accessibilityTrusted: true, residentInTray: true },
      });
    });
    expect(assistantSwitch?.getAttribute("aria-checked")).toBe("true");

    const geminiPresetBtn = Array.from(
      container.querySelectorAll<HTMLButtonElement>(".drawer-presets button"),
    ).find((b) => b.textContent?.trim() === "Gemini");
    await act(async () => {
      geminiPresetBtn?.click();
    });

    const secretInput = container.querySelector<HTMLInputElement>('input[type="password"]');
    await act(async () => {
      if (secretInput) setNativeValue(secretInput, "sk-gemini-shared-secret");
    });

    const hydrogenChip = container.querySelector<HTMLButtonElement>(
      '[data-model-preset="prov-gemini-4-hydrogen"]',
    );
    expect(hydrogenChip?.textContent).toBe("Gemini 4 Hydrogen");
    await act(async () => {
      hydrogenChip?.click();
    });
    expect(secretInput?.value).toBe("sk-gemini-shared-secret");

    const drawerForm = container.querySelector<HTMLFormElement>(".drawer-form");
    await act(async () => {
      drawerForm?.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    });
    await flushMicrotasks();

    expect(upsertedProviders[0]).toMatchObject({
      id: "prov-gemini-4-hydrogen",
      model: "gemini-4-hydrogen",
      apiKeyRef: "gemini",
    });
    expect(setApiKeyCalls).toEqual([
      { keyRef: "gemini", secretValue: "sk-gemini-shared-secret" },
    ]);
  });

  it("renders SelectionPill and dispatches confirm_selection_pill and dismiss_selection_pill", async () => {
    const invokedCmds: string[] = [];
    invokeMock.mockImplementation(async (cmd: string) => {
      invokedCmds.push(cmd);
      return undefined;
    });

    await act(async () => {
      root.render(createElement(SelectionPill));
    });
    await flushMicrotasks();

    const confirmBtn = container.querySelector<HTMLButtonElement>(".selection-pill-action");
    expect(confirmBtn?.textContent).toBe("句型分析");
    await act(async () => {
      confirmBtn?.click();
    });

    const dismissBtn = container.querySelector<HTMLButtonElement>(".selection-pill-dismiss");
    await act(async () => {
      dismissBtn?.click();
    });

    expect(invokedCmds).toEqual(["confirm_selection_pill", "dismiss_selection_pill"]);
  });
});
