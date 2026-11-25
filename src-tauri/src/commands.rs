use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use serde::Deserialize;
use tauri::ipc::Channel;
use tauri::State;

use crate::domain::types::{
    AnalysisStreamEvent, CustomAction, ProviderConfig, RenderMode, RetrievalHit,
};
use crate::llm::{create_provider, AnalysisPromptRequest, CancellationToken, LlmProvider};
use crate::os::clipboard::{ClipboardPort, SystemClipboard};
use crate::retrieval::{
    load_default_retriever, retrieve_book_context, Bm25BookRetriever, PromptBuilder, PromptConfig,
};
use crate::storage::{HistoryEntry, HistoryEntryInput, Storage};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunAnalysisInput {
    pub request_id: String,
    pub source_text: String,
    pub provider_id: String,
    pub action_id: String,
}

pub struct AppState {
    pub storage: Arc<Storage>,
    retriever: OnceLock<Option<Bm25BookRetriever>>,
    prompt_builder: OnceLock<Option<PromptBuilder>>,
    in_flight: Mutex<HashMap<String, CancellationToken>>,
}

impl AppState {
    pub fn new(storage: Arc<Storage>) -> Self {
        Self {
            storage,
            retriever: OnceLock::new(),
            prompt_builder: OnceLock::new(),
            in_flight: Mutex::new(HashMap::new()),
        }
    }

    pub fn retrieve_context(&self, source_text: &str, limit: usize) -> Vec<RetrievalHit> {
        let retriever = self.retriever.get_or_init(|| {
            resolve_resource_dir("knowledge/book")
                .and_then(|dir| load_default_retriever(&dir).ok())
        });
        match retriever {
            Some(index) => retrieve_book_context(index, source_text, limit),
            None => Vec::new(),
        }
    }

    fn prompt_builder(&self) -> Option<&PromptBuilder> {
        self.prompt_builder
            .get_or_init(|| {
                resolve_resource_dir("prompts")
                    .and_then(|dir| PromptBuilder::from_dir(&dir, PromptConfig::default()).ok())
            })
            .as_ref()
    }

    fn register_in_flight(&self, request_id: &str, token: CancellationToken) {
        if let Ok(mut map) = self.in_flight.lock() {
            if let Some(prev) = map.insert(request_id.to_string(), token) {
                prev.cancel();
            }
        }
    }

    fn cancel_in_flight(&self, request_id: &str) -> bool {
        if let Ok(mut map) = self.in_flight.lock() {
            if let Some(token) = map.remove(request_id) {
                token.cancel();
                return true;
            }
        }
        false
    }

    fn remove_in_flight(&self, request_id: &str) {
        if let Ok(mut map) = self.in_flight.lock() {
            map.remove(request_id);
        }
    }
}

struct InFlightGuard<'a> {
    state: &'a AppState,
    request_id: String,
}

impl Drop for InFlightGuard<'_> {
    fn drop(&mut self) {
        self.state.remove_in_flight(&self.request_id);
    }
}

fn resolve_resource_dir(relative: &str) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join(relative));
        if let Some(parent) = cwd.parent() {
            candidates.push(parent.join(relative));
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join(relative));
            if let Some(res) = dir.parent().map(|p| p.join("Resources").join(relative)) {
                candidates.push(res);
            }
        }
    }
    let manifest_parent = Path::new(env!("CARGO_MANIFEST_DIR")).parent();
    if let Some(root) = manifest_parent {
        candidates.push(root.join(relative));
    }

    candidates.into_iter().find(|p| p.is_dir())
}

fn resolve_api_key(storage: &Storage, provider: &ProviderConfig) -> Result<Option<String>, String> {
    let (key_ref, default_env) = match provider {
        ProviderConfig::Ollama { .. } => return Ok(None),
        ProviderConfig::OpenaiCompatible { api_key_ref, .. } => {
            (api_key_ref.as_str(), "OPENAI_API_KEY")
        }
        ProviderConfig::Anthropic { api_key_ref, .. } => {
            (api_key_ref.as_str(), "ANTHROPIC_API_KEY")
        }
    };

    if key_ref.trim().is_empty() || key_ref.eq_ignore_ascii_case("none") {
        return Ok(None);
    }

    if let Some(stored) = storage.get_api_key(key_ref).map_err(|e| e.to_string())? {
        return Ok(Some(stored));
    }

    let env_fallback = match key_ref.trim().to_ascii_lowercase().as_str() {
        "openrouter" | "key:openrouter" => "OPENROUTER_API_KEY",
        "deepseek" | "key:deepseek" => "DEEPSEEK_API_KEY",
        "gemini" | "key:gemini" => "GEMINI_API_KEY",
        _ => default_env,
    };

    Ok(std::env::var(env_fallback)
        .ok()
        .filter(|v| !v.trim().is_empty()))
}

fn build_prompt_request(
    state: &AppState,
    action: &CustomAction,
    source_text: &str,
    hits: &[RetrievalHit],
    cancel: CancellationToken,
) -> AnalysisPromptRequest {
    let (system_prompt, user_prompt) = match action.render_mode {
        RenderMode::Unified3Layer => {
            if let Some(builder) = state.prompt_builder() {
                let assembled = builder.build(source_text, hits);
                let custom = action.system_prompt.trim();
                let sys = if custom.is_empty() {
                    assembled.system_prompt
                } else {
                    format!("{custom}\n\n{}", assembled.system_prompt)
                };
                (sys, assembled.user_prompt)
            } else {
                (
                    action.system_prompt.clone(),
                    format!("Analyze the following English text and return ONLY valid JSON:\n\n{source_text}"),
                )
            }
        }
        RenderMode::CustomHtml => {
            let mut sys = action.system_prompt.clone();
            if !hits.is_empty() {
                sys.push_str("\n\n## Textbook Reference Excerpts");
                for hit in hits {
                    sys.push_str(&format!(
                        "\n- [{} | {}] {}",
                        hit.chapter, hit.section, hit.excerpt
                    ));
                }
            }
            (sys, source_text.trim().to_string())
        }
    };

    let mut req = AnalysisPromptRequest::new(system_prompt, user_prompt);
    req.cancel = cancel;
    req
}

#[tauri::command]
pub async fn run_analysis(
    request: RunAnalysisInput,
    channel: Channel<AnalysisStreamEvent>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let source_text = request.source_text.trim();
    if source_text.is_empty() {
        let msg = "sourceText cannot be empty".to_string();
        let _ = channel.send(AnalysisStreamEvent::Failed {
            code: "invalid-input".into(),
            message: msg.clone(),
        });
        return Err(msg);
    }

    let provider = state
        .storage
        .get_provider(&request.provider_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("provider not found: {}", request.provider_id))?;

    let action = state
        .storage
        .get_action(&request.action_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("action not found: {}", request.action_id))?;

    let api_key = resolve_api_key(&state.storage, &provider)?;
    let hits = state.retrieve_context(source_text, 4);
    let _ = channel.send(AnalysisStreamEvent::RetrievalReady { hits: hits.clone() });

    let cancel = CancellationToken::new();
    state.register_in_flight(&request.request_id, cancel.clone());
    let _guard = InFlightGuard {
        state: &state,
        request_id: request.request_id.clone(),
    };

    let prompt_req = build_prompt_request(&state, &action, source_text, &hits, cancel);
    let client = create_provider(&provider, api_key).map_err(|err| {
        let _ = channel.send(err.to_failed_event());
        err.to_string()
    })?;

    client
        .stream_analysis(prompt_req, |event| {
            let _ = channel.send(event);
        })
        .await
        .map_err(|err| err.to_string())
}

#[tauri::command]
pub fn cancel_analysis(request_id: String, state: State<'_, AppState>) -> Result<(), String> {
    let _ = state.cancel_in_flight(&request_id);
    Ok(())
}

#[tauri::command]
pub fn list_providers(state: State<'_, AppState>) -> Result<Vec<ProviderConfig>, String> {
    state.storage.list_providers().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn upsert_provider(config: ProviderConfig, state: State<'_, AppState>) -> Result<(), String> {
    state
        .storage
        .upsert_provider(&config)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_api_key(
    key_ref: String,
    secret_value: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state
        .storage
        .set_api_key(&key_ref, &secret_value)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_api_key_refs(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    state.storage.list_api_key_refs().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_actions(state: State<'_, AppState>) -> Result<Vec<CustomAction>, String> {
    state.storage.list_actions().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn upsert_action(action: CustomAction, state: State<'_, AppState>) -> Result<(), String> {
    state
        .storage
        .upsert_action(&action)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_history(
    entry: HistoryEntryInput,
    state: State<'_, AppState>,
) -> Result<String, String> {
    state
        .storage
        .save_history(&entry)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_history(
    limit: u32,
    offset: u32,
    state: State<'_, AppState>,
) -> Result<Vec<HistoryEntry>, String> {
    state
        .storage
        .list_history(limit, offset)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn read_clipboard_text() -> Result<Option<String>, String> {
    SystemClipboard.read_text().map_err(|e| e.to_string())
}
