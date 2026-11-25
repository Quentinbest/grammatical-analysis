// @vitest-environment jsdom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { parseGrammarCard } from "../../domain/parseCard.ts";
import { UnifiedCard } from "../UnifiedCard/index.ts";
import { SIMPLE_CARD, SINGLE_GAP_CARD } from "./fixtures.ts";

describe("UnifiedCard", () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    Reflect.set(globalThis, "IS_REACT_ACT_ENVIRONMENT", true);
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
  });

  afterEach(() => {
    act(() => {
      root.unmount();
    });
    container.remove();
  });

  it("hides fold and gap controls on a simple sentence", () => {
    act(() => {
      root.render(createElement(UnifiedCard, { card: SIMPLE_CARD, theme: "dark-ink" }));
    });

    expect(container.querySelector('[data-action="fold-all"]')).toBeNull();
    expect(container.querySelector('[data-action="toggle-gap"]')).toBeNull();
    expect(container.querySelector('[data-action="toggle-omitted"]')).toBeNull();
    expect(container.querySelector(".unified-card")?.getAttribute("data-card-theme")).toBe("dark-ink");
  });

  it("supports interactive clause folding, token note inspection, and warm-paper theme", () => {
    act(() => {
      root.render(createElement(UnifiedCard, { card: SINGLE_GAP_CARD, theme: "warm-paper", compact: true }));
    });

    const cardEl = container.querySelector(".unified-card");
    expect(cardEl?.getAttribute("data-card-theme")).toBe("warm-paper");
    expect(cardEl?.getAttribute("data-compact")).toBe("true");

    const foldAllBtn = container.querySelector<HTMLButtonElement>('[data-action="fold-all"]');
    expect(foldAllBtn).not.toBeNull();
    expect(container.querySelector(".fold-pill")).toBeNull();

    act(() => {
      foldAllBtn?.click();
    });

    const pill = container.querySelector<HTMLButtonElement>(".fold-pill");
    expect(pill).not.toBeNull();
    expect(pill?.textContent).toContain("关系子句");

    act(() => {
      pill?.click();
    });
    expect(container.querySelector(".fold-pill")).toBeNull();

    const firstToken = container.querySelector<HTMLElement>('[data-tok-id="t1"]');
    act(() => {
      firstToken?.click();
    });
    expect(container.querySelector(".inspector-head")?.textContent).toContain("The book");
    expect(container.querySelector(".inspector-box")?.textContent).toContain("主句主语");
  });

  it("renders FallbackGrammarCard cleanly without crashing", () => {
    const fallbackCard = parseGrammarCard("{bad json", "The book fell.");

    act(() => {
      root.render(createElement(UnifiedCard, { card: fallbackCard, theme: "dark-ink" }));
    });

    const cardEl = container.querySelector('[data-card-kind="fallback"]');
    expect(cardEl).not.toBeNull();
    expect(cardEl?.textContent).toContain("The book fell.");
    expect(cardEl?.textContent).toContain("JSON parse error");
    expect(cardEl?.textContent).toContain("{bad json");
  });
});
