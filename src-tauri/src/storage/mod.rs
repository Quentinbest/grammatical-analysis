use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::domain::types::{CustomAction, ProviderConfig, RenderMode};

pub const INIT_MIGRATION_SQL: &str = include_str!("../../migrations/001_init.sql");

static CARD_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("storage lock poisoned")]
    LockPoisoned,
    #[error("validation error: {0}")]
    Validation(String),
    #[error("invalid stored data: {0}")]
    CorruptData(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntryInput {
    pub source_text: String,
    pub provider_id: String,
    pub action_id: String,
    pub raw_json: String,
    pub latency_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: String,
    pub source_text: String,
    pub provider_id: String,
    pub action_id: String,
    pub raw_json: String,
    pub latency_ms: u64,
    pub created_at: i64,
}

pub struct Storage {
    conn: Mutex<Connection>,
}

impl Storage {
    pub fn open(path: &Path) -> Result<Self, StorageError> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let conn = Connection::open(path)?;
        Self::init_connection(conn)
    }

    pub fn open_in_memory() -> Result<Self, StorageError> {
        let conn = Connection::open_in_memory()?;
        Self::init_connection(conn)
    }

    fn init_connection(conn: Connection) -> Result<Self, StorageError> {
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;",
        )?;
        let storage = Self {
            conn: Mutex::new(conn),
        };
        storage.migrate()?;
        storage.seed_defaults()?;
        Ok(storage)
    }

    pub fn migrate(&self) -> Result<(), StorageError> {
        let conn = self.conn.lock().map_err(|_| StorageError::LockPoisoned)?;
        conn.execute_batch(INIT_MIGRATION_SQL)?;
        Ok(())
    }

    pub fn seed_defaults(&self) -> Result<(), StorageError> {
        let conn = self.conn.lock().map_err(|_| StorageError::LockPoisoned)?;
        let now = unix_timestamp_secs();

        for provider in default_providers() {
            let (id, kind, name, base_url, api_key_ref, model) = split_provider_fields(&provider);
            conn.execute(
                "INSERT OR IGNORE INTO providers (id, kind, name, base_url, api_key_ref, model, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![id, kind, name, base_url, api_key_ref, model, now],
            )?;
        }

        const LEGACY_PROVIDER_UPGRADES: &[(&str, &str, &str, &str)] = &[
            ("prov-openai", "OpenAI · GPT-6.2 Sol", "gpt-6.2-sol", "gpt-4.1-mini"),
            (
                "prov-anthropic",
                "Anthropic · Claude Sonnet 5.5",
                "claude-sonnet-5-5",
                "claude-sonnet-4-5",
            ),
            (
                "provider-openrouter",
                "OpenRouter · Claude Sonnet 5.5",
                "anthropic/claude-sonnet-5.5",
                "anthropic/claude-sonnet-4",
            ),
            (
                "provider-deepseek",
                "DeepSeek · DeepSeek V4.1 Flash",
                "deepseek-v4.1-flash-1017",
                "deepseek-chat",
            ),
            (
                "provider-gemini",
                "Google · Gemini 3.8 Flash",
                "gemini-3.8-flash",
                "gemini-2.5-flash",
            ),
        ];

        for &(id, new_name, new_model, legacy_model) in LEGACY_PROVIDER_UPGRADES {
            conn.execute(
                "UPDATE providers
                 SET name = ?2, model = ?3, updated_at = ?5
                 WHERE id = ?1 AND model = ?4",
                params![id, new_name, new_model, legacy_model, now],
            )?;
        }

        for action in default_actions() {
            let render_mode = render_mode_to_str(action.render_mode);
            let is_built_in = if action.is_built_in { 1i64 } else { 0i64 };
            conn.execute(
                "INSERT OR IGNORE INTO custom_actions
                 (id, name, system_prompt, is_built_in, render_mode, html_template, css_template, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    action.id,
                    action.name,
                    action.system_prompt,
                    is_built_in,
                    render_mode,
                    action.html_template,
                    action.css_template,
                    now
                ],
            )?;
        }

        Ok(())
    }

    pub fn list_providers(&self) -> Result<Vec<ProviderConfig>, StorageError> {
        let conn = self.conn.lock().map_err(|_| StorageError::LockPoisoned)?;
        let mut stmt = conn.prepare(
            "SELECT id, kind, name, base_url, api_key_ref, model
             FROM providers
             ORDER BY rowid ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?;

        let mut providers = Vec::new();
        for row in rows {
            let (id, kind, name, base_url, api_key_ref, model) = row?;
            providers.push(build_provider(id, &kind, name, base_url, api_key_ref, model)?);
        }
        Ok(providers)
    }

    pub fn get_provider(&self, id: &str) -> Result<Option<ProviderConfig>, StorageError> {
        let conn = self.conn.lock().map_err(|_| StorageError::LockPoisoned)?;
        let row = conn
            .query_row(
                "SELECT id, kind, name, base_url, api_key_ref, model FROM providers WHERE id = ?1",
                params![id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                    ))
                },
            )
            .optional()?;

        match row {
            Some((id, kind, name, base_url, api_key_ref, model)) => {
                Ok(Some(build_provider(id, &kind, name, base_url, api_key_ref, model)?))
            }
            None => Ok(None),
        }
    }

    pub fn upsert_provider(&self, config: &ProviderConfig) -> Result<(), StorageError> {
        let (id, kind, name, base_url, api_key_ref, model) = split_provider_fields(config);
        if id.trim().is_empty() {
            return Err(StorageError::Validation("provider id cannot be empty".into()));
        }
        if name.trim().is_empty() {
            return Err(StorageError::Validation("provider name cannot be empty".into()));
        }
        if base_url.trim().is_empty() {
            return Err(StorageError::Validation("provider baseUrl cannot be empty".into()));
        }
        if model.trim().is_empty() {
            return Err(StorageError::Validation("provider model cannot be empty".into()));
        }

        let conn = self.conn.lock().map_err(|_| StorageError::LockPoisoned)?;
        conn.execute(
            "INSERT INTO providers (id, kind, name, base_url, api_key_ref, model, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET
               kind = excluded.kind,
               name = excluded.name,
               base_url = excluded.base_url,
               api_key_ref = excluded.api_key_ref,
               model = excluded.model,
               updated_at = excluded.updated_at",
            params![id, kind, name, base_url, api_key_ref, model, unix_timestamp_secs()],
        )?;
        Ok(())
    }

    pub fn set_api_key(&self, key_ref: &str, secret_value: &str) -> Result<(), StorageError> {
        let trimmed_ref = key_ref.trim();
        if trimmed_ref.is_empty() {
            return Err(StorageError::Validation("api key ref cannot be empty".into()));
        }
        let conn = self.conn.lock().map_err(|_| StorageError::LockPoisoned)?;
        let trimmed_secret = secret_value.trim();
        if trimmed_secret.is_empty() {
            conn.execute("DELETE FROM api_keys WHERE key_ref = ?1", params![trimmed_ref])?;
        } else {
            conn.execute(
                "INSERT INTO api_keys (key_ref, secret_value, updated_at)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(key_ref) DO UPDATE SET
                   secret_value = excluded.secret_value,
                   updated_at = excluded.updated_at",
                params![trimmed_ref, trimmed_secret, unix_timestamp_secs()],
            )?;
        }
        Ok(())
    }

    pub fn get_api_key(&self, key_ref: &str) -> Result<Option<String>, StorageError> {
        let trimmed_ref = key_ref.trim();
        if trimmed_ref.is_empty() {
            return Ok(None);
        }
        let conn = self.conn.lock().map_err(|_| StorageError::LockPoisoned)?;
        let secret = conn
            .query_row(
                "SELECT secret_value FROM api_keys WHERE key_ref = ?1",
                params![trimmed_ref],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        Ok(secret)
    }

    pub fn list_api_key_refs(&self) -> Result<Vec<String>, StorageError> {
        let conn = self.conn.lock().map_err(|_| StorageError::LockPoisoned)?;
        let mut stmt = conn.prepare("SELECT key_ref FROM api_keys ORDER BY key_ref ASC")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut refs = Vec::new();
        for row in rows {
            refs.push(row?);
        }
        Ok(refs)
    }

    pub fn list_actions(&self) -> Result<Vec<CustomAction>, StorageError> {
        let conn = self.conn.lock().map_err(|_| StorageError::LockPoisoned)?;
        let mut stmt = conn.prepare(
            "SELECT id, name, system_prompt, is_built_in, render_mode, html_template, css_template
             FROM custom_actions
             ORDER BY is_built_in DESC, rowid ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
            ))
        })?;

        let mut actions = Vec::new();
        for row in rows {
            let (id, name, system_prompt, is_built_in, render_mode, html_template, css_template) =
                row?;
            actions.push(CustomAction {
                id,
                name,
                system_prompt,
                is_built_in: is_built_in != 0,
                render_mode: str_to_render_mode(&render_mode)?,
                html_template,
                css_template,
            });
        }
        Ok(actions)
    }

    pub fn get_action(&self, id: &str) -> Result<Option<CustomAction>, StorageError> {
        let conn = self.conn.lock().map_err(|_| StorageError::LockPoisoned)?;
        let row = conn
            .query_row(
                "SELECT id, name, system_prompt, is_built_in, render_mode, html_template, css_template
                 FROM custom_actions WHERE id = ?1",
                params![id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, Option<String>>(5)?,
                        row.get::<_, Option<String>>(6)?,
                    ))
                },
            )
            .optional()?;

        match row {
            Some((id, name, system_prompt, is_built_in, render_mode, html_template, css_template)) => {
                Ok(Some(CustomAction {
                    id,
                    name,
                    system_prompt,
                    is_built_in: is_built_in != 0,
                    render_mode: str_to_render_mode(&render_mode)?,
                    html_template,
                    css_template,
                }))
            }
            None => Ok(None),
        }
    }

    pub fn upsert_action(&self, action: &CustomAction) -> Result<(), StorageError> {
        if action.id.trim().is_empty() {
            return Err(StorageError::Validation("action id cannot be empty".into()));
        }
        if action.name.trim().is_empty() {
            return Err(StorageError::Validation("action name cannot be empty".into()));
        }

        let conn = self.conn.lock().map_err(|_| StorageError::LockPoisoned)?;
        conn.execute(
            "INSERT INTO custom_actions
             (id, name, system_prompt, is_built_in, render_mode, html_template, css_template, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET
               name = excluded.name,
               system_prompt = excluded.system_prompt,
               is_built_in = excluded.is_built_in,
               render_mode = excluded.render_mode,
               html_template = excluded.html_template,
               css_template = excluded.css_template,
               updated_at = excluded.updated_at",
            params![
                action.id,
                action.name,
                action.system_prompt,
                if action.is_built_in { 1i64 } else { 0i64 },
                render_mode_to_str(action.render_mode),
                action.html_template,
                action.css_template,
                unix_timestamp_secs()
            ],
        )?;
        Ok(())
    }

    pub fn save_history(&self, entry: &HistoryEntryInput) -> Result<String, StorageError> {
        if entry.source_text.trim().is_empty() {
            return Err(StorageError::Validation("sourceText cannot be empty".into()));
        }
        let created_at = unix_timestamp_millis();
        let seq = CARD_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
        let id = format!("card-{created_at}-{seq:06}");

        let conn = self.conn.lock().map_err(|_| StorageError::LockPoisoned)?;
        conn.execute(
            "INSERT INTO history (id, source_text, provider_id, action_id, raw_json, latency_ms, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                id,
                entry.source_text,
                entry.provider_id,
                entry.action_id,
                entry.raw_json,
                entry.latency_ms as i64,
                created_at
            ],
        )?;
        Ok(id)
    }

    pub fn list_history(&self, limit: u32, offset: u32) -> Result<Vec<HistoryEntry>, StorageError> {
        let conn = self.conn.lock().map_err(|_| StorageError::LockPoisoned)?;
        let mut stmt = conn.prepare(
            "SELECT id, source_text, provider_id, action_id, raw_json, latency_ms, created_at
             FROM history
             ORDER BY created_at DESC, rowid DESC
             LIMIT ?1 OFFSET ?2",
        )?;
        let rows = stmt.query_map(params![i64::from(limit), i64::from(offset)], |row| {
            Ok(HistoryEntry {
                id: row.get(0)?,
                source_text: row.get(1)?,
                provider_id: row.get(2)?,
                action_id: row.get(3)?,
                raw_json: row.get(4)?,
                latency_ms: row.get::<_, i64>(5)?.max(0) as u64,
                created_at: row.get(6)?,
            })
        })?;

        let mut entries = Vec::new();
        for row in rows {
            entries.push(row?);
        }
        Ok(entries)
    }
}

pub fn default_providers() -> Vec<ProviderConfig> {
    vec![
        ProviderConfig::Ollama {
            id: "prov-ollama".into(),
            name: "Ollama · Qwen 2.5 14B".into(),
            base_url: "http://localhost:11434".into(),
            model: "qwen2.5:14b".into(),
        },
        ProviderConfig::OpenaiCompatible {
            id: "prov-lmstudio".into(),
            name: "LM Studio · Qwen 2.5 14B".into(),
            base_url: "http://localhost:1234/v1".into(),
            api_key_ref: String::new(),
            model: "qwen2.5-14b-instruct".into(),
        },
        ProviderConfig::OpenaiCompatible {
            id: "prov-openai".into(),
            name: "OpenAI · GPT-6.2 Sol".into(),
            base_url: "https://api.openai.com/v1".into(),
            api_key_ref: "key:openai".into(),
            model: "gpt-6.2-sol".into(),
        },
        ProviderConfig::OpenaiCompatible {
            id: "prov-openai-gpt6-luna".into(),
            name: "OpenAI · GPT-6 Luna".into(),
            base_url: "https://api.openai.com/v1".into(),
            api_key_ref: "key:openai".into(),
            model: "gpt-6-luna".into(),
        },
        ProviderConfig::OpenaiCompatible {
            id: "prov-openai-gpt62-astra".into(),
            name: "OpenAI · GPT-6.2 Astra".into(),
            base_url: "https://api.openai.com/v1".into(),
            api_key_ref: "key:openai".into(),
            model: "gpt-6.2-astra".into(),
        },
        ProviderConfig::Anthropic {
            id: "prov-anthropic".into(),
            name: "Anthropic · Claude Sonnet 5.5".into(),
            base_url: "https://api.anthropic.com".into(),
            api_key_ref: "key:anthropic".into(),
            model: "claude-sonnet-5-5".into(),
        },
        ProviderConfig::Anthropic {
            id: "prov-anthropic-haiku-55".into(),
            name: "Anthropic · Claude Haiku 5.5".into(),
            base_url: "https://api.anthropic.com".into(),
            api_key_ref: "key:anthropic".into(),
            model: "claude-haiku-5-5".into(),
        },
        ProviderConfig::Anthropic {
            id: "prov-anthropic-opus-55".into(),
            name: "Anthropic · Claude Opus 5.5".into(),
            base_url: "https://api.anthropic.com".into(),
            api_key_ref: "key:anthropic".into(),
            model: "claude-opus-5-5".into(),
        },
        ProviderConfig::OpenaiCompatible {
            id: "provider-deepseek".into(),
            name: "DeepSeek · DeepSeek V4.1 Flash".into(),
            base_url: "https://api.deepseek.com/v1".into(),
            api_key_ref: "deepseek".into(),
            model: "deepseek-v4.1-flash-1017".into(),
        },
        ProviderConfig::OpenaiCompatible {
            id: "prov-deepseek-v4-pro".into(),
            name: "DeepSeek · DeepSeek V4 Pro".into(),
            base_url: "https://api.deepseek.com/v1".into(),
            api_key_ref: "deepseek".into(),
            model: "deepseek-v4-pro".into(),
        },
        ProviderConfig::OpenaiCompatible {
            id: "provider-gemini".into(),
            name: "Google · Gemini 3.8 Flash".into(),
            base_url: "https://generativelanguage.googleapis.com/v1beta/openai".into(),
            api_key_ref: "gemini".into(),
            model: "gemini-3.8-flash".into(),
        },
        ProviderConfig::OpenaiCompatible {
            id: "prov-gemini-4-hydrogen".into(),
            name: "Google · Gemini 4 Hydrogen".into(),
            base_url: "https://generativelanguage.googleapis.com/v1beta/openai".into(),
            api_key_ref: "gemini".into(),
            model: "gemini-4-hydrogen".into(),
        },
        ProviderConfig::OpenaiCompatible {
            id: "prov-gemini-4-carbon".into(),
            name: "Google · Gemini 4 Carbon".into(),
            base_url: "https://generativelanguage.googleapis.com/v1beta/openai".into(),
            api_key_ref: "gemini".into(),
            model: "gemini-4-carbon".into(),
        },
        ProviderConfig::OpenaiCompatible {
            id: "provider-openrouter".into(),
            name: "OpenRouter · Claude Sonnet 5.5".into(),
            base_url: "https://openrouter.ai/api/v1".into(),
            api_key_ref: "openrouter".into(),
            model: "anthropic/claude-sonnet-5.5".into(),
        },
    ]
}

pub fn default_actions() -> Vec<CustomAction> {
    vec![
        CustomAction {
            id: "act-english-structure".into(),
            name: "English Structure Analysis".into(),
            system_prompt: "Analyze English sentence syntax using Xuan Yuan-yu's grammar framework and return valid StructuredGrammarCard JSON.".into(),
            is_built_in: true,
            render_mode: RenderMode::Unified3Layer,
            html_template: None,
            css_template: None,
        },
        CustomAction {
            id: "act-compact-html".into(),
            name: "Compact HTML Card".into(),
            system_prompt: "Provide a concise Simplified Chinese grammatical breakdown (sentence backbone, clauses, and natural translation) in clean plain text or HTML snippets.".into(),
            is_built_in: true,
            render_mode: RenderMode::CustomHtml,
            html_template: Some(
                "<section class=\"compact-card\">\n  <header class=\"compact-source\">{{sourceText}}</header>\n  <div class=\"compact-body\">{{content}}</div>\n</section>".into(),
            ),
            css_template: Some(
                ".compact-card { padding: 16px 18px; border-radius: 12px; border: 1px solid rgba(148,163,184,0.28); line-height: 1.65; }\n.compact-source { font-weight: 600; font-size: 15px; margin-bottom: 10px; }\n.compact-body { font-size: 13.5px; white-space: pre-wrap; opacity: 0.92; }".into(),
            ),
        },
    ]
}

fn split_provider_fields(
    config: &ProviderConfig,
) -> (&str, &'static str, &str, &str, &str, &str) {
    match config {
        ProviderConfig::Ollama {
            id,
            name,
            base_url,
            model,
        } => (id, "ollama", name, base_url, "", model),
        ProviderConfig::OpenaiCompatible {
            id,
            name,
            base_url,
            api_key_ref,
            model,
        } => (id, "openai-compatible", name, base_url, api_key_ref, model),
        ProviderConfig::Anthropic {
            id,
            name,
            base_url,
            api_key_ref,
            model,
        } => (id, "anthropic", name, base_url, api_key_ref, model),
    }
}

fn build_provider(
    id: String,
    kind: &str,
    name: String,
    base_url: String,
    api_key_ref: String,
    model: String,
) -> Result<ProviderConfig, StorageError> {
    match kind {
        "ollama" => Ok(ProviderConfig::Ollama {
            id,
            name,
            base_url,
            model,
        }),
        "openai-compatible" => Ok(ProviderConfig::OpenaiCompatible {
            id,
            name,
            base_url,
            api_key_ref,
            model,
        }),
        "anthropic" => Ok(ProviderConfig::Anthropic {
            id,
            name,
            base_url,
            api_key_ref,
            model,
        }),
        other => Err(StorageError::CorruptData(format!(
            "unknown provider kind: {other}"
        ))),
    }
}

fn render_mode_to_str(mode: RenderMode) -> &'static str {
    match mode {
        RenderMode::Unified3Layer => "unified-3layer",
        RenderMode::CustomHtml => "custom-html",
    }
}

fn str_to_render_mode(raw: &str) -> Result<RenderMode, StorageError> {
    match raw {
        "unified-3layer" => Ok(RenderMode::Unified3Layer),
        "custom-html" => Ok(RenderMode::CustomHtml),
        other => Err(StorageError::CorruptData(format!(
            "unknown render mode: {other}"
        ))),
    }
}

fn unix_timestamp_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn unix_timestamp_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn migration_and_seeding_are_idempotent() {
        let storage = Storage::open_in_memory().expect("open in-memory storage");
        storage.migrate().expect("second migrate must succeed");
        storage.seed_defaults().expect("second seed must succeed");

        let providers = storage.list_providers().expect("list providers");
        assert_eq!(providers.len(), 14);
        assert!(providers.iter().any(|p| matches!(p, ProviderConfig::Ollama { id, .. } if id == "prov-ollama")));
        assert!(providers.iter().any(|p| matches!(p, ProviderConfig::OpenaiCompatible { id, .. } if id == "prov-lmstudio")));
        assert!(providers.iter().any(|p| matches!(p, ProviderConfig::OpenaiCompatible { id, model, .. } if id == "prov-openai" && model == "gpt-6.2-sol")));
        assert!(providers.iter().any(|p| matches!(p, ProviderConfig::Anthropic { id, model, .. } if id == "prov-anthropic-haiku-55" && model == "claude-haiku-5-5")));
        assert_eq!(
            storage.get_provider("provider-openrouter").expect("get openrouter"),
            Some(ProviderConfig::OpenaiCompatible {
                id: "provider-openrouter".into(),
                name: "OpenRouter · Claude Sonnet 5.5".into(),
                base_url: "https://openrouter.ai/api/v1".into(),
                api_key_ref: "openrouter".into(),
                model: "anthropic/claude-sonnet-5.5".into(),
            })
        );
        assert_eq!(
            storage.get_provider("provider-deepseek").expect("get deepseek"),
            Some(ProviderConfig::OpenaiCompatible {
                id: "provider-deepseek".into(),
                name: "DeepSeek · DeepSeek V4.1 Flash".into(),
                base_url: "https://api.deepseek.com/v1".into(),
                api_key_ref: "deepseek".into(),
                model: "deepseek-v4.1-flash-1017".into(),
            })
        );
        assert_eq!(
            storage.get_provider("provider-gemini").expect("get gemini"),
            Some(ProviderConfig::OpenaiCompatible {
                id: "provider-gemini".into(),
                name: "Google · Gemini 3.8 Flash".into(),
                base_url: "https://generativelanguage.googleapis.com/v1beta/openai".into(),
                api_key_ref: "gemini".into(),
                model: "gemini-3.8-flash".into(),
            })
        );

        let actions = storage.list_actions().expect("list actions");
        assert_eq!(actions.len(), 2);
        assert!(actions.iter().any(|a| a.id == "act-english-structure" && a.render_mode == RenderMode::Unified3Layer));
        assert!(actions.iter().any(|a| a.id == "act-compact-html" && a.render_mode == RenderMode::CustomHtml));
    }

    #[test]
    fn upgrades_legacy_unedited_default_models_while_preserving_custom_edits() {
        let storage = Storage::open_in_memory().expect("open storage");

        storage
            .upsert_provider(&ProviderConfig::OpenaiCompatible {
                id: "prov-openai".into(),
                name: "Cloud OpenAI".into(),
                base_url: "https://api.openai.com/v1".into(),
                api_key_ref: "key:openai".into(),
                model: "gpt-4.1-mini".into(),
            })
            .expect("insert legacy openai");
        storage
            .upsert_provider(&ProviderConfig::Anthropic {
                id: "prov-anthropic".into(),
                name: "My Custom Claude".into(),
                base_url: "https://api.anthropic.com".into(),
                api_key_ref: "key:anthropic".into(),
                model: "my-custom-claude-endpoint".into(),
            })
            .expect("insert custom anthropic");

        storage.seed_defaults().expect("re-run seed_defaults");

        assert_eq!(
            storage.get_provider("prov-openai").expect("get openai"),
            Some(ProviderConfig::OpenaiCompatible {
                id: "prov-openai".into(),
                name: "OpenAI · GPT-6.2 Sol".into(),
                base_url: "https://api.openai.com/v1".into(),
                api_key_ref: "key:openai".into(),
                model: "gpt-6.2-sol".into(),
            })
        );
        assert_eq!(
            storage.get_provider("prov-anthropic").expect("get anthropic"),
            Some(ProviderConfig::Anthropic {
                id: "prov-anthropic".into(),
                name: "My Custom Claude".into(),
                base_url: "https://api.anthropic.com".into(),
                api_key_ref: "key:anthropic".into(),
                model: "my-custom-claude-endpoint".into(),
            })
        );
    }

    #[test]
    fn crud_round_trips_for_providers_api_keys_actions_and_history() {
        let storage = Storage::open_in_memory().expect("open storage");

        let custom_provider = ProviderConfig::OpenaiCompatible {
            id: "prov-deepseek".into(),
            name: "DeepSeek".into(),
            base_url: "https://api.deepseek.com/v1".into(),
            api_key_ref: "key:deepseek".into(),
            model: "deepseek-chat".into(),
        };
        storage
            .upsert_provider(&custom_provider)
            .expect("insert provider");
        assert_eq!(
            storage.get_provider("prov-deepseek").expect("get provider"),
            Some(custom_provider)
        );

        let updated_provider = ProviderConfig::OpenaiCompatible {
            id: "prov-deepseek".into(),
            name: "DeepSeek V3".into(),
            base_url: "https://api.deepseek.com/v1".into(),
            api_key_ref: "key:deepseek".into(),
            model: "deepseek-reasoner".into(),
        };
        storage
            .upsert_provider(&updated_provider)
            .expect("update provider");
        assert_eq!(
            storage.get_provider("prov-deepseek").expect("get updated"),
            Some(updated_provider)
        );

        storage
            .set_api_key("key:deepseek", "sk-secret-123")
            .expect("set api key");
        assert_eq!(
            storage.get_api_key("key:deepseek").expect("get api key"),
            Some("sk-secret-123".to_string())
        );
        assert_eq!(
            storage.list_api_key_refs().expect("list refs"),
            vec!["key:deepseek".to_string()]
        );

        let custom_action = CustomAction {
            id: "act-vocab".into(),
            name: "Clause Highlighter".into(),
            system_prompt: "Highlight clauses.".into(),
            is_built_in: false,
            render_mode: RenderMode::CustomHtml,
            html_template: Some("<article>{{content}}</article>".into()),
            css_template: Some("article { padding: 8px; }".into()),
        };
        storage.upsert_action(&custom_action).expect("insert action");
        assert_eq!(
            storage.get_action("act-vocab").expect("get action"),
            Some(custom_action)
        );

        let first_id = storage
            .save_history(&HistoryEntryInput {
                source_text: "The book Maya recommended won a prize.".into(),
                provider_id: "prov-ollama".into(),
                action_id: "act-english-structure".into(),
                raw_json: "{\"meaning\":\"玛雅推荐的书获奖了。\"}".into(),
                latency_ms: 340,
            })
            .expect("save first history");
        let second_id = storage
            .save_history(&HistoryEntryInput {
                source_text: "While walking home, the rain started.".into(),
                provider_id: "prov-openai".into(),
                action_id: "act-english-structure".into(),
                raw_json: "{\"meaning\":\"走回家时下雨了。\"}".into(),
                latency_ms: 510,
            })
            .expect("save second history");

        let history = storage.list_history(10, 0).expect("list history");
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].id, second_id);
        assert_eq!(history[1].id, first_id);
        assert_eq!(history[0].latency_ms, 510);

        let paged = storage.list_history(1, 1).expect("paged history");
        assert_eq!(paged.len(), 1);
        assert_eq!(paged[0].id, first_id);
    }

    #[test]
    fn persists_state_across_db_reopen() {
        let tmp_dir = std::env::temp_dir().join(format!(
            "grammatical_analysis_storage_test_{}_{}",
            std::process::id(),
            unix_timestamp_millis()
        ));
        let db_path = tmp_dir.join("app.db");

        {
            let storage = Storage::open(&db_path).expect("open db first time");
            storage
                .upsert_provider(&ProviderConfig::Ollama {
                    id: "prov-ollama".into(),
                    name: "Local Ollama Custom".into(),
                    base_url: "http://127.0.0.1:11434".into(),
                    model: "qwen2.5:7b".into(),
                })
                .expect("update ollama");
            storage
                .set_api_key("key:openai", "sk-persisted")
                .expect("save key");
            storage
                .save_history(&HistoryEntryInput {
                    source_text: "Persisted sentence.".into(),
                    provider_id: "prov-ollama".into(),
                    action_id: "act-english-structure".into(),
                    raw_json: "{\"meaning\":\"持久化的句子。\"}".into(),
                    latency_ms: 210,
                })
                .expect("save history");
        }

        {
            let reopened = Storage::open(&db_path).expect("reopen db");
            let ollama = reopened
                .get_provider("prov-ollama")
                .expect("read ollama")
                .expect("ollama exists");
            assert_eq!(
                ollama,
                ProviderConfig::Ollama {
                    id: "prov-ollama".into(),
                    name: "Local Ollama Custom".into(),
                    base_url: "http://127.0.0.1:11434".into(),
                    model: "qwen2.5:7b".into(),
                }
            );
            assert_eq!(
                reopened.get_api_key("key:openai").expect("read key"),
                Some("sk-persisted".to_string())
            );
            let history = reopened.list_history(10, 0).expect("read history");
            assert_eq!(history.len(), 1);
            assert_eq!(history[0].source_text, "Persisted sentence.");
        }

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    #[test]
    fn history_query_benchmark_1000_rows_under_15ms_p95() {
        let storage = Storage::open_in_memory().expect("open benchmark db");
        {
            let conn = storage.conn.lock().expect("lock conn");
            let tx = conn.unchecked_transaction().expect("begin tx");
            {
                let mut stmt = tx
                    .prepare(
                        "INSERT INTO history (id, source_text, provider_id, action_id, raw_json, latency_ms, created_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    )
                    .expect("prepare bulk insert");
                for i in 0..1000i64 {
                    let id = format!("card-bench-{i:04}");
                    let source = format!(
                        "The research paper that the committee reviewed in session {i} changed our understanding."
                    );
                    let payload = format!(
                        "{{\"kind\":\"structured\",\"sourceText\":\"{source}\",\"meaning\":\"第 {i} 篇论文改变了我们的理解。\",\"parts\":[],\"recombination\":\"主干 + 定语从句\",\"caveatNote\":null,\"bookRef\":null}}"
                    );
                    stmt.execute(params![
                        id,
                        source,
                        "prov-ollama",
                        "act-english-structure",
                        payload,
                        250 + (i % 400),
                        1_700_000_000_000i64 + i * 1000
                    ])
                    .expect("insert benchmark row");
                }
            }
            tx.commit().expect("commit 1000 rows");
        }

        let warm = storage.list_history(50, 0).expect("warm-up query");
        assert_eq!(warm.len(), 50);
        assert_eq!(warm[0].id, "card-bench-0999");

        let mut samples_ms = Vec::with_capacity(20);
        for run in 0..20u32 {
            let offset = (run % 10) * 20;
            let started = Instant::now();
            let rows = storage.list_history(50, offset).expect("benchmark query");
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            assert_eq!(rows.len(), 50);
            samples_ms.push(elapsed_ms);
        }

        samples_ms.sort_by(|a, b| a.total_cmp(b));
        let min_ms = samples_ms[0];
        let p50_ms = samples_ms[samples_ms.len() / 2];
        let p95_idx = ((samples_ms.len() as f64) * 0.95).ceil() as usize - 1;
        let p95_ms = samples_ms[p95_idx.min(samples_ms.len() - 1)];
        let max_ms = *samples_ms.last().unwrap();

        println!(
            "[bench] history 1000-row query (20 runs): min={min_ms:.3}ms p50={p50_ms:.3}ms p95={p95_ms:.3}ms max={max_ms:.3}ms"
        );
        assert!(
            p95_ms <= 15.0,
            "1000-row history query p95 {p95_ms:.3}ms exceeded 15.0ms threshold"
        );
    }

    #[test]
    fn list_history_preserves_newest_first_order_within_same_millisecond() {
        let storage = Storage::open_in_memory().expect("in-memory db");
        let mut saved_ids = Vec::new();
        for i in 1..=15 {
            let id = storage
                .save_history(&HistoryEntryInput {
                    source_text: format!("Sentence {i}."),
                    provider_id: "prov-ollama".into(),
                    action_id: "act-english-structure".into(),
                    raw_json: "{}".into(),
                    latency_ms: i,
                })
                .expect("save history");
            saved_ids.push(id);
        }

        let listed = storage.list_history(20, 0).expect("list history");
        let listed_ids: Vec<String> = listed.into_iter().map(|e| e.id).collect();
        let expected_ids: Vec<String> = saved_ids.into_iter().rev().collect();
        assert_eq!(listed_ids, expected_ids);
    }
}
