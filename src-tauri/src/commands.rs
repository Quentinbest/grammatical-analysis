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

#[derive(Debug, Clone)]
enum InFlightEntry {
    Active(CancellationToken),
    PreCancelled,
}

const MAX_PRE_CANCELLED_ENTRIES: usize = 64;

pub struct AppState {
    pub storage: Arc<Storage>,
    resource_dir: Option<PathBuf>,
    retriever: OnceLock<Option<Bm25BookRetriever>>,
    prompt_builder: OnceLock<Option<PromptBuilder>>,
    in_flight: Mutex<HashMap<String, InFlightEntry>>,
}

impl AppState {
    pub fn new(storage: Arc<Storage>) -> Self {
        Self::with_resource_dir(storage, None)
    }

    pub fn with_resource_dir(storage: Arc<Storage>, resource_dir: Option<PathBuf>) -> Self {
        Self {
            storage,
            resource_dir,
            retriever: OnceLock::new(),
            prompt_builder: OnceLock::new(),
            in_flight: Mutex::new(HashMap::new()),
        }
    }

    pub fn retrieve_context(&self, source_text: &str, limit: usize) -> Vec<RetrievalHit> {
        let retriever = self.retriever.get_or_init(|| {
            resolve_resource_dir(self.resource_dir.as_deref(), "knowledge/book")
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
                resolve_resource_dir(self.resource_dir.as_deref(), "prompts")
                    .and_then(|dir| PromptBuilder::from_dir(&dir, PromptConfig::default()).ok())
            })
            .as_ref()
    }

    fn register_in_flight(&self, request_id: &str, token: CancellationToken) {
        if let Ok(mut map) = self.in_flight.lock() {
            match map.insert(request_id.to_string(), InFlightEntry::Active(token.clone())) {
                Some(InFlightEntry::PreCancelled) => {
                    token.cancel();
                }
                Some(InFlightEntry::Active(prev)) => {
                    prev.cancel();
                }
                None => {}
            }
        }
    }

    fn cancel_in_flight(&self, request_id: &str) -> bool {
        if let Ok(mut map) = self.in_flight.lock() {
            match map.remove(request_id) {
                Some(InFlightEntry::Active(token)) => {
                    token.cancel();
                    return true;
                }
                Some(InFlightEntry::PreCancelled) => {
                    map.insert(request_id.to_string(), InFlightEntry::PreCancelled);
                    return true;
                }
                None => {
                    let pre_cancelled_count = map
                        .values()
                        .filter(|v| matches!(v, InFlightEntry::PreCancelled))
                        .count();
                    if pre_cancelled_count >= MAX_PRE_CANCELLED_ENTRIES {
                        map.retain(|_, v| !matches!(v, InFlightEntry::PreCancelled));
                    }
                    map.insert(request_id.to_string(), InFlightEntry::PreCancelled);
                    return true;
                }
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

fn resolve_resource_dir(runtime_resource_dir: Option<&Path>, relative: &str) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(res_dir) = runtime_resource_dir {
        candidates.push(res_dir.join(relative));
    }
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
    let key_ref = match provider {
        ProviderConfig::Ollama { .. } => return Ok(None),
        ProviderConfig::OpenaiCompatible { api_key_ref, .. }
        | ProviderConfig::Anthropic { api_key_ref, .. } => api_key_ref.as_str(),
    };

    let trimmed = key_ref.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("none") {
        return Ok(None);
    }

    if let Some(stored) = storage.get_api_key(trimmed).map_err(|e| e.to_string())? {
        return Ok(Some(stored));
    }

    let env_fallback = match (provider, trimmed.to_ascii_lowercase().as_str()) {
        (ProviderConfig::OpenaiCompatible { .. }, "openai" | "key:openai") => {
            Some("OPENAI_API_KEY")
        }
        (ProviderConfig::Anthropic { .. }, "anthropic" | "key:anthropic") => {
            Some("ANTHROPIC_API_KEY")
        }
        (_, "openrouter" | "key:openrouter") => Some("OPENROUTER_API_KEY"),
        (_, "deepseek" | "key:deepseek") => Some("DEEPSEEK_API_KEY"),
        (_, "gemini" | "key:gemini") => Some("GEMINI_API_KEY"),
        _ => None,
    };

    Ok(env_fallback.and_then(|var| {
        std::env::var(var)
            .ok()
            .filter(|v| !v.trim().is_empty())
    }))
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

    let cancel = CancellationToken::new();
    state.register_in_flight(&request.request_id, cancel.clone());
    let _guard = InFlightGuard {
        state: &state,
        request_id: request.request_id.clone(),
    };

    if cancel.is_cancelled() {
        let _ = channel.send(AnalysisStreamEvent::Failed {
            code: "cancelled".into(),
            message: "analysis request cancelled".into(),
        });
        return Ok(());
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
    if cancel.is_cancelled() {
        let _ = channel.send(AnalysisStreamEvent::Failed {
            code: "cancelled".into(),
            message: "analysis request cancelled".into(),
        });
        return Ok(());
    }
    let _ = channel.send(AnalysisStreamEvent::RetrievalReady { hits: hits.clone() });

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_api_key_does_not_leak_openai_env_to_custom_provider() {
        let storage = Storage::open_in_memory().expect("in-memory db");
        let custom_provider = ProviderConfig::OpenaiCompatible {
            id: "prov-custom-thirdparty".into(),
            name: "Third Party".into(),
            base_url: "https://thirdparty.example/v1".into(),
            api_key_ref: "key:custom-thirdparty".into(),
            model: "custom-model".into(),
        };

        std::env::set_var("OPENAI_API_KEY", "sk-openai-personal-secret");
        let resolved = resolve_api_key(&storage, &custom_provider).expect("resolve");
        assert_eq!(resolved, None);

        let openai_provider = ProviderConfig::OpenaiCompatible {
            id: "prov-openai".into(),
            name: "Cloud OpenAI".into(),
            base_url: "https://api.openai.com/v1".into(),
            api_key_ref: "key:openai".into(),
            model: "gpt-4.1-mini".into(),
        };
        let resolved_openai = resolve_api_key(&storage, &openai_provider).expect("resolve openai");
        assert_eq!(
            resolved_openai.as_deref(),
            Some("sk-openai-personal-secret")
        );
    }

    #[test]
    fn cancel_in_flight_before_register_cancels_subsequent_token() {
        let storage = Arc::new(Storage::open_in_memory().expect("in-memory db"));
        let state = AppState::new(storage);

        assert!(state.cancel_in_flight("req-early-cancel"));
        let token = CancellationToken::new();
        state.register_in_flight("req-early-cancel", token.clone());
        assert!(token.is_cancelled());
        state.remove_in_flight("req-early-cancel");
    }

    #[test]
    fn resolve_resource_dir_prefers_runtime_resource_dir() {
        let temp_root = std::env::temp_dir().join(format!(
            "ga-resource-dir-test-{}",
            std::process::id()
        ));
        let prompts_dir = temp_root.join("prompts");
        std::fs::create_dir_all(&prompts_dir).expect("create temp prompts dir");

        let resolved = resolve_resource_dir(Some(&temp_root), "prompts");
        assert_eq!(resolved.as_deref(), Some(prompts_dir.as_path()));

        let _ = std::fs::remove_dir_all(&temp_root);
    }
}
