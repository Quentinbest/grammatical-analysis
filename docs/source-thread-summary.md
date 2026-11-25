# Source Thread Summary

This document summarizes the key technical points from the reference Gemini discussion (`https://share.gemini.google/5ZlRsgE4e59E` -> `https://gemini.google.com/share/3147d6bb561b16bba1242e42801ade36`) that motivated this project.

## 1. Deconstructing `read-frog` Custom AI Actions

In `mengxi-ream/read-frog`, Custom AI Actions evolved into an HTML interactive card rendering system with four layers:

1. **UI Trigger**: Triggered from `SelectionToolbar` (`CustomActionToolButton`) after selecting text on a web page.
2. **Schema & Layout (`CUSTOM_ACTION_TEMPLATES`)**: Stores each action's prompt and HTML layout, supporting interactive elements (click-to-reveal, hover explanations, pronunciation) or AI-generated templates.
3. **Provider Factory**: Built on Vercel AI SDK to route requests across cloud providers and local models (such as Ollama), using structured JSON output to populate card fields.
4. **Streaming & Storage**: Maps streamed structured fields into HTML card placeholders with light/dark theme support and optional saving to Notebase / FSRS flashcard review.

## 2. Adapting to a Local Desktop Application

To move Custom AI Actions out of a Manifest V3 browser extension into a local cross-application desktop tool:

- **Framework**: Use Tauri instead of Electron to keep background memory usage low (native WebView) while reusing React + TailwindCSS.
- **Input Capture**: Replace `mouseup` / `window.getSelection()` with a Rust-side global hotkey, simulated copy (`Ctrl+C` / `Cmd+C`), clipboard read/restore, and an always-on-top floating window near the cursor.
- **Decouple Extension Bus**: Remove the content-script/Service-Worker messaging bus; call backend commands directly via Tauri `invoke` or frontend HTTP requests.
- **Card Rendering Security**: Keep HTML template interpolation, enforce strict `DOMPurify` sanitization, and isolate card styles.
- **Local LLMs & Storage**: Treat Ollama / LM Studio as first-class providers and replace browser IndexedDB (Dexie.js) with local SQLite (`tauri-plugin-sql`).
