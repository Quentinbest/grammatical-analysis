# grammatical-analysis

A local desktop application for cross-app text-selection translation and English sentence structure analysis.

This project combines two sources:

1. **Grammar analysis methodology and textbook corpus** from `english-structure-coach` (based on 旋元佑进阶文法).
2. **Customizable HTML card rendering** inspired by `mengxi-ream/read-frog` Custom AI Actions.

## Core Decisions

- **Desktop runtime**: Tauri (Rust backend + React/TypeScript/TailwindCSS frontend) instead of a browser extension or Electron.
- **Trigger mechanism**: Global hotkey + simulated copy (`Cmd+C` / `Ctrl+C`) + clipboard restore + always-on-top floating window near the cursor.
- **Card rendering**: Adopt `read-frog` Custom AI Actions style user-editable HTML/CSS templates with flat JSON fields, sanitized with `DOMPurify` and isolated with Shadow DOM or scoped CSS.
- **Dropped components**: `Presentation Record v1`, `assets/archify-native/`, `render_structure_archify.py`, and `render_structure_svg.py` are intentionally excluded. No Python or Node.js runtime is bundled in the desktop app.
- **LLM & storage**: Support local LLMs (Ollama, LM Studio) as first-class providers alongside optional cloud APIs, with local SQLite (`tauri-plugin-sql`) for action templates, history, and optional FSRS review.

## Directory Structure

- `docs/architecture.md`: Detailed architecture design, JSON field contract for grammar analysis cards, security boundaries, and textbook retrieval strategy.
- `docs/source-thread-summary.md`: Summary of the reference Gemini discussion on `read-frog` Custom AI Actions and desktop customization.
- `prompts/`: Migrated English grammar analysis prompts, workflows, teaching method, and textbook caveat rules.
- `knowledge/book/`: Migrated 旋元佑进阶文法 Markdown chapters (`chapters/`), topic index (`index.md`), and manifest (`manifest.json`).
