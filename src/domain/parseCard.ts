import {
  type BookRef,
  type ClauseGroup,
  type ClauseItem,
  type FallbackGrammarCard,
  type GapIndex,
  type GapSlot,
  type GrammarCard,
  type InlineConnector,
  type MainToken,
  type PunctuationPart,
  type SentencePart,
  type SubToken,
  isClauseId,
  isGapIndex,
  isSyntacticRole,
  isTokenId,
} from "./types.ts";

export const MAX_CLAUSE_DEPTH = 3;

type ParseResult<T> = { readonly ok: true; readonly value: T } | { readonly ok: false; readonly error: string };

function ok<T>(value: T): ParseResult<T> {
  return { ok: true, value };
}

function fail<T>(error: string): ParseResult<T> {
  return { ok: false, error };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isNonEmptyString(value: unknown): value is string {
  return typeof value === "string" && value.trim().length > 0;
}

function stripMarkdownFences(input: string): string {
  const trimmed = input.trim();
  const match = /^```(?:json)?\s*\n?([\s\S]*?)\n?\s*```$/i.exec(trimmed);
  return match?.[1] !== undefined ? match[1].trim() : trimmed;
}

function stringifyRaw(raw: unknown): string {
  if (typeof raw === "string") {
    return raw;
  }
  try {
    const serialized = JSON.stringify(raw);
    return serialized ?? String(raw);
  } catch {
    return String(raw);
  }
}

function makeFallback(sourceText: string, rawText: string, parseError: string, meaning = ""): FallbackGrammarCard {
  return {
    kind: "fallback",
    sourceText,
    meaning,
    rawText,
    parseError,
  };
}

function parseOptionalString(value: unknown, field: string): ParseResult<string | undefined> {
  if (value === undefined || value === null) {
    return ok(undefined);
  }
  if (typeof value !== "string") {
    return fail(`${field} must be a string`);
  }
  return ok(value);
}

function parseOptionalBoolean(value: unknown, field: string): ParseResult<boolean | undefined> {
  if (value === undefined || value === null) {
    return ok(undefined);
  }
  if (typeof value !== "boolean") {
    return fail(`${field} must be a boolean`);
  }
  return ok(value);
}

function parseOptionalGapIndex(value: unknown, field: string): ParseResult<GapIndex | undefined> {
  if (value === undefined || value === null) {
    return ok(undefined);
  }
  if (!isGapIndex(value)) {
    return fail(`${field} must be 1, 2, or 3`);
  }
  return ok(value);
}

function parseTokenFields(
  obj: Record<string, unknown>,
  path: string,
): ParseResult<Omit<MainToken, "kind">> {
  if (!isTokenId(obj.id)) {
    return fail(`${path}.id must be a non-empty string`);
  }
  if (!isNonEmptyString(obj.text)) {
    return fail(`${path}.text must be a non-empty string`);
  }
  if (!isSyntacticRole(obj.role)) {
    return fail(`${path}.role must be subj, pred, obj, comp, or mod`);
  }
  if (typeof obj.note !== "string") {
    return fail(`${path}.note must be a string`);
  }

  const subLabel = parseOptionalString(obj.subLabel, `${path}.subLabel`);
  if (!subLabel.ok) {
    return subLabel;
  }
  const star = parseOptionalBoolean(obj.star, `${path}.star`);
  if (!star.ok) {
    return star;
  }
  const antecedentIndex = parseOptionalGapIndex(obj.antecedentIndex, `${path}.antecedentIndex`);
  if (!antecedentIndex.ok) {
    return antecedentIndex;
  }

  return ok({
    id: obj.id,
    text: obj.text,
    role: obj.role,
    note: obj.note,
    ...(subLabel.value !== undefined ? { subLabel: subLabel.value } : {}),
    ...(star.value !== undefined ? { star: star.value } : {}),
    ...(antecedentIndex.value !== undefined ? { antecedentIndex: antecedentIndex.value } : {}),
  });
}

function parseGapSlot(obj: Record<string, unknown>, path: string): ParseResult<GapSlot> {
  if (!isTokenId(obj.id)) {
    return fail(`${path}.id must be a non-empty string`);
  }
  if (!isGapIndex(obj.index)) {
    return fail(`${path}.index must be 1, 2, or 3`);
  }
  if (typeof obj.note !== "string") {
    return fail(`${path}.note must be a string`);
  }
  return ok({
    kind: "gap",
    id: obj.id,
    index: obj.index,
    note: obj.note,
  });
}

function parseInlineConnector(obj: Record<string, unknown>, path: string): ParseResult<InlineConnector> {
  if (!isTokenId(obj.id)) {
    return fail(`${path}.id must be a non-empty string`);
  }
  if (!isNonEmptyString(obj.text)) {
    return fail(`${path}.text must be a non-empty string`);
  }
  if (!isNonEmptyString(obj.badge)) {
    return fail(`${path}.badge must be a non-empty string`);
  }
  if (typeof obj.note !== "string") {
    return fail(`${path}.note must be a string`);
  }
  const subLabel = parseOptionalString(obj.subLabel, `${path}.subLabel`);
  if (!subLabel.ok) {
    return subLabel;
  }
  const star = parseOptionalBoolean(obj.star, `${path}.star`);
  if (!star.ok) {
    return star;
  }

  return ok({
    kind: "connector",
    id: obj.id,
    text: obj.text,
    badge: obj.badge,
    note: obj.note,
    ...(subLabel.value !== undefined ? { subLabel: subLabel.value } : {}),
    ...(star.value !== undefined ? { star: star.value } : {}),
  });
}

function parseClauseItem(value: unknown, path: string, depth: number): ParseResult<ClauseItem> {
  if (!isRecord(value)) {
    return fail(`${path} must be an object`);
  }
  switch (value.kind) {
    case "sub-token": {
      const fields = parseTokenFields(value, path);
      if (!fields.ok) {
        return fields;
      }
      const subToken: SubToken = { kind: "sub-token", ...fields.value };
      return ok(subToken);
    }
    case "gap":
      return parseGapSlot(value, path);
    case "connector":
      return parseInlineConnector(value, path);
    case "clause":
      return parseClauseGroup(value, path, depth + 1);
    default:
      return fail(`${path}.kind must be sub-token, gap, connector, or clause`);
  }
}

function parseClauseGroup(
  obj: Record<string, unknown>,
  path: string,
  depth = 1,
): ParseResult<ClauseGroup> {
  if (depth > MAX_CLAUSE_DEPTH) {
    return fail(`${path} exceeds maximum clause depth of ${MAX_CLAUSE_DEPTH}`);
  }
  if (!isClauseId(obj.id)) {
    return fail(`${path}.id must be a non-empty string`);
  }
  if (!isNonEmptyString(obj.badge)) {
    return fail(`${path}.badge must be a non-empty string`);
  }
  const connector = parseOptionalString(obj.connector, `${path}.connector`);
  if (!connector.ok) {
    return connector;
  }
  const omittedHint = parseOptionalString(obj.omittedHint, `${path}.omittedHint`);
  if (!omittedHint.ok) {
    return omittedHint;
  }
  const star = parseOptionalBoolean(obj.star, `${path}.star`);
  if (!star.ok) {
    return star;
  }
  if (!Array.isArray(obj.items)) {
    return fail(`${path}.items must be an array`);
  }

  const items: ClauseItem[] = [];
  for (let i = 0; i < obj.items.length; i += 1) {
    const parsedItem = parseClauseItem(obj.items[i], `${path}.items[${i}]`, depth);
    if (!parsedItem.ok) {
      return parsedItem;
    }
    items.push(parsedItem.value);
  }

  return ok({
    kind: "clause",
    id: obj.id,
    badge: obj.badge,
    items,
    ...(connector.value !== undefined ? { connector: connector.value } : {}),
    ...(omittedHint.value !== undefined ? { omittedHint: omittedHint.value } : {}),
    ...(star.value !== undefined ? { star: star.value } : {}),
  });
}

function parsePunctuationPart(obj: Record<string, unknown>, path: string): ParseResult<PunctuationPart> {
  if (!isNonEmptyString(obj.text)) {
    return fail(`${path}.text must be a non-empty string`);
  }
  return ok({ kind: "punct", text: obj.text });
}

function parseSentencePart(value: unknown, index: number): ParseResult<SentencePart> {
  const path = `parts[${index}]`;
  if (!isRecord(value)) {
    return fail(`${path} must be an object`);
  }
  switch (value.kind) {
    case "token": {
      const fields = parseTokenFields(value, path);
      if (!fields.ok) {
        return fields;
      }
      const token: MainToken = { kind: "token", ...fields.value };
      return ok(token);
    }
    case "clause":
      return parseClauseGroup(value, path, 1);
    case "connector":
      return parseInlineConnector(value, path);
    case "punct":
      return parsePunctuationPart(value, path);
    default:
      return fail(`${path}.kind must be token, clause, connector, or punct`);
  }
}

function parseBookRef(value: unknown): ParseResult<BookRef | null> {
  if (value === undefined || value === null) {
    return ok(null);
  }
  if (!isRecord(value)) {
    return fail("bookRef must be an object or null");
  }
  if (!isNonEmptyString(value.chapter)) {
    return fail("bookRef.chapter must be a non-empty string");
  }
  if (!isNonEmptyString(value.section)) {
    return fail("bookRef.section must be a non-empty string");
  }
  let passageId: string | undefined;
  if (value.passageId !== undefined && value.passageId !== null) {
    if (!isNonEmptyString(value.passageId)) {
      return fail("bookRef.passageId must be a non-empty string when provided");
    }
    passageId = value.passageId;
  }
  return ok({
    chapter: value.chapter,
    section: value.section,
    ...(passageId !== undefined ? { passageId } : {}),
  });
}

function parseCaveatNote(value: unknown): ParseResult<string | null> {
  if (value === undefined || value === null) {
    return ok(null);
  }
  if (typeof value !== "string") {
    return fail("caveatNote must be a string or null");
  }
  return ok(value.trim().length > 0 ? value : null);
}

function collectClauseAntecedents(clause: ClauseGroup, indices: Set<GapIndex>): void {
  for (const item of clause.items) {
    switch (item.kind) {
      case "sub-token":
        if (item.antecedentIndex !== undefined) {
          indices.add(item.antecedentIndex);
        }
        break;
      case "clause":
        collectClauseAntecedents(item, indices);
        break;
      case "gap":
      case "connector":
        break;
      default: {
        const _exhaustive: never = item;
        return _exhaustive;
      }
    }
  }
}

function collectAntecedentIndices(parts: readonly SentencePart[]): Set<GapIndex> {
  const indices = new Set<GapIndex>();
  for (const part of parts) {
    switch (part.kind) {
      case "token":
        if (part.antecedentIndex !== undefined) {
          indices.add(part.antecedentIndex);
        }
        break;
      case "clause":
        collectClauseAntecedents(part, indices);
        break;
      case "connector":
      case "punct":
        break;
      default: {
        const _exhaustive: never = part;
        return _exhaustive;
      }
    }
  }
  return indices;
}

function stripClauseOrphanGaps(
  clause: ClauseGroup,
  validIndices: ReadonlySet<GapIndex>,
): ClauseGroup {
  let changed = false;
  const nextItems: ClauseItem[] = [];
  for (const item of clause.items) {
    if (item.kind === "gap") {
      if (validIndices.has(item.index)) {
        nextItems.push(item);
      } else {
        changed = true;
      }
      continue;
    }
    if (item.kind === "clause") {
      const cleanedChild = stripClauseOrphanGaps(item, validIndices);
      if (cleanedChild !== item) {
        changed = true;
      }
      nextItems.push(cleanedChild);
      continue;
    }
    nextItems.push(item);
  }
  return changed ? { ...clause, items: nextItems } : clause;
}

function stripOrphanGaps(parts: readonly SentencePart[]): readonly SentencePart[] {
  const validIndices = collectAntecedentIndices(parts);
  return parts.map((part): SentencePart => {
    if (part.kind !== "clause") {
      return part;
    }
    return stripClauseOrphanGaps(part, validIndices);
  });
}

export function parseGrammarCard(raw: unknown, sourceText: string): GrammarCard {
  const rawText = stringifyRaw(raw);
  let candidate: unknown = raw;

  if (typeof raw === "string") {
    const cleaned = stripMarkdownFences(raw);
    if (cleaned.length === 0) {
      return makeFallback(sourceText, rawText, "Empty JSON input");
    }
    try {
      candidate = JSON.parse(cleaned);
    } catch (err) {
      const message = err instanceof Error ? err.message : "Invalid JSON";
      return makeFallback(sourceText, rawText, `JSON parse error: ${message}`);
    }
  }

  if (!isRecord(candidate)) {
    return makeFallback(sourceText, rawText, "Card payload must be a JSON object");
  }

  const meaningText = typeof candidate.meaning === "string" ? candidate.meaning : "";

  if (!isNonEmptyString(candidate.meaning)) {
    return makeFallback(sourceText, rawText, "meaning must be a non-empty string", meaningText);
  }
  if (!isNonEmptyString(candidate.recombination)) {
    return makeFallback(sourceText, rawText, "recombination must be a non-empty string", meaningText);
  }
  if (!Array.isArray(candidate.parts) || candidate.parts.length === 0) {
    return makeFallback(sourceText, rawText, "parts must be a non-empty array", meaningText);
  }

  const parsedParts: SentencePart[] = [];
  for (let i = 0; i < candidate.parts.length; i += 1) {
    const parsedPart = parseSentencePart(candidate.parts[i], i);
    if (!parsedPart.ok) {
      return makeFallback(sourceText, rawText, parsedPart.error, meaningText);
    }
    parsedParts.push(parsedPart.value);
  }

  const caveatNote = parseCaveatNote(candidate.caveatNote);
  if (!caveatNote.ok) {
    return makeFallback(sourceText, rawText, caveatNote.error, meaningText);
  }

  const bookRef = parseBookRef(candidate.bookRef);
  if (!bookRef.ok) {
    return makeFallback(sourceText, rawText, bookRef.error, meaningText);
  }

  const resolvedSource =
    sourceText.trim().length > 0
      ? sourceText
      : isNonEmptyString(candidate.sourceText)
        ? candidate.sourceText
        : sourceText;

  return {
    kind: "structured",
    sourceText: resolvedSource,
    meaning: candidate.meaning,
    parts: stripOrphanGaps(parsedParts),
    recombination: candidate.recombination,
    caveatNote: caveatNote.value,
    bookRef: bookRef.value,
  };
}
