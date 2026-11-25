use std::future::Future;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::domain::types::AnalysisStreamEvent;

use super::provider::{
    join_endpoint, send_and_stream, AnalysisPromptRequest, LlmProvider, ProviderError,
};
use super::sse::NdjsonParser;

/// Local Ollama `/api/chat` NDJSON streaming provider.
#[derive(Debug, Clone)]
pub struct OllamaProvider {
    client: reqwest::Client,
    base_url: String,
    model: String,
}

impl OllamaProvider {
    pub fn new(client: reqwest::Client, base_url: String, model: String) -> Self {
        Self {
            client,
            base_url,
            model,
        }
    }
}

#[derive(Serialize)]
struct OllamaChatRequest<'a> {
    model: &'a str,
    stream: bool,
    messages: [OllamaMessage<'a>; 2],
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<OllamaOptions>,
}

#[derive(Serialize)]
struct OllamaMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Serialize)]
struct OllamaOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    num_predict: Option<u32>,
}

#[derive(Deserialize)]
struct OllamaChatChunk {
    #[serde(default)]
    message: Option<OllamaChunkMessage>,
    #[serde(default)]
    done: bool,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Deserialize)]
struct OllamaChunkMessage {
    #[serde(default)]
    content: String,
}

impl LlmProvider for OllamaProvider {
    fn stream_analysis(
        &self,
        request: AnalysisPromptRequest,
        on_event: impl Fn(AnalysisStreamEvent) + Send + Sync,
    ) -> impl Future<Output = Result<(), ProviderError>> + Send {
        async move {
            let started = Instant::now();
            let url = join_endpoint(&self.base_url, "api/chat");

            let options = if request.temperature.is_some() || request.max_tokens.is_some() {
                Some(OllamaOptions {
                    temperature: request.temperature,
                    num_predict: request.max_tokens,
                })
            } else {
                None
            };

            let body = OllamaChatRequest {
                model: &self.model,
                stream: true,
                messages: [
                    OllamaMessage {
                        role: "system",
                        content: &request.system_prompt,
                    },
                    OllamaMessage {
                        role: "user",
                        content: &request.user_prompt,
                    },
                ],
                options,
            };

            let req_builder = self.client.post(&url).json(&body);

            let mut parser = NdjsonParser::new();
            let mut accumulated = String::new();
            let mut parse_error: Option<ProviderError> = None;
            let mut done = false;

            let handle_line = |line: &str,
                               accumulated: &mut String,
                               done: &mut bool,
                               parse_error: &mut Option<ProviderError>| {
                if *done || parse_error.is_some() {
                    return;
                }
                match serde_json::from_str::<OllamaChatChunk>(line) {
                    Ok(chunk) => {
                        if let Some(err_msg) = chunk.error {
                            *parse_error = Some(ProviderError::Protocol { message: err_msg });
                            return;
                        }
                        if let Some(msg) = chunk.message {
                            if !msg.content.is_empty() {
                                accumulated.push_str(&msg.content);
                                on_event(AnalysisStreamEvent::TokenDelta { chunk: msg.content });
                            }
                        }
                        if chunk.done {
                            *done = true;
                        }
                    }
                    Err(err) => {
                        *parse_error = Some(ProviderError::Protocol {
                            message: format!("invalid Ollama NDJSON chunk: {err}"),
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
                        Some(bytes) => parser.feed(bytes, |line| {
                            handle_line(line, &mut accumulated, &mut done, &mut parse_error);
                        }),
                        None => parser.finish(|line| {
                            handle_line(line, &mut accumulated, &mut done, &mut parse_error);
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
