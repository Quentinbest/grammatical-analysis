use std::future::Future;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;

use futures_util::StreamExt;
use tokio::sync::Notify;
use tokio::time::Instant;

use crate::domain::types::{AnalysisStreamEvent, ProviderConfig};

use super::anthropic::AnthropicProvider;
use super::ollama::OllamaProvider;
use super::openai_compat::OpenAiCompatProvider;

/// Cooperative, cloneable cancellation token for interrupting in-flight LLM streams.
#[derive(Debug, Clone, Default)]
pub struct CancellationToken {
    inner: Arc<CancellationInner>,
}

#[derive(Debug, Default)]
struct CancellationInner {
    cancelled: AtomicBool,
    notify: Notify,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        if !self.inner.cancelled.swap(true, Ordering::Release) {
            self.inner.notify.notify_waiters();
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::Acquire)
    }

    pub async fn cancelled(&self) {
        let notified = self.inner.notify.notified();
        if self.is_cancelled() {
            return;
        }
        notified.await;
    }
}

/// Prompt payload and execution controls passed to any `LlmProvider`.
#[derive(Debug, Clone)]
pub struct AnalysisPromptRequest {
    pub system_prompt: String,
    pub user_prompt: String,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub timeout: Duration,
    pub cancel: CancellationToken,
}

impl AnalysisPromptRequest {
    pub fn new(system_prompt: impl Into<String>, user_prompt: impl Into<String>) -> Self {
        Self {
            system_prompt: system_prompt.into(),
            user_prompt: user_prompt.into(),
            temperature: Some(0.2),
            max_tokens: Some(2048),
            timeout: Duration::from_secs(60),
            cancel: CancellationToken::new(),
        }
    }
}

/// Structured error categories emitted by local and cloud LLM providers.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProviderError {
    #[error("unauthorized (HTTP {status}): {message}")]
    Unauthorized { status: u16, message: String },

    #[error("rate limited (HTTP {status}): {message}")]
    RateLimited { status: u16, message: String },

    #[error("request timed out after {timeout_ms}ms")]
    Timeout { timeout_ms: u64 },

    #[error("request cancelled")]
    Cancelled,

    #[error("network error: {message}")]
    Network { message: String },

    #[error("protocol error: {message}")]
    Protocol { message: String },
}

impl ProviderError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unauthorized { .. } => "unauthorized",
            Self::RateLimited { .. } => "rate-limited",
            Self::Timeout { .. } => "timeout",
            Self::Cancelled => "cancelled",
            Self::Network { .. } => "network",
            Self::Protocol { .. } => "protocol",
        }
    }

    pub fn to_failed_event(&self) -> AnalysisStreamEvent {
        AnalysisStreamEvent::Failed {
            code: self.code().to_string(),
            message: self.to_string(),
        }
    }
}

/// Unified streaming interface implemented by all local and cloud LLM providers.
pub trait LlmProvider: Send + Sync {
    fn stream_analysis(
        &self,
        request: AnalysisPromptRequest,
        on_event: impl Fn(AnalysisStreamEvent) + Send + Sync,
    ) -> impl Future<Output = Result<(), ProviderError>> + Send;
}

/// Equal-status provider dispatcher covering Ollama, OpenAI-compatible, and Anthropic.
#[derive(Debug, Clone)]
pub enum ProviderClient {
    Ollama(OllamaProvider),
    OpenaiCompatible(OpenAiCompatProvider),
    Anthropic(AnthropicProvider),
}

impl LlmProvider for ProviderClient {
    fn stream_analysis(
        &self,
        request: AnalysisPromptRequest,
        on_event: impl Fn(AnalysisStreamEvent) + Send + Sync,
    ) -> impl Future<Output = Result<(), ProviderError>> + Send {
        async move {
            match self {
                Self::Ollama(provider) => provider.stream_analysis(request, on_event).await,
                Self::OpenaiCompatible(provider) => provider.stream_analysis(request, on_event).await,
                Self::Anthropic(provider) => provider.stream_analysis(request, on_event).await,
            }
        }
    }
}

/// Construct an equal-status `ProviderClient` from a `ProviderConfig` and optional resolved API key.
pub fn create_provider(
    config: &ProviderConfig,
    api_key: Option<String>,
) -> Result<ProviderClient, ProviderError> {
    let client = build_http_client()?;
    let normalized_key = api_key.filter(|k| !k.trim().is_empty());

    match config {
        ProviderConfig::Ollama {
            base_url, model, ..
        } => Ok(ProviderClient::Ollama(OllamaProvider::new(
            client,
            base_url.clone(),
            model.clone(),
        ))),
        ProviderConfig::OpenaiCompatible {
            base_url, model, ..
        } => Ok(ProviderClient::OpenaiCompatible(OpenAiCompatProvider::new(
            client,
            base_url.clone(),
            model.clone(),
            normalized_key,
        ))),
        ProviderConfig::Anthropic {
            base_url, model, ..
        } => Ok(ProviderClient::Anthropic(AnthropicProvider::new(
            client,
            base_url.clone(),
            model.clone(),
            normalized_key,
        ))),
    }
}

pub(crate) fn build_http_client() -> Result<reqwest::Client, ProviderError> {
    reqwest::Client::builder()
        .no_proxy()
        .build()
        .map_err(|err| ProviderError::Network {
            message: format!("failed to build HTTP client: {err}"),
        })
}

pub(crate) fn join_endpoint(base_url: &str, suffix: &str) -> String {
    let base = base_url.trim_end_matches('/');
    let clean_suffix = suffix.trim_start_matches('/');
    if base.ends_with(clean_suffix) {
        base.to_string()
    } else {
        format!("{base}/{clean_suffix}")
    }
}

pub(crate) fn map_http_status(status: u16, body: &str) -> ProviderError {
    let detail = truncate_error_body(body);
    match status {
        401 | 403 => ProviderError::Unauthorized {
            status,
            message: detail,
        },
        429 => ProviderError::RateLimited {
            status,
            message: detail,
        },
        _ => ProviderError::Protocol {
            message: format!("HTTP {status}: {detail}"),
        },
    }
}

fn truncate_error_body(body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return "empty response body".to_string();
    }
    const MAX_LEN: usize = 512;
    if trimmed.len() <= MAX_LEN {
        trimmed.to_string()
    } else {
        let mut end = MAX_LEN;
        while !trimmed.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &trimmed[..end])
    }
}

pub(crate) fn emit_and_return_err(
    err: ProviderError,
    on_event: &(impl Fn(AnalysisStreamEvent) + Send + Sync),
) -> Result<(), ProviderError> {
    on_event(err.to_failed_event());
    Err(err)
}

/// Sends a prepared HTTP request and drives its response byte stream with unified
/// timeout and cooperative cancellation semantics.
///
/// `on_chunk` receives `Some(bytes)` for each network chunk (returning `Ok(true)` to
/// stop early once a terminal sentinel is seen) and `None` once at EOF to flush any
/// buffered parser state.
pub(crate) async fn send_and_stream<FChunk>(
    request_builder: reqwest::RequestBuilder,
    timeout: Duration,
    cancel: &CancellationToken,
    on_event: &(impl Fn(AnalysisStreamEvent) + Send + Sync),
    mut on_chunk: FChunk,
) -> Result<(), ProviderError>
where
    FChunk: FnMut(Option<&[u8]>) -> Result<bool, ProviderError>,
{
    if cancel.is_cancelled() {
        return emit_and_return_err(ProviderError::Cancelled, on_event);
    }

    let timeout_ms = timeout.as_millis() as u64;
    let deadline = Instant::now() + timeout;

    let response = tokio::select! {
        biased;
        _ = cancel.cancelled() => {
            return emit_and_return_err(ProviderError::Cancelled, on_event);
        }
        res = tokio::time::timeout_at(deadline, request_builder.send()) => {
            match res {
                Ok(Ok(resp)) => resp,
                Ok(Err(err)) => {
                    let provider_err = if err.is_timeout() {
                        ProviderError::Timeout { timeout_ms }
                    } else {
                        ProviderError::Network {
                            message: err.to_string(),
                        }
                    };
                    return emit_and_return_err(provider_err, on_event);
                }
                Err(_) => {
                    return emit_and_return_err(ProviderError::Timeout { timeout_ms }, on_event);
                }
            }
        }
    };

    let status = response.status();
    if !status.is_success() {
        let body = tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                return emit_and_return_err(ProviderError::Cancelled, on_event);
            }
            text = tokio::time::timeout_at(deadline, response.text()) => {
                match text {
                    Ok(Ok(t)) => t,
                    Ok(Err(err)) => format!("failed reading error body: {err}"),
                    Err(_) => return emit_and_return_err(ProviderError::Timeout { timeout_ms }, on_event),
                }
            }
        };
        return emit_and_return_err(map_http_status(status.as_u16(), &body), on_event);
    }

    let mut byte_stream = response.bytes_stream();
    loop {
        if cancel.is_cancelled() {
            return emit_and_return_err(ProviderError::Cancelled, on_event);
        }

        let next_item = tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                return emit_and_return_err(ProviderError::Cancelled, on_event);
            }
            item = tokio::time::timeout_at(deadline, byte_stream.next()) => {
                match item {
                    Ok(item) => item,
                    Err(_) => {
                        return emit_and_return_err(ProviderError::Timeout { timeout_ms }, on_event);
                    }
                }
            }
        };

        match next_item {
            Some(Ok(bytes)) => match on_chunk(Some(&bytes)) {
                Ok(true) => break,
                Ok(false) => {}
                Err(err) => return emit_and_return_err(err, on_event),
            },
            Some(Err(err)) => {
                let provider_err = if err.is_timeout() {
                    ProviderError::Timeout { timeout_ms }
                } else {
                    ProviderError::Network {
                        message: err.to_string(),
                    }
                };
                return emit_and_return_err(provider_err, on_event);
            }
            None => break,
        }
    }

    if let Err(err) = on_chunk(None) {
        return emit_and_return_err(err, on_event);
    }

    Ok(())
}
