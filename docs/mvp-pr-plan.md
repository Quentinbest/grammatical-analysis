# MVP multi-phase PR plan

This plan breaks the `grammatical-analysis` desktop MVP into six independently verifiable PRs. Every PR delivers one bounded change with unit, live, and performance checks. Do not start implementation until the operator approves this plan.

## Overview of changes

The MVP delivers a single Tauri v2 desktop binary with a Rust core engine and a React/TypeScript frontend. Users select English text in any desktop app, press a global hotkey, and see a cursor-near floating card that combines inline syntactic annotation, in-place backbone folding, and conditional co-indexed gap traces in both Dark Ink and Warm Paper skins. Local LLMs and cloud APIs run on equal footing, backed by local `knowledge/book/` BM25 retrieval and SQLite persistence.

## PR landing order and dependency graph

Landing order: `PR-01` -> (`PR-02`, `PR-03`, `PR-04`, `PR-05` in parallel) -> `PR-06`.

```text
trunk
  └── PR-01 (scaffold + shared domain types + boundary parser)
        ├── PR-02 (unified 3-layer card UI + themes + Shadow DOM)   [operator review-gated]
        ├── PR-03 (Rust BM25 textbook retrieval + prompt assembler)
        ├── PR-04 (Rust equal-status local/cloud LLM streaming)
        └── PR-05 (OS hotkey + clipboard restore + floating window) [operator review-gated]
              └── PR-06 (SQLite store + Actions CRUD + E2E wiring)  [operator review-gated]
```

### Non-overlapping file ownership

- `PR-01`: `package.json`, `tsconfig.json`, `vite.config.ts`, `vitest.config.ts`, `index.html`, `src/main.tsx`, `src/domain/types.ts`, `src/domain/parseCard.ts`, `src/domain/__tests__/`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, `src-tauri/src/main.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/domain/`
- `PR-02`: `src/domain/cardViewModel.ts`, `src/styles/tokens.css`, `src/components/UnifiedCard/`, `src/components/CustomTemplateSandbox/`, `src/components/__tests__/`
- `PR-03`: `src-tauri/src/retrieval/`
- `PR-04`: `src-tauri/src/llm/`
- `PR-05`: `src-tauri/src/os/`
- `PR-06`: `src-tauri/src/storage/`, `src-tauri/migrations/`, `src-tauri/src/commands.rs`, `src/App.tsx`, `src/hooks/useAnalysisStream.ts`, `src/components/SettingsDrawer/`

---

## PR-01. Tauri scaffold, shared domain types, and boundary parser

- [ ] **Depends on:** trunk (none).
- [ ] **Files:** `package.json`, `tsconfig.json`, `vite.config.ts`, `vitest.config.ts`, `index.html`, `src/main.tsx`, `src/domain/types.ts`, `src/domain/parseCard.ts`, `src/domain/__tests__/parseCard.test.ts`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, `src-tauri/src/main.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/domain/types.rs`.
- [ ] **One change:** Initialize the Tauri v2 + React + TypeScript + Vitest workspace, define the branded TypeScript and Rust domain types from `docs/design-spec.md`, and implement `parseGrammarCard` with fallback handling for malformed LLM JSON.
- [ ] **Visible result:** Empty Tauri desktop shell compiles and launches, and `parseGrammarCard` parses all five prototype golden sentences into typed `StructuredGrammarCard` objects while degrading malformed JSON to `FallbackGrammarCard`.
- [ ] **Unit verification:** `pnpm test` passes all 5 golden sentence fixtures plus malformed JSON, orphan `GapIndex`, and missing optional fields tests; `cargo test --manifest-path src-tauri/Cargo.toml` passes serde round-trip tests.
- [ ] **Live verification:** Run `pnpm tauri dev` in the PR worktree; confirm the desktop window opens with zero console errors and exits cleanly on close.
- [ ] **Perf verification:** Metric: `parseGrammarCard` p99 latency on the 34-word complex sentence fixture. Probe: Vitest benchmark script running 1,000 parses. Trunk baseline: N/A (establishes initial baseline on trunk). Fail threshold: p99 > 2.0 ms.

## PR-02. Unified three-layer grammar card UI, dual themes, and Shadow DOM sandbox

**Operator review-gated:** changes user-facing visual layout and card interaction.

- [ ] **Depends on:** `PR-01`.
- [ ] **Files:** `src/domain/cardViewModel.ts`, `src/styles/tokens.css`, `src/components/UnifiedCard/*`, `src/components/CustomTemplateSandbox/*`, `src/components/__tests__/*`.
- [ ] **One change:** Build the cohesive `UnifiedCard` component implementing L1 inline syntactic annotation, L2 in-place backbone folding, L3 conditional co-indexed gap traces (same-line always visible, cross-line on click), Dark Ink and Warm Paper themes, and a `DOMPurify` + Shadow DOM sandbox for custom HTML templates.
- [ ] **Visible result:** Storybook or dev preview renders all five prototype sentences cleanly at both 340px and 560px widths in Dark Ink and Warm Paper skins, with interactive clause folding, gap toggling, and XSS-safe custom template rendering.
- [ ] **Unit verification:** `pnpm test` verifies `cardViewModel` fold/unfold transitions, `shouldDrawGapArc` cross-line suppression, and `DOMPurify` stripping `<script>` and `onerror` attributes in custom templates.
- [ ] **Live verification:** Launch `pnpm dev` in the PR worktree, drive the preview with headless Chrome across all 5 sentences in both themes and 340px/560px widths, capture screenshots, and verify zero text overlap, zero clipping, and working click-to-fold and click-to-trace behavior.
- [ ] **Perf verification:** Metric: initial render plus SVG trace paint time for the 34-word dual-gap sentence. Probe: `performance.now()` timing harness in headless Chrome over 20 renders. Trunk baseline: measured on `PR-01` empty shell mount time first. Fail threshold: card render + trace paint > 16 ms (one 60fps frame).
- [ ] **Operator review:** Operator approves screenshots and interaction before merge.

## PR-03. Local textbook BM25 retrieval and prompt assembler (Rust)

- [ ] **Depends on:** `PR-01`.
- [ ] **Files:** `src-tauri/src/retrieval/mod.rs`, `src-tauri/src/retrieval/bm25.rs`, `src-tauri/src/retrieval/prompt_builder.rs`.
- [ ] **One change:** Index `knowledge/book/index.md` and `knowledge/book/chapters/*.md` in Rust using lightweight BM25 lookup, assemble the grammar analysis system prompt from `prompts/SKILL.md`, `prompts/workflows.md`, `prompts/teaching-method.md`, and always include high-frequency guardrails from `prompts/book-caveats.md`.
- [ ] **Visible result:** Running the Rust CLI probe against test sentences (`While walking home, the rain started.`, `The book Maya recommended won a prize.`) prints deterministic top-k chapter hits (`ch25`, `ch19`) and a bounded prompt containing caveat rules and the JSON output schema.
- [ ] **Unit verification:** `cargo test --manifest-path src-tauri/Cargo.toml retrieval` verifies top-3 recall on 10 curated grammar queries, verifies `book-caveats.md` is always present, and verifies prompt token budget stays under the configured cap.
- [ ] **Live verification:** Run `cargo run --manifest-path src-tauri/Cargo.toml --bin retrieval_probe -- "While walking home, the rain started."` in the PR worktree; confirm exit code 0 and that `ch25` appears in the returned hits with a real passage ID.
- [ ] **Perf verification:** Metric: cold index load time and p95 single-query BM25 latency across all chapters in `knowledge/book/chapters/`. Probe: Rust benchmark binary running 200 queries. Trunk baseline: N/A (first retrieval implementation; recorded as baseline). Fail threshold: cold index load > 150 ms or query p95 > 10 ms.

## PR-04. Equal-status local and cloud LLM streaming engine (Rust)

- [ ] **Depends on:** `PR-01`.
- [ ] **Files:** `src-tauri/src/llm/mod.rs`, `src-tauri/src/llm/provider.rs`, `src-tauri/src/llm/openai_compat.rs`, `src-tauri/src/llm/anthropic.rs`, `src-tauri/src/llm/ollama.rs`.
- [ ] **One change:** Implement the unified `LlmProvider` trait in Rust supporting Ollama, OpenAI-compatible (LM Studio, OpenAI, Gemini compat), and Anthropic streaming endpoints on equal footing, emitting typed `AnalysisStreamEvent` messages with request cancellation and timeouts.
- [ ] **Visible result:** Mock SSE servers and a real local or cloud endpoint stream `TokenDelta` chunks and a final `Completed` payload through the same trait without CORS errors.
- [ ] **Unit verification:** `cargo test --manifest-path src-tauri/Cargo.toml llm` tests SSE chunk reassembly, cancellation mid-stream, HTTP 401/429/500 error mapping, and timeout handling against local `wiremock` servers for all three provider kinds.
- [ ] **Live verification:** Start a local mock SSE server (and optionally a live Ollama or cloud endpoint if credentials exist in env) in the PR worktree, run `cargo run --bin llm_stream_probe`, and confirm streamed deltas assemble into a valid `StructuredGrammarCard` JSON payload.
- [ ] **Perf verification:** Metric: SSE frame parsing overhead per 1,000 streamed chunks. Probe: Rust timing test feeding 1,000 synthetic SSE lines through the parser. Trunk baseline: N/A (establishes streaming parser baseline). Fail threshold: total overhead > 15 ms (0.015 ms per chunk).

## PR-05. OS global hotkey, clipboard capture/restore, and cursor-near floating window

**Operator review-gated:** changes system-level hotkey behavior, clipboard handling, and floating window ergonomics.

- [ ] **Depends on:** `PR-01`.
- [ ] **Files:** `src-tauri/src/os/mod.rs`, `src-tauri/src/os/hotkey.rs`, `src-tauri/src/os/clipboard.rs`, `src-tauri/src/os/floating_window.rs`.
- [ ] **One change:** Register a configurable global hotkey (default `Alt+E`), snapshot clipboard text, trigger `Cmd+C` or `Ctrl+C`, capture the selected text, restore the original clipboard via a `Drop` guard, and display a pre-warmed frameless always-on-top floating window near the cursor within screen bounds.
- [ ] **Visible result:** Selecting text in another macOS app and pressing `Alt+E` pops the frameless floating window beside the cursor showing the captured text, while `Cmd+V` still pastes the user's original clipboard content.
- [ ] **Unit verification:** `cargo test --manifest-path src-tauri/Cargo.toml os` tests window-clamping math across multi-monitor screen edges and verifies the clipboard `Drop` guard restores prior text on both success and simulated timeout errors.
- [ ] **Live verification:** Run `pnpm tauri dev` in the PR worktree, copy a sentinel string `PRIOR_CLIPBOARD_TEST`, select an English sentence in a separate text editor, press `Alt+E`, confirm the floating window opens near the cursor with the selected sentence, press `Esc` to dismiss, and confirm the clipboard still equals `PRIOR_CLIPBOARD_TEST`.
- [ ] **Perf verification:** Metric: hotkey-press to floating-window-visible latency (pre-warmed WebView). Probe: Rust `Instant::now()` timestamp logged from hotkey callback to window `show()` completion over 10 triggers. Trunk baseline: cold `PR-01` window creation time measured first. Fail threshold: p95 hotkey-to-visible > 180 ms.
- [ ] **Operator review:** Operator tests hotkey feel, cursor placement, and macOS Accessibility permission prompt before merge.

## PR-06. SQLite persistence, Custom AI Actions store, and end-to-end MVP wiring

**Operator review-gated:** final user-facing end-to-end desktop workflow.

- [ ] **Depends on:** `PR-02`, `PR-03`, `PR-04`, `PR-05`.
- [ ] **Files:** `src-tauri/src/storage/mod.rs`, `src-tauri/migrations/001_init.sql`, `src-tauri/src/commands.rs`, `src/App.tsx`, `src/hooks/useAnalysisStream.ts`, `src/components/SettingsDrawer/*`.
- [ ] **One change:** Wire `tauri-plugin-sql` (SQLite) for provider configs, Custom AI Actions templates, and query history, then connect the full pipeline from global hotkey capture through BM25 retrieval and LLM streaming to the unified three-layer card and local history persistence.
- [ ] **Visible result:** Pressing `Alt+E` on any English sentence in any desktop app streams a structured grammar analysis card in Dark Ink or Warm Paper skin, allows switching equal-status local or cloud providers and Custom AI Actions, and persists history across app restarts.
- [ ] **Unit verification:** `cargo test --manifest-path src-tauri/Cargo.toml storage` verifies SQLite migrations and CRUD round-trips for providers, actions, and history; `pnpm test` verifies `useAnalysisStream` state transitions (`idle` -> `streaming` -> `ready` / `fallback` / `error`).
- [ ] **Live verification:** Run `pnpm tauri dev` in the PR worktree; execute the full scenario: configure a provider, trigger `Alt+E` on a complex sentence, verify streamed card renders with folding and gap traces, edit a custom HTML Action template and confirm Shadow DOM renders it safely, restart the app, and confirm saved history and provider config persist.
- [ ] **Perf verification:** Metric: idle resident memory (RSS) of the running Tauri binary after 10 analyses, and SQLite history query p95 at 1,000 rows. Probe: `ps -o rss=` on the running process and a Rust 1,000-row query benchmark. Trunk baseline: `PR-01` idle Tauri shell RSS measured first. Fail threshold: idle RSS > 150 MB or 1,000-row history query p95 > 15 ms.
- [ ] **Operator review:** Operator signs off on the end-to-end desktop MVP before merge.

---

## What remains unproven until execution

- **macOS Accessibility permission UX** for simulated `Cmd+C` in sandboxed or hardened apps (PDF viewers, Electron IDEs) cannot be verified until `PR-05` runs live on macOS.
- **Small local model JSON reliability** (for example Ollama 7B/14B models emitting valid `ClauseGroup` and `GapSlot` indices) must be measured empirically against our 5 test sentences during `PR-04` and `PR-06` to tune the prompt and confirm graceful degradation to `FallbackGrammarCard`.
- **FSRS spaced-repetition scheduling** is intentionally deferred out of MVP (`PR-01` through `PR-06`) so the core capture-to-card loop lands cleanly first; the SQLite migration reserves room for an `fsrs_state` table in a follow-up PR.
