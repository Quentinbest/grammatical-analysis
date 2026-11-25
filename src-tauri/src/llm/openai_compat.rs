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
    choices: Vec<ChunkChoice>,
    #[serde(default)]
    error: Option<InlineError>,
}

#[derive(Deserialize)]
struct ChunkChoice {
    #[serde(default)]
    delta: ChunkDelta,
}

#[derive(Default, Deserialize)]
struct ChunkDelta {
    #[serde(default)]
    content: Option<String>,
}

#[derive(Deserialize)]
struct InlineError {
    #[serde(default)]
    message: Option<String>,
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

            let handle_data = |data: &str,
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
                if trimmed == "[DONE]" {
                    *done = true;
                    return;
                }

                match serde_json::from_str::<ChatCompletionsChunk>(trimmed) {
                    Ok(chunk) => {
                        if let Some(err) = chunk.error {
                            *parse_error = Some(ProviderError::Protocol {
                                message: err
                                    .message
                                    .unwrap_or_else(|| "upstream stream error".to_string()),
                            });
                            return;
                        }
                        for choice in chunk.choices {
                            if let Some(text) = choice.delta.content {
                                if !text.is_empty() {
                                    accumulated.push_str(&text);
                                    on_event(AnalysisStreamEvent::TokenDelta { chunk: text });
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
                            handle_data(ev.data, &mut accumulated, &mut done, &mut parse_error);
                        }),
                        None => parser.finish(|ev| {
                            handle_data(ev.data, &mut accumulated, &mut done, &mut parse_error);
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
