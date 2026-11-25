use std::sync::{Arc, Mutex};
use std::time::Instant;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use grammatical_analysis_lib::domain::types::{AnalysisStreamEvent, ProviderConfig};
use grammatical_analysis_lib::llm::{
    create_provider, AnalysisPromptRequest, LlmProvider, SseParser,
};

const GOLDEN_CARD_JSON: &str = r#"{"kind":"structured","sourceText":"The book Maya recommended won a prize.","meaning":"玛雅推荐的那本书获得了奖项。","parts":[{"kind":"token","id":"t1","text":"The book","role":"subj","antecedentIndex":1,"note":"主句主语，同时作为关系子句的先行词。"},{"kind":"clause","id":"c1","badge":"关系子句","omittedHint":"(that)","items":[{"kind":"sub-token","id":"t2","text":"Maya","role":"subj","note":"子句主语。"},{"kind":"sub-token","id":"t3","text":"recommended","role":"pred","note":"子句谓语动词。"},{"kind":"gap","id":"g1","index":1,"note":"省略的关系代词 that 在此处作宾语。"}]},{"kind":"token","id":"t4","text":"won","role":"pred","note":"主句谓语动词。"},{"kind":"token","id":"t5","text":"a prize","role":"obj","note":"主句宾语。"},{"kind":"punct","text":"."}],"recombination":"主干是 The book won a prize，中间的关系子句 Maya recommended 修饰 The book。","caveatNote":null,"bookRef":{"chapter":"ch19","section":"关系子句","passageId":"ch19-p04"}}"#;

async fn start_mock_sse_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind local SSE server");
    let addr = listener.local_addr().expect("local addr");

    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("accept client");
        let mut req_buf = [0u8; 4096];
        let _ = stream.read(&mut req_buf).await;

        let header = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n";
        stream.write_all(header.as_bytes()).await.unwrap();

        // Stream the golden StructuredGrammarCard JSON in 24-char chunks over SSE.
        let chars: Vec<char> = GOLDEN_CARD_JSON.chars().collect();
        for piece in chars.chunks(24) {
            let text: String = piece.iter().collect();
            let sse_payload = serde_json::json!({
                "choices": [{
                    "delta": { "content": text }
                }]
            });
            let frame = format!("data: {sse_payload}\n\n");
            let chunked = format!("{:x}\r\n{}\r\n", frame.len(), frame);
            stream.write_all(chunked.as_bytes()).await.unwrap();
        }

        let done = "data: [DONE]\n\n";
        let done_chunk = format!("{:x}\r\n{}\r\n0\r\n\r\n", done.len(), done);
        stream.write_all(done_chunk.as_bytes()).await.unwrap();
        stream.flush().await.unwrap();
    });

    format!("http://{addr}/v1")
}

fn run_sse_perf_probe() {
    let chunks: Vec<Vec<u8>> = (0..1000)
        .map(|i| {
            format!(
                "data: {{\"choices\":[{{\"delta\":{{\"content\":\"tok{i} \"}}}}]}}\n\n"
            )
            .into_bytes()
        })
        .collect();

    let mut run_ms = Vec::with_capacity(5);
    for run in 0..5 {
        let mut parser = SseParser::new();
        let mut parsed = 0usize;
        let start = Instant::now();
        for chunk in &chunks {
            parser.feed(chunk, |ev| {
                if !ev.data.is_empty() {
                    parsed += 1;
                }
            });
        }
        parser.finish(|ev| {
            if !ev.data.is_empty() {
                parsed += 1;
            }
        });
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        assert_eq!(parsed, 1000, "expected 1000 parsed SSE events");
        println!("perf run {}: 1,000 SSE chunks parsed in {:.3} ms", run + 1, ms);
        run_ms.push(ms);
    }

    let min_ms = run_ms.iter().copied().fold(f64::INFINITY, f64::min);
    let max_ms = run_ms.iter().copied().fold(0.0_f64, f64::max);
    let avg_ms = run_ms.iter().sum::<f64>() / run_ms.len() as f64;
    println!(
        "perf summary (1,000 chunks x 5 runs): min={:.3} ms avg={:.3} ms max={:.3} ms (threshold < 15.000 ms)",
        min_ms, avg_ms, max_ms
    );
    assert!(
        max_ms < 15.0,
        "SSE parsing exceeded 15ms threshold: max={max_ms:.3} ms"
    );
}

#[tokio::main]
async fn main() {
    let base_url = start_mock_sse_server().await;
    let config = ProviderConfig::OpenaiCompatible {
        id: "probe-local-sse".into(),
        name: "Local Mock SSE".into(),
        base_url,
        api_key_ref: "none".into(),
        model: "probe-model".into(),
    };

    let provider = create_provider(&config, None).expect("create provider");
    let deltas = Arc::new(Mutex::new(0usize));
    let completed_json = Arc::new(Mutex::new(None::<(String, u64)>));

    let deltas_cb = Arc::clone(&deltas);
    let completed_cb = Arc::clone(&completed_json);

    provider
        .stream_analysis(
            AnalysisPromptRequest::new(
                "You are an English grammar analyzer.",
                "The book Maya recommended won a prize.",
            ),
            move |event| match event {
                AnalysisStreamEvent::TokenDelta { .. } => {
                    *deltas_cb.lock().unwrap() += 1;
                }
                AnalysisStreamEvent::Completed {
                    raw_json,
                    latency_ms,
                } => {
                    *completed_cb.lock().unwrap() = Some((raw_json, latency_ms));
                }
                AnalysisStreamEvent::Failed { code, message } => {
                    panic!("unexpected stream failure ({code}): {message}");
                }
                AnalysisStreamEvent::RetrievalReady { .. } => {}
            },
        )
        .await
        .expect("end-to-end stream_analysis failed");

    let (raw_json, latency_ms) = completed_json
        .lock()
        .unwrap()
        .take()
        .expect("missing Completed event");
    let delta_count = *deltas.lock().unwrap();

    let parsed: serde_json::Value =
        serde_json::from_str(&raw_json).expect("assembled payload must be valid JSON");
    assert_eq!(parsed["kind"], "structured");
    assert_eq!(parsed["sourceText"], "The book Maya recommended won a prize.");
    assert!(parsed["parts"].as_array().map(|a| a.len()).unwrap_or(0) >= 5);
    assert_eq!(parsed["bookRef"]["chapter"], "ch19");

    println!(
        "live probe ok: received {} TokenDelta chunks, assembled {} bytes valid StructuredGrammarCard JSON in {} ms",
        delta_count,
        raw_json.len(),
        latency_ms
    );

    run_sse_perf_probe();
}
