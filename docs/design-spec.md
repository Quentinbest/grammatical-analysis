# Architecture design specification

This document defines the module boundaries, core types, function signatures, and state ownership for the `grammatical-analysis` desktop MVP. It builds on [architecture.md](architecture.md) and the visual decisions validated in the unified three-layer prototype.

## 1. Two competing architectural shapes

We compare two candidate shapes before writing production code.

### Shape A. Frontend-orchestrated thin Rust shell

React and TypeScript own LLM HTTP streaming, prompt assembly, textbook lookup, and card rendering. Rust only owns the global hotkey, clipboard capture, floating window placement, and a SQLite proxy.

- **Strengths.** One TypeScript codebase handles prompts, streaming, and UI. Unit tests run in Vitest without Rust compilation.
- **Weaknesses.** WKWebView enforces CORS and TLS rules that break arbitrary local or cloud LLM endpoints. API keys sit in WebView JavaScript memory. Reading `knowledge/book/chapters/*.md` requires either bundling the full textbook into the frontend asset bundle or crossing IPC on every lookup.

### Shape B. Rust core engine with pure TypeScript contract and renderer (chosen)

Rust owns OS integration, local textbook BM25 retrieval, prompt assembly, equal-status local and cloud LLM streaming, and SQLite persistence. TypeScript owns strict boundary parsing, pure view-model projection, the unified three-layer card renderer, and Shadow DOM plus `DOMPurify` template isolation.

- **Strengths.** Zero CORS issues across Ollama, LM Studio, OpenAI, Anthropic, and Gemini. API keys stay inside Rust and local storage rather than WebView state. BM25 lookup reads `knowledge/book/` directly from disk in milliseconds. The frontend stays pure and deterministic because it consumes one typed stream of domain events.
- **Weaknesses.** Rust must maintain one HTTP SSE client for OpenAI-compatible or Ollama endpoints and one adapter for Anthropic Messages streaming. Both adapters share a single `LlmProvider` trait, which keeps the extra code bounded.

### Decision

Adopt **Shape B**. Each piece of state has one owner, each boundary parses untrusted input once, and callers inside each layer never re-validate internal types.

## 2. State ownership and module boundaries

| State or responsibility | Single owner | Exposed boundary |
|---|---|---|
| Global hotkey, clipboard save/copy/restore, cursor-near floating window | `src-tauri/src/os/` | Tauri event `selection://captured` |
| Textbook index, chapter search, and caveat injection | `src-tauri/src/retrieval/` | Pure Rust `retrieve_book_context()` |
| Equal-status local and cloud LLM HTTP streaming | `src-tauri/src/llm/` | Tauri command `run_analysis` with `Channel<AnalysisStreamEvent>` |
| Custom AI Actions, provider configs, and card history | `src-tauri/src/storage/` (SQLite) | Tauri commands `actions_*`, `providers_*`, `history_*` |
| Raw LLM JSON parsing and fallback normalization | `src/domain/parseCard.ts` | Pure function `parseGrammarCard(raw: unknown)` |
| Three-layer card projection (fold state, gap trace eligibility) | `src/domain/cardViewModel.ts` | Pure functions with zero React or DOM imports |
| Dark Ink and Warm Paper UI plus Shadow DOM custom templates | `src/components/UnifiedCard/` | React components plus `DOMPurify` sanitizer |

## 3. Core domain types (TypeScript)

The frontend defines the canonical card shape in `src/domain/types.ts`. Semantic primitives use branded string or numeric types so callers cannot swap IDs.

```ts
export type ProviderId = string & { readonly __brand: "ProviderId" };
export type ActionId = string & { readonly __brand: "ActionId" };
export type CardId = string & { readonly __brand: "CardId" };
export type TokenId = string & { readonly __brand: "TokenId" };
export type ClauseId = string & { readonly __brand: "ClauseId" };
export type GapIndex = 1 | 2 | 3;

export type SyntacticRole = "subj" | "pred" | "obj" | "mod";
export type CardTheme = "dark-ink" | "warm-paper";

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

export type ClauseItem = SubToken | GapSlot | InlineConnector;

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

export type SentencePart = MainToken | ClauseGroup | PunctuationPart;

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
```

### Equal-status provider configuration

Local models and cloud vendor APIs share one discriminated union with no primary or fallback hierarchy.

```ts
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
```

### Custom AI Action contract

```ts
export interface CustomAction {
	readonly id: ActionId;
	readonly name: string;
	readonly systemPrompt: string;
	readonly isBuiltIn: boolean;
	readonly renderMode: "unified-3layer" | "custom-html";
	readonly htmlTemplate: string | null;
	readonly cssTemplate: string | null;
}
```

## 4. Core Rust interfaces and Tauri IPC signatures

Rust mirrors the wire payloads in `src-tauri/src/domain/types.rs` using `serde` with `#[serde(tag = "kind", rename_all = "kebab-case")]`.

```rust
pub struct RetrievalHit {
	pub chapter: String,
	pub section: String,
	pub passage_id: Option<String>,
	pub excerpt: String,
	pub score: f32,
}

pub trait BookRetriever: Send + Sync {
	fn retrieve(&self, source_text: &str, limit: usize) -> Vec<RetrievalHit>;
}

pub enum AnalysisStreamEvent {
	RetrievalReady { hits: Vec<RetrievalHit> },
	TokenDelta { chunk: String },
	Completed { raw_json: String, latency_ms: u64 },
	Failed { code: String, message: String },
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
	async fn stream_analysis(
		&self,
		request: AnalysisPromptRequest,
		on_event: impl Fn(AnalysisStreamEvent) + Send + Sync,
	) -> Result<(), ProviderError>;
}
```

### Tauri command surface

- `run_analysis(request: RunAnalysisInput, channel: Channel<AnalysisStreamEvent>) -> Result<(), String>`
- `cancel_analysis(request_id: String) -> Result<(), String>`
- `list_providers() -> Result<Vec<ProviderConfig>, String>`
- `upsert_provider(config: ProviderConfig) -> Result<(), String>`
- `list_actions() -> Result<Vec<CustomAction>, String>`
- `upsert_action(action: CustomAction) -> Result<(), String>`
- `save_history(entry: HistoryEntryInput) -> Result<CardId, String>`
- `list_history(limit: u32, offset: u32) -> Result<Vec<HistoryEntry>, String>`

## 5. Boundary discipline and failure modes

1. **LLM JSON boundary.** `parseGrammarCard(raw, sourceText)` validates untrusted JSON once. It verifies that every `GapSlot.index` matches an existing `antecedentIndex`, drops orphan gap indices cleanly, and falls back to `FallbackGrammarCard` when JSON fails to parse. The floating window never crashes on imperfect local model output.
2. **Custom HTML template boundary.** Any `custom-html` action runs through `DOMPurify.sanitize` with scripts and inline event handlers stripped before mounting into a closed or open Shadow Root.
3. **Clipboard boundary.** `src-tauri/src/os/clipboard.rs` snapshots prior clipboard text, simulates `Cmd+C` or `Ctrl+C`, polls for a change up to 250ms, reads the selection, and restores the prior clipboard content in a `Drop` guard so failures never wipe the user's clipboard.
4. **Cross-line gap trace rule.** `src/domain/cardViewModel.ts` exposes `shouldDrawGapArc(sameLine, isSelected)`. Same-line gaps draw dashed curves by default. Cross-line gaps keep their co-indexed badges visible and draw curves only when selected, preventing lines from crossing intermediate text.

## 6. Accepted deviations log

Record any approved deviation from this specification during PR execution here.

- None yet.
