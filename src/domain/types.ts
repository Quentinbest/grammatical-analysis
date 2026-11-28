export type ProviderId = string & { readonly __brand: "ProviderId" };
export type ActionId = string & { readonly __brand: "ActionId" };
export type CardId = string & { readonly __brand: "CardId" };
export type TokenId = string & { readonly __brand: "TokenId" };
export type ClauseId = string & { readonly __brand: "ClauseId" };
export type GapIndex = 1 | 2 | 3;

export type SyntacticRole = "subj" | "pred" | "obj" | "comp" | "mod";
export type CardTheme = "dark-ink" | "warm-paper";

function isNonEmptyString(value: unknown): value is string {
  return typeof value === "string" && value.trim().length > 0;
}

export function isProviderId(value: unknown): value is ProviderId {
  return isNonEmptyString(value);
}

export function isActionId(value: unknown): value is ActionId {
  return isNonEmptyString(value);
}

export function isCardId(value: unknown): value is CardId {
  return isNonEmptyString(value);
}

export function isTokenId(value: unknown): value is TokenId {
  return isNonEmptyString(value);
}

export function isClauseId(value: unknown): value is ClauseId {
  return isNonEmptyString(value);
}

export function isGapIndex(value: unknown): value is GapIndex {
  return value === 1 || value === 2 || value === 3;
}

export function isSyntacticRole(value: unknown): value is SyntacticRole {
  return (
    value === "subj" ||
    value === "pred" ||
    value === "obj" ||
    value === "comp" ||
    value === "mod"
  );
}

export function isCardTheme(value: unknown): value is CardTheme {
  return value === "dark-ink" || value === "warm-paper";
}

export interface MainToken {
  readonly kind: "token";
  readonly id: TokenId;
  readonly text: string;
  readonly role: SyntacticRole;
  readonly subLabel?: string;
  readonly star?: boolean;
  readonly antecedentIndex?: GapIndex;
  readonly note: string;
}

export interface SubToken {
  readonly kind: "sub-token";
  readonly id: TokenId;
  readonly text: string;
  readonly role: SyntacticRole;
  readonly subLabel?: string;
  readonly star?: boolean;
  readonly antecedentIndex?: GapIndex;
  readonly note: string;
}

export interface GapSlot {
  readonly kind: "gap";
  readonly id: TokenId;
  readonly index: GapIndex;
  readonly note: string;
}

export interface InlineConnector {
  readonly kind: "connector";
  readonly id: TokenId;
  readonly text: string;
  readonly badge: string;
  readonly subLabel?: string;
  readonly star?: boolean;
  readonly note: string;
}

export type ClauseItem = SubToken | GapSlot | InlineConnector | ClauseGroup;

export interface ClauseGroup {
  readonly kind: "clause";
  readonly id: ClauseId;
  readonly badge: string;
  readonly connector?: string;
  readonly omittedHint?: string;
  readonly star?: boolean;
  readonly items: readonly ClauseItem[];
}

export interface PunctuationPart {
  readonly kind: "punct";
  readonly text: string;
}

export type SentencePart = MainToken | ClauseGroup | InlineConnector | PunctuationPart;

export interface BookRef {
  readonly chapter: string;
  readonly section: string;
  readonly passageId?: string;
}

export interface StructuredGrammarCard {
  readonly kind: "structured";
  readonly sourceText: string;
  readonly meaning: string;
  readonly parts: readonly SentencePart[];
  readonly recombination: string;
  readonly caveatNote: string | null;
  readonly bookRef: BookRef | null;
}

export interface FallbackGrammarCard {
  readonly kind: "fallback";
  readonly sourceText: string;
  readonly meaning: string;
  readonly rawText: string;
  readonly parseError: string;
}

export type GrammarCard = StructuredGrammarCard | FallbackGrammarCard;

export type ProviderConfig =
  | {
      readonly kind: "ollama";
      readonly id: ProviderId;
      readonly name: string;
      readonly baseUrl: string;
      readonly model: string;
    }
  | {
      readonly kind: "openai-compatible";
      readonly id: ProviderId;
      readonly name: string;
      readonly baseUrl: string;
      readonly apiKeyRef: string;
      readonly model: string;
    }
  | {
      readonly kind: "anthropic";
      readonly id: ProviderId;
      readonly name: string;
      readonly baseUrl: string;
      readonly apiKeyRef: string;
      readonly model: string;
    };

export interface CustomAction {
  readonly id: ActionId;
  readonly name: string;
  readonly systemPrompt: string;
  readonly isBuiltIn: boolean;
  readonly renderMode: "unified-3layer" | "custom-html";
  readonly htmlTemplate: string | null;
  readonly cssTemplate: string | null;
}

export interface RetrievalHit {
  readonly chapter: string;
  readonly section: string;
  readonly passageId?: string;
  readonly excerpt: string;
  readonly score: number;
}

export type AnalysisStreamEvent =
  | { readonly kind: "retrieval-ready"; readonly hits: readonly RetrievalHit[] }
  | { readonly kind: "token-delta"; readonly chunk: string }
  | { readonly kind: "completed"; readonly rawJson: string; readonly latencyMs: number }
  | { readonly kind: "failed"; readonly code: string; readonly message: string };
