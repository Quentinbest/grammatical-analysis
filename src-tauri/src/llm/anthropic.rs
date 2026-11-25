use std::future::Future;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::domain::types::AnalysisStreamEvent;

use super::provider::{
    join_endpoint, send_and_stream, AnalysisPromptRequest, LlmProvider, ProviderError,
};
use super::sse::SseParser;

const ANTHROPIC_VERSION: &str = "2023-06-01";
const DEFAULT_MAX_TOKENS: u32 = 2048;

/// Anthropic Messages API streaming provider.
#[derive(Debug, Clone)]
pub struct AnthropicProvider {
    client: reqwest::Client,
    base_url: String,
    model: String,
    api_key: Option<String>,
}

impl AnthropicProvider {
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
struct MessagesRequest<'a> {
    model: &'a str,
    stream: bool,
    max_tokens: u32,
    #[serde(skip_serializing_if = "str::is_empty")]
    system: &'a str,
    messages: [AnthropicMessage<'a>; 1],
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
}

#[derive(Serialize)]
struct AnthropicMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct AnthropicEventPayload {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    delta: Option<AnthropicDelta>,
    #[serde(default)]
    error: Option<AnthropicErrorBody>,
}

#[derive(Deserialize)]
struct AnthropicDelta {
    #[serde(rename = "type")]
    delta_type: Option<String>,
    #[serde(default)]
    text: Option<String>,
}

#[derive(Deserialize)]
struct AnthropicErrorBody {
    #[serde(rename = "type")]
    error_type: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

fn anthropic_messages_url(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    if trimmed.ends_with("/v1/messages") {
        trimmed.to_string()
    } else if trimmed.ends_with("/v1") {
        format!("{trimmed}/messages")
    } else {
        join_endpoint(trimmed, "v1/messages")
    }
}

impl LlmProvider for AnthropicProvider {
    fn stream_analysis(
        &self,
        request: AnalysisPromptRequest,
        on_event: impl Fn(AnalysisStreamEvent) + Send + Sync,
    ) -> impl Future<Output = Result<(), ProviderError>> + Send {
        async move {
            let started = Instant::now();
            let url = anthropic_messages_url(&self.base_url);

            let body = MessagesRequest {
                model: &self.model,
                stream: true,
                max_tokens: request.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
                system: &request.system_prompt,
                messages: [AnthropicMessage {
                    role: "user",
                    content: &request.user_prompt,
                }],
                temperature: request.temperature,
            };

            let mut req_builder = self
                .client
                .post(&url)
                .header(reqwest::header::ACCEPT, "text/event-stream")
                .header("anthropic-version", ANTHROPIC_VERSION)
                .json(&body);

            if let Some(ref key) = self.api_key {
                req_builder = req_builder.header("x-api-key", key);
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
                if *done || parse_error.is_some() {
                    return;
                }
                let trimmed = data.trim();
                if trimmed.is_empty() {
                    return;
                }

                let payload: AnthropicEventPayload = match serde_json::from_str(trimmed) {
                    Ok(p) => p,
                    Err(err) => {
                        *parse_error = Some(ProviderError::Protocol {
                            message: format!("invalid Anthropic SSE JSON: {err}"),
                        });
                        return;
                    }
                };

                let kind = event_name.unwrap_or(payload.event_type.as_str());
                match kind {
                    "content_block_delta" => {
                        if let Some(delta) = payload.delta {
                            let is_text = delta
                                .delta_type
                                .as_deref()
                                .map(|t| t == "text_delta")
                                .unwrap_or(true);
                            if is_text {
                                if let Some(text) = delta.text {
                                    if !text.is_empty() {
                                        accumulated.push_str(&text);
                                        on_event(AnalysisStreamEvent::TokenDelta { chunk: text });
                                    }
                                }
                            }
                        }
                    }
                    "message_stop" => {
                        *done = true;
                    }
                    "error" => {
                        let (err_type, msg) = match payload.error {
                            Some(e) => (
                                e.error_type.unwrap_or_default(),
                                e.message.unwrap_or_else(|| "Anthropic stream error".into()),
                            ),
                            None => (String::new(), "Anthropic stream error".into()),
                        };
                        *parse_error = Some(if err_type == "rate_limit_error" {
                            ProviderError::RateLimited {
                                status: 429,
                                message: msg,
                            }
                        } else if err_type == "authentication_error" {
                            ProviderError::Unauthorized {
                                status: 401,
                                message: msg,
                            }
                        } else {
                            ProviderError::Protocol { message: msg }
                        });
                    }
                    _ => {}
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
