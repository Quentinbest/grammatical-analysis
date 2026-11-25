use std::future::Future;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::domain::types::AnalysisStreamEvent;

use super::provider::{
    join_endpoint, send_and_stream, AnalysisPromptRequest, LlmProvider, ProviderError,
};
use super::sse::SseParser;

/// OpenAI-compatible streaming provider (LM Studio, OpenAI, Gemini compat, vLLM, etc.).
#[derive(Debug, Clone)]
pub struct OpenAiCompatProvider {
    client: reqwest::Client,
    base_url: String,
    model: String,
    api_key: Option<String>,
}

impl OpenAiCompatProvider {
    pub fn new(
        client: reqwest::Client,
        base_url: String,
        model: String,
        api_key: Option<String>,
    ) -> Self {
        Self {
            client,
            base_url,
            model,
            api_key,
        }
    }
}

#[derive(Serialize)]
struct ChatCompletionsRequest<'a> {
    model: &'a str,
    stream: bool,
    messages: [ChatMessage<'a>; 2],
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct ChatCompletionsChunk {
    #[serde(default)]
    choices: Option<Vec<ChunkChoice>>,
    #[serde(default)]
    error: Option<InlineErrorField>,
}

#[derive(Deserialize)]
struct ChunkChoice {
    #[serde(default)]
    delta: Option<ChunkDelta>,
}

#[derive(Default, Deserialize)]
struct ChunkDelta {
    #[serde(default)]
    content: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum InlineErrorField {
    Object(InlineError),
    Message(String),
}

#[derive(Deserialize)]
struct InlineError {
    #[serde(default)]
    message: Option<String>,
    #[serde(rename = "type", default)]
    error_type: Option<String>,
    #[serde(default)]
    code: Option<serde_json::Value>,
    #[serde(default)]
    status: Option<u16>,
}

fn map_inline_error(err: InlineError) -> ProviderError {
    let message = err
        .message
        .filter(|m| !m.trim().is_empty())
        .unwrap_or_else(|| "upstream stream error".to_string());

    let err_type = err
        .error_type
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let code_str = err
        .code
        .as_ref()
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let code_num = err
        .code
        .as_ref()
        .and_then(|v| v.as_u64())
        .and_then(|n| u16::try_from(n).ok());
    let status = err.status.or(code_num);

    if status == Some(429)
        || err_type == "rate_limit_error"
        || err_type.contains("rate_limit")
        || code_str == "rate_limit_exceeded"
        || code_str == "rate_limit_error"
        || code_str == "429"
    {
        return ProviderError::RateLimited {
            status: 429,
            message,
        };
    }

    if matches!(status, Some(401 | 403))
        || err_type == "authentication_error"
        || err_type == "unauthorized"
        || err_type == "permission_error"
        || code_str == "invalid_api_key"
        || code_str == "authentication_error"
        || code_str == "unauthorized"
        || code_str == "401"
        || code_str == "403"
    {
        return ProviderError::Unauthorized {
            status: status.filter(|s| matches!(s, 401 | 403)).unwrap_or(401),
            message,
        };
    }

    ProviderError::Protocol { message }
}

fn map_inline_error_field(field: InlineErrorField) -> ProviderError {
    match field {
        InlineErrorField::Object(err) => map_inline_error(err),
        InlineErrorField::Message(message) => ProviderError::Protocol {
            message: if message.trim().is_empty() {
                "upstream stream error".to_string()
            } else {
                message
            },
        },
    }
}

fn parse_sse_error_frame(data: &str) -> ProviderError {
    let trimmed = data.trim();
    if trimmed.is_empty() {
        return ProviderError::Protocol {
            message: "upstream SSE error event".to_string(),
        };
    }

    if let Ok(chunk) = serde_json::from_str::<ChatCompletionsChunk>(trimmed) {
        if let Some(inline) = chunk.error {
            return map_inline_error_field(inline);
        }
    }

    if let Ok(inline) = serde_json::from_str::<InlineError>(trimmed) {
        if inline.message.is_some()
            || inline.error_type.is_some()
            || inline.code.is_some()
            || inline.status.is_some()
        {
            return map_inline_error(inline);
        }
    }

    ProviderError::Protocol {
        message: trimmed.to_string(),
    }
}

impl LlmProvider for OpenAiCompatProvider {
    fn stream_analysis(
        &self,
        request: AnalysisPromptRequest,
        on_event: impl Fn(AnalysisStreamEvent) + Send + Sync,
    ) -> impl Future<Output = Result<(), ProviderError>> + Send {
        async move {
            let started = Instant::now();
            let url = join_endpoint(&self.base_url, "chat/completions");

            let body = ChatCompletionsRequest {
                model: &self.model,
                stream: true,
                messages: [
                    ChatMessage {
                        role: "system",
                        content: &request.system_prompt,
                    },
                    ChatMessage {
                        role: "user",
                        content: &request.user_prompt,
                    },
                ],
                temperature: request.temperature,
                max_tokens: request.max_tokens,
            };

            let mut req_builder = self
                .client
                .post(&url)
                .header(reqwest::header::ACCEPT, "text/event-stream")
                .json(&body);

            if let Some(ref key) = self.api_key {
                req_builder = req_builder.bearer_auth(key);
            }

            let mut parser = SseParser::new();
            let mut accumulated = String::new();
            let mut parse_error: Option<ProviderError> = None;
            let mut done = false;

            let handle_event = |event_name: Option<&str>,
                                data: &str,
                                accumulated: &mut String,
                                done: &mut bool,
                                parse_error: &mut Option<ProviderError>| {
                if *done || parse_error.is_some() || request.cancel.is_cancelled() {
                    return;
                }
                if event_name == Some("error") {
                    *parse_error = Some(parse_sse_error_frame(data));
                    return;
                }
                let trimmed = data.trim();
                if trimmed.is_empty() {
                    return;
                }
                if trimmed == "[DONE]" {
                    *done = true;
                    return;
                }

                match serde_json::from_str::<ChatCompletionsChunk>(trimmed) {
                    Ok(chunk) => {
                        if let Some(err) = chunk.error {
                            *parse_error = Some(map_inline_error_field(err));
                            return;
                        }
                        for choice in chunk.choices.unwrap_or_default() {
                            if let Some(delta) = choice.delta {
                                if let Some(text) = delta.content {
                                    if !text.is_empty() {
                                        accumulated.push_str(&text);
                                        on_event(AnalysisStreamEvent::TokenDelta { chunk: text });
                                    }
                                }
                            }
                        }
                    }
                    Err(err) => {
                        *parse_error = Some(ProviderError::Protocol {
                            message: format!("invalid OpenAI-compatible SSE JSON: {err}"),
                        });
                    }
                }
            };

            send_and_stream(
                req_builder,
                request.timeout,
                &request.cancel,
                &on_event,
                |maybe_bytes| {
                    match maybe_bytes {
                        Some(bytes) => parser.feed(bytes, |ev| {
                            handle_event(
                                ev.event,
                                ev.data,
                                &mut accumulated,
                                &mut done,
                                &mut parse_error,
                            );
                        }),
                        None => parser.finish(|ev| {
                            handle_event(
                                ev.event,
                                ev.data,
                                &mut accumulated,
                                &mut done,
                                &mut parse_error,
                            );
                        }),
                    }
                    if let Some(err) = parse_error.take() {
                        return Err(err);
                    }
                    Ok(done)
                },
            )
            .await?;

            on_event(AnalysisStreamEvent::Completed {
                raw_json: accumulated,
                latency_ms: started.elapsed().as_millis() as u64,
            });
            Ok(())
        }
    }
}
