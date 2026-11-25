CREATE TABLE IF NOT EXISTS providers (
    id TEXT PRIMARY KEY NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('ollama', 'openai-compatible', 'anthropic')),
    name TEXT NOT NULL,
    base_url TEXT NOT NULL,
    api_key_ref TEXT NOT NULL DEFAULT '',
    model TEXT NOT NULL,
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE IF NOT EXISTS api_keys (
    key_ref TEXT PRIMARY KEY NOT NULL,
    secret_value TEXT NOT NULL,
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE IF NOT EXISTS custom_actions (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    system_prompt TEXT NOT NULL,
    is_built_in INTEGER NOT NULL DEFAULT 0 CHECK (is_built_in IN (0, 1)),
    render_mode TEXT NOT NULL CHECK (render_mode IN ('unified-3layer', 'custom-html')),
    html_template TEXT,
    css_template TEXT,
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE IF NOT EXISTS history (
    id TEXT PRIMARY KEY NOT NULL,
    source_text TEXT NOT NULL,
    provider_id TEXT NOT NULL,
    action_id TEXT NOT NULL,
    raw_json TEXT NOT NULL,
    latency_ms INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_history_created_at_desc
    ON history (created_at DESC, id DESC);

CREATE TABLE IF NOT EXISTS fsrs_state (
    card_id TEXT PRIMARY KEY NOT NULL,
    stability REAL NOT NULL DEFAULT 0.0,
    difficulty REAL NOT NULL DEFAULT 0.0,
    due_at INTEGER NOT NULL DEFAULT 0,
    reps INTEGER NOT NULL DEFAULT 0,
    lapses INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);
