import type {
  ClauseGroup,
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

function collectClauseIds(clause: ClauseGroup, target: Set<ClauseId>): void {
  target.add(clause.id);
  for (const item of clause.items) {
    if (item.kind === "clause") {
      collectClauseIds(item, target);
    }
  }
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
      collectClauseIds(part, next);
    }
  }
  return next;
}

export function hasFoldableClauses(card: StructuredGrammarCard): boolean {
  return card.parts.some((part) => part.kind === "clause");
}

export function hasLineBreakTargets(card: StructuredGrammarCard): boolean {
  return card.parts.some((part) => part.kind === "clause" || part.kind === "connector");
}

export function areAllClausesFolded(
  card: StructuredGrammarCard,
  folded: FoldedClauses,
): boolean {
  const allIds = new Set<ClauseId>();
  for (const part of card.parts) {
    if (part.kind === "clause") {
      collectClauseIds(part, allIds);
    }
  }
  if (allIds.size === 0) {
    return false;
  }
  for (const id of allIds) {
    if (!folded.has(id)) {
      return false;
    }
  }
  return true;
}

function clauseHasGapSlots(clause: ClauseGroup): boolean {
  for (const item of clause.items) {
    if (item.kind === "gap") {
      return true;
    }
    if (item.kind === "clause" && clauseHasGapSlots(item)) {
      return true;
    }
  }
  return false;
}

export function hasGapSlots(card: StructuredGrammarCard): boolean {
  for (const part of card.parts) {
    if (part.kind === "clause" && clauseHasGapSlots(part)) {
      return true;
    }
  }
  return false;
}

function clauseHasOmittedHints(clause: ClauseGroup): boolean {
  if (clause.omittedHint !== undefined && clause.omittedHint.length > 0) {
    return true;
  }
  for (const item of clause.items) {
    if (item.kind === "clause" && clauseHasOmittedHints(item)) {
      return true;
    }
  }
  return false;
}

export function hasOmittedHints(card: StructuredGrammarCard): boolean {
  for (const part of card.parts) {
    if (part.kind === "clause" && clauseHasOmittedHints(part)) {
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

function isTokenVisibleInClause(
  clause: ClauseGroup,
  id: TokenId,
  folded: FoldedClauses,
  showGapLayer: boolean,
): boolean {
  if (folded.has(clause.id)) {
    return false;
  }
  for (const item of clause.items) {
    switch (item.kind) {
      case "sub-token":
      case "connector":
        if (item.id === id) {
          return true;
        }
        break;
      case "gap":
        if (item.id === id) {
          return showGapLayer;
        }
        break;
      case "clause":
        if (isTokenVisibleInClause(item, id, folded, showGapLayer)) {
          return true;
        }
        break;
      default: {
        const _exhaustive: never = item;
        return _exhaustive;
      }
    }
  }
  return false;
}

export function isTokenVisible(
  card: StructuredGrammarCard,
  id: TokenId,
  folded: FoldedClauses,
  showGapLayer: boolean,
): boolean {
  for (const part of card.parts) {
    switch (part.kind) {
      case "token":
      case "connector":
        if (part.id === id) {
          return true;
        }
        break;
      case "clause":
        if (isTokenVisibleInClause(part, id, folded, showGapLayer)) {
          return true;
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
  return false;
}

function findTokenNoteInClause(clause: ClauseGroup, id: TokenId): TokenNote | null {
  for (const item of clause.items) {
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
      case "clause": {
        const nested = findTokenNoteInClause(item, id);
        if (nested !== null) {
          return nested;
        }
        break;
      }
      default: {
        const _exhaustive: never = item;
        return _exhaustive;
      }
    }
  }
  return null;
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
      case "connector":
        if (part.id === id && part.note.trim().length > 0) {
          return { title: part.text, note: part.note };
        }
        break;
      case "clause": {
        const found = findTokenNoteInClause(part, id);
        if (found !== null) {
          return found;
        }
        break;
      }
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
