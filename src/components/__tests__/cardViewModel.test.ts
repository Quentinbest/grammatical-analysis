import { describe, expect, it } from "vitest";
import {
  areAllClausesFolded,
  createEmptyFoldState,
  findTokenNote,
  hasFoldableClauses,
  hasGapSlots,
  hasOmittedHints,
  isTokenVisible,
  setAllClausesFolded,
  shouldDrawGapArc,
  toggleClauseFold,
} from "../../domain/cardViewModel.ts";
import {
  COMPLEX_34_WORD_CARD,
  SIMPLE_CARD,
  SINGLE_GAP_CARD,
  toClauseId,
  toTokenId,
} from "./fixtures.ts";

describe("cardViewModel", () => {
  it("detects foldable clauses, gap slots, and omitted hints accurately", () => {
    expect(hasFoldableClauses(SIMPLE_CARD)).toBe(false);
    expect(hasGapSlots(SIMPLE_CARD)).toBe(false);
    expect(hasOmittedHints(SIMPLE_CARD)).toBe(false);

    expect(hasFoldableClauses(SINGLE_GAP_CARD)).toBe(true);
    expect(hasGapSlots(SINGLE_GAP_CARD)).toBe(true);
    expect(hasOmittedHints(SINGLE_GAP_CARD)).toBe(true);

    expect(hasFoldableClauses(COMPLEX_34_WORD_CARD)).toBe(true);
    expect(hasGapSlots(COMPLEX_34_WORD_CARD)).toBe(true);
    expect(hasOmittedHints(COMPLEX_34_WORD_CARD)).toBe(true);
  });

  it("toggles individual clauses and checks all-folded state immutably", () => {
    const empty = createEmptyFoldState();
    expect(areAllClausesFolded(SIMPLE_CARD, empty)).toBe(false);
    expect(areAllClausesFolded(COMPLEX_34_WORD_CARD, empty)).toBe(false);

    const c1 = toClauseId("c1");
    const c2 = toClauseId("c2");

    const foldC1 = toggleClauseFold(empty, c1);
    expect(empty.size).toBe(0);
    expect(foldC1.has(c1)).toBe(true);
    expect(areAllClausesFolded(COMPLEX_34_WORD_CARD, foldC1)).toBe(false);

    const foldAll = setAllClausesFolded(COMPLEX_34_WORD_CARD, true);
    expect(areAllClausesFolded(COMPLEX_34_WORD_CARD, foldAll)).toBe(true);
    expect(foldAll.size).toBe(3);

    const unfoldC2 = toggleClauseFold(foldAll, c2);
    expect(unfoldC2.has(c2)).toBe(false);
    expect(areAllClausesFolded(COMPLEX_34_WORD_CARD, unfoldC2)).toBe(false);

    const unfoldAll = setAllClausesFolded(COMPLEX_34_WORD_CARD, false);
    expect(unfoldAll.size).toBe(0);
    expect(areAllClausesFolded(COMPLEX_34_WORD_CARD, unfoldAll)).toBe(false);
  });

  it("draws gap arcs on same line by default and cross-line only when selected", () => {
    expect(shouldDrawGapArc(true, false)).toBe(true);
    expect(shouldDrawGapArc(true, true)).toBe(true);
    expect(shouldDrawGapArc(false, false)).toBe(false);
    expect(shouldDrawGapArc(false, true)).toBe(true);
  });

  it("finds notes for main tokens, sub-tokens, gap slots, and inline connectors", () => {
    expect(findTokenNote(COMPLEX_34_WORD_CARD, null)).toBeNull();

    expect(findTokenNote(COMPLEX_34_WORD_CARD, toTokenId("t5"))).toEqual({
      title: "the novel",
      note: "全句主语，先行词 1。",
    });
    expect(findTokenNote(COMPLEX_34_WORD_CARD, toTokenId("t6"))).toEqual({
      title: "Elena",
      note: "子句主语。",
    });
    expect(findTokenNote(COMPLEX_34_WORD_CARD, toTokenId("g1"))).toEqual({
      title: "空位 □1",
      note: "省略关系代词指代 the novel。",
    });
    expect(findTokenNote(COMPLEX_34_WORD_CARD, toTokenId("t1"))).toEqual({
      title: "Although",
      note: "引出省略主语和 be 动词的让步分句。",
    });
    expect(findTokenNote(COMPLEX_34_WORD_CARD, toTokenId("missing"))).toBeNull();
  });

  it("checks token visibility across folded clauses and gap layer toggle", () => {
    const empty = createEmptyFoldState();
    const foldC1 = toggleClauseFold(empty, toClauseId("c1"));

    // Main token remains visible regardless of folds or gap toggle
    expect(isTokenVisible(SINGLE_GAP_CARD, toTokenId("t1"), foldC1, false)).toBe(true);

    // Sub-token hidden when its clause is folded
    expect(isTokenVisible(SINGLE_GAP_CARD, toTokenId("t2"), empty, true)).toBe(true);
    expect(isTokenVisible(SINGLE_GAP_CARD, toTokenId("t2"), foldC1, true)).toBe(false);

    // Gap slot hidden when gap layer is off OR when its clause is folded
    expect(isTokenVisible(SINGLE_GAP_CARD, toTokenId("g1"), empty, true)).toBe(true);
    expect(isTokenVisible(SINGLE_GAP_CARD, toTokenId("g1"), empty, false)).toBe(false);
    expect(isTokenVisible(SINGLE_GAP_CARD, toTokenId("g1"), foldC1, true)).toBe(false);
  });
});
