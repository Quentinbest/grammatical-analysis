// @vitest-environment jsdom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

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

import { App } from "../../App.tsx";

function findButtonByText(container: HTMLElement, label: string): HTMLButtonElement | undefined {
  const buttons = Array.from(container.querySelectorAll("button"));
  return buttons.find((btn) => btn.textContent?.trim() === label);
}

describe("App root and window theme synchronization", () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    Reflect.set(globalThis, "IS_REACT_ACT_ENVIRONMENT", true);
    delete document.documentElement.dataset.cardTheme;
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
    delete document.documentElement.dataset.cardTheme;
    Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  });

  it("syncs document.documentElement.dataset.cardTheme on mount and when toggling Warm Paper", () => {
    act(() => {
      root.render(createElement(App));
    });

    expect(document.documentElement.dataset.cardTheme).toBe("dark-ink");

    const warmPaperButton = findButtonByText(container, "Warm Paper");
    expect(warmPaperButton).toBeDefined();

    act(() => {
      warmPaperButton?.click();
    });

    expect(document.documentElement.dataset.cardTheme).toBe("warm-paper");

    const darkInkButton = findButtonByText(container, "Dark Ink");
    expect(darkInkButton).toBeDefined();

    act(() => {
      darkInkButton?.click();
    });

    expect(document.documentElement.dataset.cardTheme).toBe("dark-ink");
  });

  it("syncs native Tauri window theme and background color without throwing on rejected IPC", async () => {
    Reflect.set(window, "__TAURI_INTERNALS__", {
      invoke: (cmd: string, args?: Record<string, unknown>) => invokeMock(cmd, args),
      metadata: {
        currentWindow: { label: "main" },
      },
    });

    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "list_providers") return [];
      if (cmd === "list_actions") return [];
      if (cmd === "list_history") return [];
      if (cmd === "list_api_key_refs") return [];
      if (cmd === "plugin:window|set_theme" || cmd === "plugin:window|set_background_color") {
        return undefined;
      }
      return undefined;
    });

    await act(async () => {
      root.render(createElement(App));
    });

    expect(invokeMock).toHaveBeenCalledWith("plugin:window|set_theme", {
      label: "main",
      value: "dark",
    });
    expect(invokeMock).toHaveBeenCalledWith("plugin:window|set_background_color", {
      label: "main",
      value: "#141519",
    });

    invokeMock.mockRejectedValueOnce(new Error("permission denied"));
    invokeMock.mockRejectedValueOnce(new Error("permission denied"));

    const warmPaperButton = findButtonByText(container, "Warm Paper");
    await act(async () => {
      warmPaperButton?.click();
    });

    expect(document.documentElement.dataset.cardTheme).toBe("warm-paper");
    expect(invokeMock).toHaveBeenCalledWith("plugin:window|set_theme", {
      label: "main",
      value: "light",
    });
    expect(invokeMock).toHaveBeenCalledWith("plugin:window|set_background_color", {
      label: "main",
      value: "#f3eee5",
    });
  });
});
