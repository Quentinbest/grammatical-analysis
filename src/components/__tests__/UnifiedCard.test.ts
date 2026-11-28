// @vitest-environment jsdom
import { act, createElement, useLayoutEffect } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { parseGrammarCard } from "../../domain/parseCard.ts";
import type { GrammarCard } from "../../domain/types.ts";
import { UnifiedCard } from "../UnifiedCard/index.ts";
import { COMPLEX_34_WORD_CARD, SIMPLE_CARD, SINGLE_GAP_CARD } from "./fixtures.ts";

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

  it("does not duplicate connector text when a clause has both clause.connector and a leading InlineConnector", () => {
    const duplicatedCard = parseGrammarCard(
      {
        meaning: "虽然下雨，我们还是出门了。",
        parts: [
          {
            kind: "clause",
            id: "c1",
            badge: "让步子句",
            connector: "Although",
            items: [
              { kind: "connector", id: "t1", text: "Although", badge: "连接词", note: "让步连词。" },
              { kind: "sub-token", id: "t2", text: "it rained", role: "pred", note: "从句。" },
            ],
          },
          { kind: "punct", text: "," },
          { kind: "token", id: "t3", text: "we left", role: "subj", note: "主句。" },
          { kind: "punct", text: "." },
        ],
        recombination: "Although it rained, we left.",
        caveatNote: null,
        bookRef: null,
      },
      "Although it rained, we left.",
    );

    act(() => {
      root.render(createElement(UnifiedCard, { card: duplicatedCard, theme: "dark-ink" }));
    });

    const ovals = container.querySelectorAll(".conn-oval");
    expect(ovals.length).toBe(1);
    expect(container.querySelector(".sent-body")?.textContent).not.toContain("Although Although");

    act(() => {
      root.render(createElement(UnifiedCard, { card: COMPLEX_34_WORD_CARD, theme: "dark-ink" }));
    });

    const complexOvals = container.querySelectorAll(".conn-oval");
    expect(complexOvals.length).toBe(1);
    expect(container.querySelector(".sent-body")?.textContent).not.toContain("Although Although");
  });

  it("clears inspector selection when hiding a selected gap or folding a clause containing the selected token", () => {
    act(() => {
      root.render(createElement(UnifiedCard, { card: SINGLE_GAP_CARD, theme: "dark-ink" }));
    });

    // Select gap slot, then hide gap layer
    const gapEl = container.querySelector<HTMLElement>('[data-tok-id="g1"]');
    act(() => {
      gapEl?.click();
    });
    expect(container.querySelector(".inspector-head")?.textContent).toContain("空位 □1");

    const toggleGapBtn = container.querySelector<HTMLButtonElement>('[data-action="toggle-gap"]');
    act(() => {
      toggleGapBtn?.click();
    });
    expect(container.querySelector(".inspector-head")?.textContent).toContain("结构要点");

    // Toggling gap layer back on must not resurrect stale selection
    act(() => {
      toggleGapBtn?.click();
    });
    expect(container.querySelector(".inspector-head")?.textContent).toContain("结构要点");

    // Select sub-token inside c1, then fold c1 individually
    const subToken = container.querySelector<HTMLElement>('[data-tok-id="t2"]');
    act(() => {
      subToken?.click();
    });
    expect(container.querySelector(".inspector-head")?.textContent).toContain("Maya");

    const clauseBadge = container.querySelector<HTMLButtonElement>('[data-fold-clause="c1"]');
    act(() => {
      clauseBadge?.click();
    });
    expect(container.querySelector(".inspector-head")?.textContent).toContain("结构要点");

    // Unfold c1, select sub-token again, then click fold-all
    act(() => {
      container.querySelector<HTMLButtonElement>('[data-fold-clause="c1"]')?.click();
    });
    act(() => {
      container.querySelector<HTMLElement>('[data-tok-id="t2"]')?.click();
    });
    expect(container.querySelector(".inspector-head")?.textContent).toContain("Maya");

    act(() => {
      container.querySelector<HTMLButtonElement>('[data-action="fold-all"]')?.click();
    });
    expect(container.querySelector(".inspector-head")?.textContent).toContain("结构要点");

    // Main-clause token remains selected when folding clauses
    act(() => {
      container.querySelector<HTMLElement>('[data-tok-id="t1"]')?.click();
    });
    expect(container.querySelector(".inspector-head")?.textContent).toContain("The book");
    act(() => {
      container.querySelector<HTMLButtonElement>('[data-action="fold-all"]')?.click();
    });
    expect(container.querySelector(".inspector-head")?.textContent).toContain("The book");
  });

  it("resets fold and selection state synchronously on the first committed frame when card prop changes", () => {
    const commitSnapshots: Array<{ foldPills: number; inspectorHead: string }> = [];

    function Harness({ card }: { readonly card: GrammarCard }) {
      useLayoutEffect(() => {
        commitSnapshots.push({
          foldPills: container.querySelectorAll(".fold-pill").length,
          inspectorHead: container.querySelector(".inspector-head")?.textContent ?? "",
        });
      }, [card]);

      return createElement(UnifiedCard, { card, theme: "dark-ink" });
    }

    act(() => {
      root.render(createElement(Harness, { card: SINGLE_GAP_CARD }));
    });

    // Fold c1 and select t1 on the first card
    act(() => {
      container.querySelector<HTMLButtonElement>('[data-action="fold-all"]')?.click();
      container.querySelector<HTMLElement>('[data-tok-id="t1"]')?.click();
    });
    expect(container.querySelectorAll(".fold-pill").length).toBe(1);
    expect(container.querySelector(".inspector-head")?.textContent).toContain("The book");

    commitSnapshots.length = 0;

    // Switch to a different card; the very first layout commit must already be clean
    act(() => {
      root.render(createElement(Harness, { card: COMPLEX_34_WORD_CARD }));
    });

    expect(commitSnapshots.length).toBeGreaterThan(0);
    expect(commitSnapshots[0]?.foldPills).toBe(0);
    expect(commitSnapshots[0]?.inspectorHead).toContain("结构要点");
    expect(commitSnapshots[0]?.inspectorHead).not.toContain("The book");
  });

  it("does not insert a stray space before punctuation tokens", () => {
    act(() => {
      root.render(createElement(UnifiedCard, { card: SIMPLE_CARD, theme: "dark-ink" }));
    });

    expect(container.querySelector(".sent-body")?.textContent).toBe(
      "The students finished the assignment early.",
    );

    act(() => {
      root.render(createElement(UnifiedCard, { card: COMPLEX_34_WORD_CARD, theme: "dark-ink" }));
    });

    const complexText = container.querySelector(".sent-body")?.textContent ?? "";
    expect(complexText).not.toContain(" ,");
    expect(complexText).not.toContain(" .");
    expect(complexText.endsWith(".")).toBe(true);
  });

  it("applies .selected and .conn-interactive to inline connectors, and keeps clause.connector non-interactive", () => {
    act(() => {
      root.render(createElement(UnifiedCard, { card: COMPLEX_34_WORD_CARD, theme: "dark-ink" }));
    });

    const inlineConn = container.querySelector<HTMLElement>('[data-tok-id="t1"]');
    expect(inlineConn).not.toBeNull();
    expect(inlineConn?.classList.contains("conn-interactive")).toBe(true);
    expect(inlineConn?.classList.contains("selected")).toBe(false);

    act(() => {
      inlineConn?.click();
    });
    expect(inlineConn?.classList.contains("selected")).toBe(true);
    expect(container.querySelector(".inspector-head")?.textContent).toContain("Although");

    act(() => {
      inlineConn?.click();
    });
    expect(inlineConn?.classList.contains("selected")).toBe(false);

    const staticConnectorCard = parseGrammarCard(
      {
        meaning: "因为他累了，他早睡了。",
        parts: [
          {
            kind: "clause",
            id: "c1",
            badge: "原因子句",
            connector: "Because",
            items: [{ kind: "sub-token", id: "t1", text: "he was tired", role: "pred", note: "原因。" }],
          },
          { kind: "punct", text: "," },
          { kind: "token", id: "t2", text: "he slept early", role: "subj", note: "主句。" },
          { kind: "punct", text: "." },
        ],
        recombination: "Because he was tired, he slept early.",
        caveatNote: null,
        bookRef: null,
      },
      "Because he was tired, he slept early.",
    );

    act(() => {
      root.render(createElement(UnifiedCard, { card: staticConnectorCard, theme: "dark-ink" }));
    });

    const staticOval = container.querySelector<HTMLElement>(".conn-oval");
    expect(staticOval).not.toBeNull();
    expect(staticOval?.textContent).toContain("Because");
    expect(staticOval?.classList.contains("conn-interactive")).toBe(false);
  });

  it("renders u-comp complement role, nested clauses, [断句] toggle, and calm usage-note-strip", () => {
    const nestedCompCard = parseGrammarCard(
      {
        meaning: "我不认为它是唯一能解决这个问题的团队，而且那只是开始。",
        parts: [
          { kind: "token", id: "t1", text: "I", role: "subj", note: "主语。" },
          { kind: "token", id: "t2", text: "don't think", role: "pred", subLabel: "否定转移", note: "谓语。" },
          {
            kind: "clause",
            id: "c1",
            badge: "宾语从句",
            omittedHint: "(that)",
            items: [
              { kind: "sub-token", id: "st1", text: "it", role: "subj", note: "从句主语。" },
              { kind: "sub-token", id: "st2", text: "is", role: "pred", note: "系动词。" },
              {
                kind: "sub-token",
                id: "st3",
                text: "the only team",
                role: "comp",
                antecedentIndex: 1,
                note: "主语补语。",
              },
              {
                kind: "clause",
                id: "c2",
                badge: "定语从句",
                connector: "that",
                items: [
                  { kind: "connector", id: "cn1", text: "that", badge: "关系代词", note: "引导定语从句。" },
                  { kind: "gap", id: "g1", index: 1, note: "主语空位。" },
                  { kind: "sub-token", id: "st4", text: "can solve", role: "pred", note: "定语从句谓语。" },
                  { kind: "sub-token", id: "st5", text: "it", role: "obj", note: "宾语。" },
                ],
              },
            ],
          },
          { kind: "punct", text: "," },
          { kind: "connector", id: "cn-and", text: "and", badge: "并列连词", note: "并列分句连接词。" },
          { kind: "token", id: "t3", text: "that", role: "subj", note: "指示代词主语。" },
          { kind: "token", id: "t4", text: "is", role: "pred", note: "系动词。" },
          { kind: "token", id: "t5", text: "the start", role: "comp", note: "表语补语。" },
          { kind: "punct", text: "." },
        ],
        recombination: "主干与嵌套从句。",
        caveatNote: "think 否定转移至宾语从句语义。",
        bookRef: { chapter: "ch17", section: "名词子句" },
      },
      "I don't think it is the only team that can solve it, and that is the start.",
    );

    act(() => {
      root.render(createElement(UnifiedCard, { card: nestedCompCard, theme: "dark-ink" }));
    });

    // Legend includes 补语 and token renders .u-comp
    expect(container.querySelector(".legend")?.textContent).toContain("补语");
    expect(container.querySelector('[data-tok-id="t5"]')?.classList.contains("u-comp")).toBe(true);
    expect(container.querySelector('[data-tok-id="st3"]')?.classList.contains("u-comp")).toBe(true);

    // Nested clause renders .nested-clause
    const nestedClauseEl = container.querySelector('[data-clause-id="c2"]');
    expect(nestedClauseEl?.classList.contains("nested-clause")).toBe(true);

    // Calm usage-note-strip is used instead of red caveat-strip on StructuredCardView
    expect(container.querySelector(".usage-note-strip")?.textContent).toContain("否定转移");
    expect(container.querySelector(".caveat-strip")).toBeNull();

    // Selecting nested token then folding outer clause clears inspector selection
    act(() => {
      container.querySelector<HTMLElement>('[data-tok-id="st4"]')?.click();
    });
    expect(container.querySelector(".inspector-head")?.textContent).toContain("can solve");

    act(() => {
      container.querySelector<HTMLButtonElement>('[data-fold-clause="c1"]')?.click();
    });
    expect(container.querySelector(".inspector-head")?.textContent).toContain("结构要点");

    // [断句] toggle adds data-linebreak="true" and renders .sent-linebreak <br> elements
    const lineBreakBtn = container.querySelector<HTMLButtonElement>('[data-action="toggle-linebreak"]');
    expect(lineBreakBtn?.textContent).toBe("断句");
    expect(container.querySelectorAll(".sent-linebreak").length).toBe(0);

    act(() => {
      lineBreakBtn?.click();
    });
    expect(container.querySelector(".sent-body")?.getAttribute("data-linebreak")).toBe("true");
    expect(container.querySelectorAll(".sent-linebreak").length).toBe(2);
  });
});
