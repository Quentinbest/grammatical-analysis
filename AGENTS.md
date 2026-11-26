# Agent instructions

Run `pnpm check` and `cargo test --offline --manifest-path src-tauri/Cargo.toml` before committing.

## Rules and enforcement

| Rule | Enforced by |
| --- | --- |
| Keep `version` identical across `package.json`, `src-tauri/tauri.conf.json`, and `src-tauri/Cargo.toml` | `scripts/check-repo.mjs` (`pnpm check` and `.github/workflows/ci.yml`) |
| Keep `PROVIDER_PRESETS` in `src/components/SettingsDrawer/SettingsDrawer.tsx` in sync with `default_providers()` in `src-tauri/src/storage/mod.rs` | `scripts/check-repo.mjs` (`pnpm check` and `.github/workflows/ci.yml`) |
| Run TypeScript typechecking, Vite build, and Vitest together on every change | `pnpm check` and `.github/workflows/ci.yml` |
| Omit `max_tokens` and `num_predict` by default on OpenAI-compatible and Ollama requests; use `8192` on Anthropic | `default_request_omits_token_cap_for_openai_and_ollama_and_uses_8192_for_anthropic` in `src-tauri/src/llm/mod.rs` |
| Treat JSON `null` as omitted on optional `StructuredGrammarCard` fields while rejecting non-null invalid types | `src/domain/__tests__/parseCard.test.ts` (`pnpm check` and `.github/workflows/ci.yml`) |
| Keep local SQLite persistence, BM25 retrieval, and SSE/NDJSON parsers green on every PR | `cargo test --manifest-path src-tauri/Cargo.toml` in `.github/workflows/ci.yml` |
