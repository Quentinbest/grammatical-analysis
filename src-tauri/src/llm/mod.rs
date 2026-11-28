pub mod anthropic;
pub mod ollama;
pub mod openai_compat;
pub mod provider;
pub mod sse;

pub use anthropic::AnthropicProvider;
pub use ollama::OllamaProvider;
pub use openai_compat::OpenAiCompatProvider;
pub use provider::{
    build_http_client, create_provider, create_provider_with_client, AnalysisPromptRequest,
    CancellationToken, LlmProvider, ProviderClient, ProviderError,
};
pub use sse::{NdjsonParser, SseEvent, SseParser};

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use crate::domain::types::{AnalysisStreamEvent, ProviderConfig};

    use super::*;

    async fn spawn_mock_server<F, Fut>(handler: F) -> (String, tokio::task::JoinHandle<String>)
    where
        F: FnOnce(tokio::net::TcpStream, String) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind local mock listener");
        let addr = listener.local_addr().expect("local addr");
        let base_url = format!("http://{addr}");

        let handle = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept connection");
            let req_text = read_http_request(&mut stream).await;
            handler(stream, req_text.clone()).await;
            req_text
        });

        (base_url, handle)
    }

    async fn read_http_request(stream: &mut tokio::net::TcpStream) -> String {
        let mut buf = Vec::with_capacity(2048);
        let mut tmp = [0u8; 1024];
        let mut header_end = None;
        let mut content_length = 0usize;

        loop {
            let n = stream.read(&mut tmp).await.unwrap_or(0);
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&tmp[..n]);

            if header_end.is_none() {
                if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    header_end = Some(pos + 4);
                    let headers = String::from_utf8_lossy(&buf[..pos]);
                    for line in headers.lines() {
                        if let Some((k, v)) = line.split_once(':') {
                            if k.eq_ignore_ascii_case("content-length") {
                                content_length = v.trim().parse().unwrap_or(0);
                            }
                        }
                    }
                }
            }

            if let Some(end) = header_end {
                if buf.len() >= end + content_length {
                    break;
                }
            }
        }

        String::from_utf8_lossy(&buf).into_owned()
    }

    async fn write_chunked_http(
        stream: &mut tokio::net::TcpStream,
        content_type: &str,
        chunks: &[&str],
        delay_between: Option<Duration>,
    ) {
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n"
        );
        let _ = stream.write_all(header.as_bytes()).await;
        let _ = stream.flush().await;

        for chunk in chunks {
            let frame = format!("{:x}\r\n{}\r\n", chunk.len(), chunk);
            if stream.write_all(frame.as_bytes()).await.is_err() {
                return;
            }
            if stream.flush().await.is_err() {
                return;
            }
            if let Some(delay) = delay_between {
                tokio::time::sleep(delay).await;
            }
        }
        let _ = stream.write_all(b"0\r\n\r\n").await;
        let _ = stream.flush().await;
    }

    async fn write_error_http(stream: &mut tokio::net::TcpStream, status: u16, body: &str) {
        let reason = match status {
            401 => "Unauthorized",
            429 => "Too Many Requests",
            500 => "Internal Server Error",
            _ => "Error",
        };
        let resp = format!(
            "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(resp.as_bytes()).await;
        let _ = stream.flush().await;
    }

    #[tokio::test]
    async fn ollama_streams_ndjson_deltas_and_completed() {
        let (base_url, req_handle) = spawn_mock_server(|mut stream, _| async move {
            let chunks = [
                "{\"message\":{\"role\":\"assistant\",\"content\":\"{\\\"meaning\\\":\"},\"done\":false}\n",
                "{\"message\":{\"role\":\"assistant\",\"content\":\"\\\"ok\\\"}\"},\"done\":false}\n",
                "{\"message\":{\"role\":\"assistant\",\"content\":\"\"},\"done\":true}\n",
            ];
            write_chunked_http(&mut stream, "application/x-ndjson", &chunks, None).await;
        })
        .await;

        let config = ProviderConfig::Ollama {
            id: "ollama-1".into(),
            name: "Local Ollama".into(),
            base_url,
            model: "qwen2.5:14b".into(),
        };
        let provider = create_provider(&config, None).expect("create ollama provider");
        let events = Arc::new(Mutex::new(Vec::new()));
        let events_cb = Arc::clone(&events);

        provider
            .stream_analysis(
                AnalysisPromptRequest::new("system prompt", "analyze this"),
                move |ev| events_cb.lock().unwrap().push(ev),
            )
            .await
            .expect("ollama stream succeeds");

        let raw_req = req_handle.await.expect("server join");
        assert!(raw_req.starts_with("POST /api/chat HTTP/1.1"));

        let captured = events.lock().unwrap();
        assert_eq!(captured.len(), 3);
        assert_eq!(
            captured[0],
            AnalysisStreamEvent::TokenDelta {
                chunk: "{\"meaning\":".into()
            }
        );
        assert_eq!(
            captured[1],
            AnalysisStreamEvent::TokenDelta {
                chunk: "\"ok\"}".into()
            }
        );
        match &captured[2] {
            AnalysisStreamEvent::Completed { raw_json, .. } => {
                assert_eq!(raw_json, "{\"meaning\":\"ok\"}");
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn openai_compat_streams_sse_deltas_and_completed() {
        let (base_url, req_handle) = spawn_mock_server(|mut stream, _| async move {
            let chunks = [
                "data: {\"choices\":[{\"delta\":{\"content\":\"{\\\"meaning\\\":\"}}]}\n",
                "\ndata: {\"choices\":[{\"delta\":{\"content\":\"\\\"hi\\\"}\"}}]}\n\n",
                "data: [DONE]\n\n",
            ];
            write_chunked_http(&mut stream, "text/event-stream", &chunks, None).await;
        })
        .await;

        let config = ProviderConfig::OpenaiCompatible {
            id: "openai-1".into(),
            name: "OpenAI".into(),
            base_url: format!("{base_url}/v1"),
            api_key_ref: "keychain:openai".into(),
            model: "gpt-4.1-mini".into(),
        };
        let provider =
            create_provider(&config, Some("sk-test-123".into())).expect("create openai provider");
        let events = Arc::new(Mutex::new(Vec::new()));
        let events_cb = Arc::clone(&events);

        provider
            .stream_analysis(
                AnalysisPromptRequest::new("system", "user"),
                move |ev| events_cb.lock().unwrap().push(ev),
            )
            .await
            .expect("openai stream succeeds");

        let raw_req = req_handle.await.expect("server join");
        assert!(raw_req.starts_with("POST /v1/chat/completions HTTP/1.1"));
        assert!(
            raw_req
                .to_ascii_lowercase()
                .contains("authorization: bearer sk-test-123"),
            "expected bearer auth header, got:\n{raw_req}"
        );

        let captured = events.lock().unwrap();
        assert_eq!(captured.len(), 3);
        match &captured[2] {
            AnalysisStreamEvent::Completed { raw_json, .. } => {
                assert_eq!(raw_json, "{\"meaning\":\"hi\"}");
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn openai_compat_local_lm_studio_omits_auth_when_no_api_key() {
        let (base_url, req_handle) = spawn_mock_server(|mut stream, _| async move {
            let chunks = [
                "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\n",
                "data: [DONE]\n\n",
            ];
            write_chunked_http(&mut stream, "text/event-stream", &chunks, None).await;
        })
        .await;

        let config = ProviderConfig::OpenaiCompatible {
            id: "lmstudio-1".into(),
            name: "LM Studio".into(),
            base_url: format!("{base_url}/v1"),
            api_key_ref: "none".into(),
            model: "local-qwen".into(),
        };
        let provider = create_provider(&config, None).expect("create lm studio provider");

        provider
            .stream_analysis(AnalysisPromptRequest::new("sys", "usr"), |_| {})
            .await
            .expect("lm studio stream succeeds");

        let raw_req = req_handle.await.expect("server join");
        assert!(!raw_req.to_ascii_lowercase().contains("authorization:"));
    }

    #[tokio::test]
    async fn anthropic_streams_sse_deltas_and_completed() {
        let (base_url, req_handle) = spawn_mock_server(|mut stream, _| async move {
            let chunks = [
                "event: message_start\ndata: {\"type\":\"message_start\"}\n\n",
                "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"{\\\"meaning\\\":\"}}\n\n",
                "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"\\\"claude\\\"}\"}}\n\n",
                "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
            ];
            write_chunked_http(&mut stream, "text/event-stream", &chunks, None).await;
        })
        .await;

        let config = ProviderConfig::Anthropic {
            id: "anthropic-1".into(),
            name: "Anthropic".into(),
            base_url,
            api_key_ref: "keychain:anthropic".into(),
            model: "claude-sonnet-4-5".into(),
        };
        let provider = create_provider(&config, Some("sk-ant-test".into()))
            .expect("create anthropic provider");
        let events = Arc::new(Mutex::new(Vec::new()));
        let events_cb = Arc::clone(&events);

        provider
            .stream_analysis(
                AnalysisPromptRequest::new("sys", "usr"),
                move |ev| events_cb.lock().unwrap().push(ev),
            )
            .await
            .expect("anthropic stream succeeds");

        let raw_req = req_handle.await.expect("server join");
        assert!(raw_req.starts_with("POST /v1/messages HTTP/1.1"));
        let lower = raw_req.to_ascii_lowercase();
        assert!(lower.contains("x-api-key: sk-ant-test"));
        assert!(lower.contains("anthropic-version: 2023-06-01"));

        let captured = events.lock().unwrap();
        assert_eq!(captured.len(), 3);
        match &captured[2] {
            AnalysisStreamEvent::Completed { raw_json, .. } => {
                assert_eq!(raw_json, "{\"meaning\":\"claude\"}");
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn mid_stream_cancellation_stops_all_three_providers() {
        for kind in ["ollama", "openai", "anthropic"] {
            let (base_url, _handle) = spawn_mock_server(move |mut stream, _| async move {
                let (content_type, first_chunk, second_chunk) = match kind {
                    "ollama" => (
                        "application/x-ndjson",
                        "{\"message\":{\"content\":\"part1\"},\"done\":false}\n",
                        "{\"message\":{\"content\":\"part2\"},\"done\":true}\n",
                    ),
                    "openai" => (
                        "text/event-stream",
                        "data: {\"choices\":[{\"delta\":{\"content\":\"part1\"}}]}\n\n",
                        "data: {\"choices\":[{\"delta\":{\"content\":\"part2\"}}]}\n\n",
                    ),
                    _ => (
                        "text/event-stream",
                        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"part1\"}}\n\n",
                        "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
                    ),
                };
                write_chunked_http(
                    &mut stream,
                    content_type,
                    &[first_chunk, second_chunk],
                    Some(Duration::from_secs(5)),
                )
                .await;
            })
            .await;

            let config = match kind {
                "ollama" => ProviderConfig::Ollama {
                    id: "o".into(),
                    name: "Ollama".into(),
                    base_url,
                    model: "m".into(),
                },
                "openai" => ProviderConfig::OpenaiCompatible {
                    id: "oa".into(),
                    name: "OpenAI".into(),
                    base_url,
                    api_key_ref: "k".into(),
                    model: "m".into(),
                },
                _ => ProviderConfig::Anthropic {
                    id: "a".into(),
                    name: "Anthropic".into(),
                    base_url,
                    api_key_ref: "k".into(),
                    model: "m".into(),
                },
            };

            let provider = create_provider(&config, Some("key".into())).unwrap();
            let cancel = CancellationToken::new();
            let cancel_on_first = cancel.clone();
            let events = Arc::new(Mutex::new(Vec::new()));
            let events_cb = Arc::clone(&events);

            let mut req = AnalysisPromptRequest::new("sys", "usr");
            req.cancel = cancel;

            let err = provider
                .stream_analysis(req, move |ev| {
                    if matches!(ev, AnalysisStreamEvent::TokenDelta { .. }) {
                        cancel_on_first.cancel();
                    }
                    events_cb.lock().unwrap().push(ev);
                })
                .await
                .expect_err("stream must return Cancelled");

            assert_eq!(err, ProviderError::Cancelled, "provider={kind}");
            let captured = events.lock().unwrap();
            assert!(
                matches!(
                    captured.last(),
                    Some(AnalysisStreamEvent::Failed { code, .. }) if code == "cancelled"
                ),
                "provider={kind}, events={captured:?}"
            );
        }
    }

    #[tokio::test]
    async fn http_401_429_500_errors_map_consistently_across_all_providers() {
        for kind in ["ollama", "openai", "anthropic"] {
            for (status, expected_code) in [
                (401u16, "unauthorized"),
                (429u16, "rate-limited"),
                (500u16, "protocol"),
            ] {
                let (base_url, _handle) = spawn_mock_server(move |mut stream, _| async move {
                    write_error_http(&mut stream, status, "{\"error\":\"boom\"}").await;
                })
                .await;

                let config = match kind {
                    "ollama" => ProviderConfig::Ollama {
                        id: "o".into(),
                        name: "Ollama".into(),
                        base_url,
                        model: "m".into(),
                    },
                    "openai" => ProviderConfig::OpenaiCompatible {
                        id: "oa".into(),
                        name: "OpenAI".into(),
                        base_url,
                        api_key_ref: "k".into(),
                        model: "m".into(),
                    },
                    _ => ProviderConfig::Anthropic {
                        id: "a".into(),
                        name: "Anthropic".into(),
                        base_url,
                        api_key_ref: "k".into(),
                        model: "m".into(),
                    },
                };

                let provider = create_provider(&config, Some("key".into())).unwrap();
                let events = Arc::new(Mutex::new(Vec::new()));
                let events_cb = Arc::clone(&events);

                let err = provider
                    .stream_analysis(AnalysisPromptRequest::new("sys", "usr"), move |ev| {
                        events_cb.lock().unwrap().push(ev);
                    })
                    .await
                    .expect_err("expected HTTP error");

                assert_eq!(
                    err.code(),
                    expected_code,
                    "provider={kind}, status={status}, err={err:?}"
                );
                let captured = events.lock().unwrap();
                assert!(matches!(
                    captured.last(),
                    Some(AnalysisStreamEvent::Failed { code, .. }) if code == expected_code
                ));
            }
        }
    }

    #[tokio::test]
    async fn stalled_server_triggers_timeout_error() {
        let (base_url, _handle) = spawn_mock_server(|stream, _| async move {
            tokio::time::sleep(Duration::from_secs(5)).await;
            drop(stream);
        })
        .await;

        let config = ProviderConfig::OpenaiCompatible {
            id: "oa-timeout".into(),
            name: "OpenAI".into(),
            base_url,
            api_key_ref: "k".into(),
            model: "m".into(),
        };

        let provider = create_provider(&config, Some("key".into())).unwrap();
        let events = Arc::new(Mutex::new(Vec::new()));
        let events_cb = Arc::clone(&events);

        let mut req = AnalysisPromptRequest::new("sys", "usr");
        req.timeout = Duration::from_millis(120);

        let err = provider
            .stream_analysis(req, move |ev| events_cb.lock().unwrap().push(ev))
            .await
            .expect_err("expected timeout");

        assert!(
            matches!(err, ProviderError::Timeout { timeout_ms: 120 }),
            "expected Timeout, got {err:?}"
        );
        let captured = events.lock().unwrap();
        assert!(matches!(
            captured.last(),
            Some(AnalysisStreamEvent::Failed { code, .. }) if code == "timeout"
        ));
    }

    fn provider_config_for(kind: &str, base_url: String) -> ProviderConfig {
        match kind {
            "ollama" => ProviderConfig::Ollama {
                id: "o".into(),
                name: "Ollama".into(),
                base_url,
                model: "m".into(),
            },
            "openai" => ProviderConfig::OpenaiCompatible {
                id: "oa".into(),
                name: "OpenAI".into(),
                base_url,
                api_key_ref: "k".into(),
                model: "m".into(),
            },
            _ => ProviderConfig::Anthropic {
                id: "a".into(),
                name: "Anthropic".into(),
                base_url,
                api_key_ref: "k".into(),
                model: "m".into(),
            },
        }
    }

    #[tokio::test]
    async fn truncated_stream_without_terminal_marker_fails_with_protocol_across_all_providers() {
        for kind in ["ollama", "openai", "anthropic"] {
            let (base_url, _handle) = spawn_mock_server(move |mut stream, _| async move {
                let (content_type, chunk) = match kind {
                    "ollama" => (
                        "application/x-ndjson",
                        "{\"message\":{\"content\":\"partial\"},\"done\":false}\n",
                    ),
                    "openai" => (
                        "text/event-stream",
                        "data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n",
                    ),
                    _ => (
                        "text/event-stream",
                        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"partial\"}}\n\n",
                    ),
                };
                write_chunked_http(&mut stream, content_type, &[chunk], None).await;
            })
            .await;

            let config = provider_config_for(kind, base_url);
            let provider = create_provider(&config, Some("key".into())).unwrap();
            let events = Arc::new(Mutex::new(Vec::new()));
            let events_cb = Arc::clone(&events);

            let err = provider
                .stream_analysis(AnalysisPromptRequest::new("sys", "usr"), move |ev| {
                    events_cb.lock().unwrap().push(ev);
                })
                .await
                .expect_err("truncated stream must return Protocol error");

            assert!(
                matches!(err, ProviderError::Protocol { .. }),
                "provider={kind}, err={err:?}"
            );
            let captured = events.lock().unwrap();
            assert!(
                !captured
                    .iter()
                    .any(|ev| matches!(ev, AnalysisStreamEvent::Completed { .. })),
                "provider={kind} must not emit Completed on truncated stream: {captured:?}"
            );
            assert!(
                matches!(
                    captured.last(),
                    Some(AnalysisStreamEvent::Failed { code, .. }) if code == "protocol"
                ),
                "provider={kind}, events={captured:?}"
            );
        }
    }

    #[tokio::test]
    async fn empty_and_non_sse_http_200_responses_fail_with_protocol() {
        for kind in ["ollama", "openai", "anthropic"] {
            for (label, body) in [
                ("empty", ""),
                ("plain_json", "{\"status\":\"ok\",\"message\":\"not a stream\"}"),
            ] {
                let body_owned = body.to_string();
                let (base_url, _handle) = spawn_mock_server(move |mut stream, _| async move {
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body_owned.len(),
                        body_owned
                    );
                    let _ = stream.write_all(resp.as_bytes()).await;
                    let _ = stream.flush().await;
                })
                .await;

                let config = provider_config_for(kind, base_url);
                let provider = create_provider(&config, Some("key".into())).unwrap();
                let events = Arc::new(Mutex::new(Vec::new()));
                let events_cb = Arc::clone(&events);

                let err = provider
                    .stream_analysis(AnalysisPromptRequest::new("sys", "usr"), move |ev| {
                        events_cb.lock().unwrap().push(ev);
                    })
                    .await
                    .expect_err("empty/non-SSE 200 OK must return Protocol error");

                assert!(
                    matches!(err, ProviderError::Protocol { .. }),
                    "provider={kind}, case={label}, err={err:?}"
                );
                let captured = events.lock().unwrap();
                assert!(
                    !captured
                        .iter()
                        .any(|ev| matches!(ev, AnalysisStreamEvent::Completed { .. })),
                    "provider={kind}, case={label} must not emit Completed: {captured:?}"
                );
                assert!(
                    matches!(
                        captured.last(),
                        Some(AnalysisStreamEvent::Failed { code, .. }) if code == "protocol"
                    ),
                    "provider={kind}, case={label}, events={captured:?}"
                );
            }
        }
    }

    #[tokio::test]
    async fn cancellation_during_final_chunk_or_eof_flush_emits_cancelled_not_completed() {
        for kind in ["ollama", "openai", "anthropic"] {
            for case in ["single_final_chunk", "eof_flush_without_trailing_newline"] {
                let (base_url, _handle) = spawn_mock_server(move |mut stream, _| async move {
                    let (content_type, payload) = match (kind, case) {
                        ("ollama", "single_final_chunk") => (
                            "application/x-ndjson",
                            "{\"message\":{\"content\":\"part1\"},\"done\":true}\n",
                        ),
                        ("ollama", _) => (
                            "application/x-ndjson",
                            "{\"message\":{\"content\":\"part1\"},\"done\":true}",
                        ),
                        ("openai", "single_final_chunk") => (
                            "text/event-stream",
                            "data: {\"choices\":[{\"delta\":{\"content\":\"part1\"}}]}\n\ndata: [DONE]\n\n",
                        ),
                        ("openai", _) => (
                            "text/event-stream",
                            "data: {\"choices\":[{\"delta\":{\"content\":\"part1\"}}]}",
                        ),
                        (_, "single_final_chunk") => (
                            "text/event-stream",
                            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"part1\"}}\n\nevent: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
                        ),
                        (_, _) => (
                            "text/event-stream",
                            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"part1\"}}",
                        ),
                    };
                    write_chunked_http(&mut stream, content_type, &[payload], None).await;
                })
                .await;

                let config = provider_config_for(kind, base_url);
                let provider = create_provider(&config, Some("key".into())).unwrap();
                let cancel = CancellationToken::new();
                let cancel_on_delta = cancel.clone();
                let events = Arc::new(Mutex::new(Vec::new()));
                let events_cb = Arc::clone(&events);

                let mut req = AnalysisPromptRequest::new("sys", "usr");
                req.cancel = cancel;

                let err = provider
                    .stream_analysis(req, move |ev| {
                        if matches!(ev, AnalysisStreamEvent::TokenDelta { .. }) {
                            cancel_on_delta.cancel();
                        }
                        events_cb.lock().unwrap().push(ev);
                    })
                    .await
                    .expect_err("cancelled stream must return Cancelled");

                assert_eq!(err, ProviderError::Cancelled, "provider={kind}, case={case}");
                let captured = events.lock().unwrap();
                assert!(
                    !captured
                        .iter()
                        .any(|ev| matches!(ev, AnalysisStreamEvent::Completed { .. })),
                    "provider={kind}, case={case} must not emit Completed: {captured:?}"
                );
                assert!(
                    matches!(
                        captured.last(),
                        Some(AnalysisStreamEvent::Failed { code, .. }) if code == "cancelled"
                    ),
                    "provider={kind}, case={case}, events={captured:?}"
                );
            }
        }
    }

    #[test]
    fn join_endpoint_and_anthropic_url_preserve_query_and_check_segment_boundaries() {
        use super::anthropic::anthropic_messages_url;
        use super::provider::join_endpoint;

        assert_eq!(
            join_endpoint(
                "https://my-resource.openai.azure.com/openai/deployments/gpt4?api-version=2024-02-01",
                "chat/completions"
            ),
            "https://my-resource.openai.azure.com/openai/deployments/gpt4/chat/completions?api-version=2024-02-01"
        );
        assert_eq!(
            join_endpoint(
                "https://my-resource.openai.azure.com/openai/deployments/gpt4/chat/completions/?api-version=2024-02-01#frag",
                "chat/completions"
            ),
            "https://my-resource.openai.azure.com/openai/deployments/gpt4/chat/completions?api-version=2024-02-01#frag"
        );
        assert_eq!(
            join_endpoint("http://localhost:11434/my-api/chat", "api/chat"),
            "http://localhost:11434/my-api/chat/api/chat"
        );
        assert_eq!(
            join_endpoint("http://localhost:11434/proxy/api/chat/", "api/chat"),
            "http://localhost:11434/proxy/api/chat"
        );
        assert_eq!(
            join_endpoint("http://localhost:11434/api/chat", "api/chat"),
            "http://localhost:11434/api/chat"
        );

        assert_eq!(
            anthropic_messages_url("https://api.anthropic.com/v1?beta=1"),
            "https://api.anthropic.com/v1/messages?beta=1"
        );
        assert_eq!(
            anthropic_messages_url("https://api.anthropic.com/v1/messages/?beta=1#top"),
            "https://api.anthropic.com/v1/messages?beta=1#top"
        );
        assert_eq!(
            anthropic_messages_url("https://api.anthropic.com/my-v1?beta=1"),
            "https://api.anthropic.com/my-v1/v1/messages?beta=1"
        );
        assert_eq!(
            anthropic_messages_url("https://api.anthropic.com/proxy/v1"),
            "https://api.anthropic.com/proxy/v1/messages"
        );
    }

    #[tokio::test]
    async fn openai_compat_handles_null_choices_and_null_delta() {
        let (base_url, _handle) = spawn_mock_server(|mut stream, _| async move {
            let chunks = [
                "data: {\"choices\":null}\n\n",
                "data: {\"choices\":[{\"delta\":null}]}\n\n",
                "data: {\"choices\":[{\"delta\":{\"content\":null}}]}\n\n",
                "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\n",
                "data: [DONE]\n\n",
            ];
            write_chunked_http(&mut stream, "text/event-stream", &chunks, None).await;
        })
        .await;

        let config = provider_config_for("openai", format!("{base_url}/v1"));
        let provider = create_provider(&config, Some("sk-test".into())).unwrap();
        let events = Arc::new(Mutex::new(Vec::new()));
        let events_cb = Arc::clone(&events);

        provider
            .stream_analysis(AnalysisPromptRequest::new("sys", "usr"), move |ev| {
                events_cb.lock().unwrap().push(ev);
            })
            .await
            .expect("null choices/delta should succeed cleanly");

        let captured = events.lock().unwrap();
        assert_eq!(captured.len(), 2);
        assert_eq!(
            captured[0],
            AnalysisStreamEvent::TokenDelta { chunk: "ok".into() }
        );
        assert!(matches!(
            &captured[1],
            AnalysisStreamEvent::Completed { raw_json, .. } if raw_json == "ok"
        ));
    }

    #[tokio::test]
    async fn openai_compat_maps_sse_error_frames_and_inline_rate_limit_or_auth_errors() {
        let cases: [(&str, &str, &str); 5] = [
            (
                "sse_event_error_rate_limit",
                "event: error\ndata: {\"error\":{\"type\":\"rate_limit_error\",\"message\":\"slow down\"}}\n\n",
                "rate-limited",
            ),
            (
                "sse_event_error_auth",
                "event: error\ndata: {\"type\":\"authentication_error\",\"code\":401,\"message\":\"bad key\"}\n\n",
                "unauthorized",
            ),
            (
                "sse_event_error_plain",
                "event: error\ndata: {\"choices\":[]}\n\n",
                "protocol",
            ),
            (
                "inline_rate_limit_429",
                "data: {\"error\":{\"code\":429,\"message\":\"too many requests\"}}\n\n",
                "rate-limited",
            ),
            (
                "inline_auth_401",
                "data: {\"error\":{\"status\":401,\"message\":\"invalid api key\"}}\n\n",
                "unauthorized",
            ),
        ];

        for (label, frame, expected_code) in cases {
            let frame_owned = frame.to_string();
            let (base_url, _handle) = spawn_mock_server(move |mut stream, _| async move {
                write_chunked_http(&mut stream, "text/event-stream", &[&frame_owned], None).await;
            })
            .await;

            let config = provider_config_for("openai", format!("{base_url}/v1"));
            let provider = create_provider(&config, Some("sk-test".into())).unwrap();
            let events = Arc::new(Mutex::new(Vec::new()));
            let events_cb = Arc::clone(&events);

            let err = provider
                .stream_analysis(AnalysisPromptRequest::new("sys", "usr"), move |ev| {
                    events_cb.lock().unwrap().push(ev);
                })
                .await
                .expect_err("error frame must fail");

            assert_eq!(err.code(), expected_code, "case={label}, err={err:?}");
            let captured = events.lock().unwrap();
            assert!(
                matches!(
                    captured.last(),
                    Some(AnalysisStreamEvent::Failed { code, .. }) if code == expected_code
                ),
                "case={label}, events={captured:?}"
            );
        }
    }

    #[tokio::test]
    async fn anthropic_dispatches_event_message_empty_and_omitted_via_payload_type() {
        let (base_url, _handle) = spawn_mock_server(|mut stream, _| async move {
            let chunks = [
                "event: message\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"A\"}}\n\n",
                "event:\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"B\"}}\n\n",
                "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"C\"}}\n\n",
                "event: message\ndata: {\"type\":\"message_stop\"}\n\n",
            ];
            write_chunked_http(&mut stream, "text/event-stream", &chunks, None).await;
        })
        .await;

        let config = provider_config_for("anthropic", base_url);
        let provider = create_provider(&config, Some("sk-ant".into())).unwrap();
        let events = Arc::new(Mutex::new(Vec::new()));
        let events_cb = Arc::clone(&events);

        provider
            .stream_analysis(AnalysisPromptRequest::new("sys", "usr"), move |ev| {
                events_cb.lock().unwrap().push(ev);
            })
            .await
            .expect("anthropic stream with event: message must succeed");

        let captured = events.lock().unwrap();
        assert_eq!(captured.len(), 4);
        assert!(matches!(
            &captured[3],
            AnalysisStreamEvent::Completed { raw_json, .. } if raw_json == "ABC"
        ));
    }

    #[tokio::test]
    async fn create_provider_trims_api_key_whitespace_and_newlines() {
        for kind in ["openai", "anthropic"] {
            let (base_url, req_handle) = spawn_mock_server(move |mut stream, _| async move {
                let chunks = if kind == "openai" {
                    vec![
                        "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\n",
                        "data: [DONE]\n\n",
                    ]
                } else {
                    vec![
                        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"ok\"}}\n\n",
                        "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
                    ]
                };
                write_chunked_http(&mut stream, "text/event-stream", &chunks, None).await;
            })
            .await;

            let config = provider_config_for(kind, base_url);
            let provider =
                create_provider(&config, Some("  sk-trimmed-key\r\n".into())).unwrap();

            provider
                .stream_analysis(AnalysisPromptRequest::new("sys", "usr"), |_| {})
                .await
                .expect("trimmed api_key must not fail header builder");

            let raw_req = req_handle.await.expect("server join").to_ascii_lowercase();
            if kind == "openai" {
                assert!(
                    raw_req.contains("authorization: bearer sk-trimmed-key\r\n"),
                    "expected trimmed bearer header, got:\n{raw_req}"
                );
            } else {
                assert!(
                    raw_req.contains("x-api-key: sk-trimmed-key\r\n"),
                    "expected trimmed x-api-key header, got:\n{raw_req}"
                );
            }
        }
    }

    #[tokio::test]
    async fn default_request_omits_token_cap_for_openai_and_ollama_and_uses_8192_for_anthropic() {
        for kind in ["openai", "ollama", "anthropic"] {
            let (base_url, req_handle) = spawn_mock_server(move |mut stream, _| async move {
                let (content_type, chunks) = match kind {
                    "ollama" => (
                        "application/x-ndjson",
                        vec!["{\"message\":{\"content\":\"ok\"},\"done\":true}\n"],
                    ),
                    "openai" => (
                        "text/event-stream",
                        vec![
                            "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\n",
                            "data: [DONE]\n\n",
                        ],
                    ),
                    _ => (
                        "text/event-stream",
                        vec![
                            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"ok\"}}\n\n",
                            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
                        ],
                    ),
                };
                write_chunked_http(&mut stream, content_type, &chunks, None).await;
            })
            .await;

            let config = provider_config_for(kind, base_url);
            let provider = create_provider(&config, Some("sk-test".into())).unwrap();

            provider
                .stream_analysis(AnalysisPromptRequest::new("sys", "usr"), |_| {})
                .await
                .expect("default request stream succeeds");

            let raw_req = req_handle.await.expect("server join");
            let (_, body_str) = raw_req
                .split_once("\r\n\r\n")
                .expect("HTTP request must have header/body separator");
            let body: serde_json::Value =
                serde_json::from_str(body_str).expect("request body must be valid JSON");

            match kind {
                "openai" => {
                    assert!(
                        body.get("max_tokens").is_none(),
                        "openai default request must omit max_tokens, got: {body}"
                    );
                }
                "ollama" => {
                    let num_predict = body.get("options").and_then(|o| o.get("num_predict"));
                    assert!(
                        num_predict.is_none(),
                        "ollama default request must omit num_predict, got: {body}"
                    );
                }
                _ => {
                    assert_eq!(
                        body.get("max_tokens").and_then(|v| v.as_u64()),
                        Some(8192),
                        "anthropic default request must set max_tokens to 8192, got: {body}"
                    );
                }
            }
        }
    }

    #[tokio::test]
    async fn openai_compat_sets_low_reasoning_effort_only_for_gemini_and_respects_override() {
        let googleapis_config = ProviderConfig::OpenaiCompatible {
            id: "prov-gemini-host".into(),
            name: "Gemini Host".into(),
            base_url: "https://generativelanguage.googleapis.com/v1beta/openai".into(),
            api_key_ref: "gemini".into(),
            model: "custom-gemini-endpoint-model".into(),
        };
        let ProviderClient::OpenaiCompatible(googleapis_provider) =
            create_provider(&googleapis_config, Some("gm-key".into())).unwrap()
        else {
            panic!("expected OpenaiCompatible provider");
        };
        let googleapis_body = serde_json::to_value(
            googleapis_provider.build_request_body(&AnalysisPromptRequest::new("sys", "usr")),
        )
        .expect("serialize googleapis body");
        assert_eq!(
            googleapis_body
                .get("reasoning_effort")
                .and_then(|v| v.as_str()),
            Some("low")
        );
        assert!(googleapis_body.get("max_tokens").is_none());

        let wire_cases: [(&str, Option<&str>, Option<&str>); 5] = [
            ("gemini-4-hydrogen", None, Some("low")),
            ("gemini-3.8-flash", None, Some("low")),
            ("gemini-4-hydrogen", Some("high"), Some("high")),
            ("gpt-6.2-sol", None, None),
            ("deepseek-v4.1-flash-1017", None, None),
        ];

        for (model, explicit_effort, expected_effort) in wire_cases {
            let (base_url, req_handle) = spawn_mock_server(|mut stream, _| async move {
                let chunks = [
                    "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\n",
                    "data: [DONE]\n\n",
                ];
                write_chunked_http(&mut stream, "text/event-stream", &chunks, None).await;
            })
            .await;

            let config = ProviderConfig::OpenaiCompatible {
                id: "prov-wire-case".into(),
                name: "Wire Case".into(),
                base_url: format!("{base_url}/v1"),
                api_key_ref: "key".into(),
                model: model.into(),
            };
            let client = build_http_client().expect("build shared http client");
            let provider =
                create_provider_with_client(client, &config, Some("sk-test".into())).unwrap();

            let mut req = AnalysisPromptRequest::new("sys", "usr");
            req.reasoning_effort = explicit_effort.map(str::to_string);

            provider
                .stream_analysis(req, |_| {})
                .await
                .expect("stream succeeds");

            let raw_req = req_handle.await.expect("server join");
            let (_, body_str) = raw_req
                .split_once("\r\n\r\n")
                .expect("HTTP request must have header/body separator");
            let body: serde_json::Value =
                serde_json::from_str(body_str).expect("request body must be valid JSON");

            assert_eq!(
                body.get("reasoning_effort").and_then(|v| v.as_str()),
                expected_effort,
                "model={model}, explicit={explicit_effort:?}, body={body}"
            );
            assert!(
                body.get("max_tokens").is_none(),
                "model={model} default request must omit max_tokens, got: {body}"
            );
        }
    }
}
