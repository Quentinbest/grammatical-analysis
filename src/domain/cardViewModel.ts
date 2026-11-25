import type {
  ClauseId,
  GapIndex,
  StructuredGrammarCard,
  TokenId,
} from "./types.ts";

export type FoldedClauses = ReadonlySet<ClauseId>;

export interface TokenNote {
  readonly title: string;
  readonly note: string;
}

export function createEmptyFoldState(): FoldedClauses {
  return new Set<ClauseId>();
}

export function toggleClauseFold(
  folded: FoldedClauses,
  clauseId: ClauseId,
): FoldedClauses {
  const next = new Set<ClauseId>(folded);
  if (next.has(clauseId)) {
    next.delete(clauseId);
  } else {
    next.add(clauseId);
  }
  return next;
}

export function setAllClausesFolded(
  card: StructuredGrammarCard,
  shouldFold: boolean,
): FoldedClauses {
  if (!shouldFold) {
    return new Set<ClauseId>();
  }
  const next = new Set<ClauseId>();
  for (const part of card.parts) {
    if (part.kind === "clause") {
      next.add(part.id);
    }
  }
  return next;
}

export function hasFoldableClauses(card: StructuredGrammarCard): boolean {
  return card.parts.some((part) => part.kind === "clause");
}

export function areAllClausesFolded(
  card: StructuredGrammarCard,
  folded: FoldedClauses,
): boolean {
  let clauseCount = 0;
  for (const part of card.parts) {
    if (part.kind === "clause") {
      clauseCount += 1;
      if (!folded.has(part.id)) {
        return false;
      }
    }
  }
  return clauseCount > 0;
}

export function hasGapSlots(card: StructuredGrammarCard): boolean {
  for (const part of card.parts) {
    if (part.kind === "clause") {
      for (const item of part.items) {
        if (item.kind === "gap") {
          return true;
        }
      }
    }
  }
  return false;
}

export function hasOmittedHints(card: StructuredGrammarCard): boolean {
  for (const part of card.parts) {
    if (part.kind === "clause" && part.omittedHint !== undefined && part.omittedHint.length > 0) {
      return true;
    }
  }
  return false;
}

export function shouldDrawGapArc(sameLine: boolean, isSelected: boolean): boolean {
  return sameLine || isSelected;
}

export function formatGapTitle(index: GapIndex): string {
  return `空位 □${index}`;
}

export function isTokenVisible(
  card: StructuredGrammarCard,
  id: TokenId,
  folded: FoldedClauses,
  showGapLayer: boolean,
): boolean {
  for (const part of card.parts) {
    if (part.kind === "token" && part.id === id) {
      return true;
    }
    if (part.kind === "clause") {
      if (folded.has(part.id)) {
        continue;
      }
      for (const item of part.items) {
        if (item.id === id) {
          return item.kind !== "gap" || showGapLayer;
        }
      }
    }
  }
  return false;
}

export function findTokenNote(
  card: StructuredGrammarCard,
  id: TokenId | null,
): TokenNote | null {
  if (id === null) {
    return null;
  }

  for (const part of card.parts) {
    switch (part.kind) {
      case "token":
        if (part.id === id && part.note.trim().length > 0) {
          return { title: part.text, note: part.note };
        }
        break;
      case "clause":
        for (const item of part.items) {
          switch (item.kind) {
            case "sub-token":
              if (item.id === id && item.note.trim().length > 0) {
                return { title: item.text, note: item.note };
              }
              break;
            case "gap":
              if (item.id === id && item.note.trim().length > 0) {
                return { title: formatGapTitle(item.index), note: item.note };
              }
              break;
            case "connector":
              if (item.id === id && item.note.trim().length > 0) {
                return { title: item.text, note: item.note };
              }
              break;
            default: {
              const _exhaustive: never = item;
              return _exhaustive;
            }
          }
        }
        break;
      case "punct":
        break;
      default: {
        const _exhaustive: never = part;
        return _exhaustive;
      }
    }
  }

  return null;
}
