# Architecture Design

## 1. System Overview

`grammatical-analysis` is a local-first desktop tool for selecting text in any application (PDF readers, IDEs, browsers, local documents) and rendering structured translation and English grammar analysis cards.

Instead of running inside a browser extension sandbox (Manifest V3) or bundling Python/Node.js visualization scripts, the application runs as a single Tauri binary with a Rust backend and a React/TypeScript frontend.

## 2. Layer Architecture

### 2.1 System Trigger & Window Layer (Rust / Tauri)

- **Global hotkey**: Register a system-wide shortcut (such as `Alt+E` or a user-configured keybinding).
- **Text capture**: Save the current clipboard state, trigger a system copy (`Cmd+C` on macOS, `Ctrl+C` on Windows/Linux), read the selected text, and restore the previous clipboard content.
- **Floating window**: Show a pre-warmed, frameless, always-on-top WebView window positioned near the cursor.

### 2.2 AI Orchestration & Prompt Layer

- **Provider support**: Connect to local OpenAI-compatible or Ollama endpoints (`http://localhost:11434`) as first-class providers, with optional cloud API keys for higher-accuracy grammar analysis.
- **Grammar analysis prompt**: Built from `prompts/SKILL.md`, `prompts/workflows.md`, `prompts/teaching-method.md`, and `prompts/book-caveats.md`.
- **Textbook retrieval**: Because a standalone desktop app does not run an agent file-reading loop, perform lightweight local keyword/BM25 lookup against `knowledge/book/index.md` and `knowledge/book/chapters/` before sending the prompt, and always include high-frequency rules from `prompts/book-caveats.md`.

### 2.3 Custom AI Actions & Card Renderer (React / TypeScript)

Instead of `Presentation Record v1` (which requires exact Unicode code point `spans` and geometry preflight checks), the built-in grammar analysis action and any user-defined actions share one flat JSON-to-HTML card pipeline:

1. **Flat JSON Output Contract** (example for the built-in English Structure Analysis action):
   - `meaning`: Accurate Chinese meaning of the full sentence or paragraph.
   - `chunked_sentence`: Original English text annotated with bracketed structural chunks (for example, `[The green apple on the table] [looks fresh]`).
   - `relations`: List of modifier/complement/discontinuous-collocation explanations.
   - `recombination`: Summary putting the chunked meanings back together.
   - `caveat_note`: Optional warning when textbook caveats or genuine ambiguities apply.
   - `book_ref`: Optional chapter/section reference from 旋元佑进阶文法.
2. **Customizable HTML/CSS Templates**:
   - Users can edit both the prompt and the HTML/CSS card layout per Action.
   - Streamed or completed JSON fields are interpolated into template placeholders.
3. **Security & Style Isolation**:
   - Sanitize rendered HTML strictly with `DOMPurify` to prevent XSS from user-edited or AI-generated templates.
   - Render cards inside Shadow DOM or a scoped CSS container so user styles cannot break the floating window layout.
   - Use event delegation on the card container for interactive behaviors (such as click-to-reveal explanations or audio playback triggers).

### 2.4 Local Persistence (SQLite)

- Use `tauri-plugin-sql` (SQLite) to store:
  - Custom AI Action definitions (name, prompt, JSON schema, HTML/CSS template).
  - Query history and saved analysis cards.
  - Optional FSRS spaced-repetition scheduling state for language review.

## 3. Excluded Legacy Assets

The following components from `english-structure-coach` were intentionally left out:

- `scripts/structure_presentation.py` (`Presentation Record v1` validator and layout preflight)
- `scripts/render_structure_archify.py` and `assets/archify-native/` (Node.js `archify.mjs` HTML fragment pipeline)
- `scripts/render_structure_svg.py` and `references/svg-diagrams.md` (1280x1280 static SVG fallback)
- `references/book/original/` (raw EPUB XHTML and image files)
