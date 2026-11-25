use std::env;
use std::path::{Path, PathBuf};
use std::process;
use std::time::Instant;

use grammatical_analysis_lib::retrieval::{
    Bm25BookRetriever, BookRetriever, PromptBuilder, PromptConfig,
};

const BENCH_QUERIES: &[&str] = &[
    "While walking home, the rain started.",
    "The book Maya recommended won a prize.",
    "He resigned, which surprised everyone on the team.",
    "The quarterly report was written by the committee.",
    "He stopped smoking after the doctor warned him.",
    "She is used to working late on Fridays.",
    "If I were you, I would accept the fellowship.",
    "Never have I seen such a clear explanation.",
    "Neither the manager nor the engineers were informed.",
    "What he said during the meeting surprised everyone.",
    "Although widely praised, the policy faced criticism.",
    "The man sitting by the window is my uncle.",
    "She must have missed the early train.",
    "The students are taking notes diligently.",
    "She is much taller than her older brother.",
    "He turned the job offer down immediately.",
    "Those apples I bought at the supermarket look fresh.",
    "The number of applicants has increased sharply.",
    "So do I.",
    "To tell you the truth, I forgot about the deadline.",
];

fn resolve_repo_root() -> PathBuf {
    if let Ok(cwd) = env::current_dir() {
        if cwd.join("knowledge/book/index.md").exists() {
            return cwd;
        }
        if let Some(parent) = cwd.parent() {
            if parent.join("knowledge/book/index.md").exists() {
                return parent.to_path_buf();
            }
        }
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn run_bench(book_dir: &Path) {
    let load_start = Instant::now();
    let retriever = Bm25BookRetriever::from_book_dir(book_dir).unwrap_or_else(|e| {
        eprintln!("error loading book index: {e}");
        process::exit(1);
    });
    let load_ms = load_start.elapsed().as_secs_f64() * 1000.0;

    let total_runs = 200usize;
    let mut latencies_ms = Vec::with_capacity(total_runs);
    let mut total_hits = 0usize;

    for i in 0..total_runs {
        let query = BENCH_QUERIES[i % BENCH_QUERIES.len()];
        let q_start = Instant::now();
        let hits = retriever.retrieve(query, 3);
        let q_ms = q_start.elapsed().as_secs_f64() * 1000.0;
        total_hits += hits.len();
        latencies_ms.push(q_ms);
    }

    latencies_ms.sort_by(|a, b| a.total_cmp(b));
    let min_ms = latencies_ms[0];
    let p50_ms = latencies_ms[total_runs / 2];
    let p95_idx = ((total_runs as f64) * 0.95).ceil() as usize - 1;
    let p95_ms = latencies_ms[p95_idx.min(total_runs - 1)];
    let max_ms = latencies_ms[total_runs - 1];

    println!("BM25 Retrieval Benchmark");
    println!("  documents:       {}", retriever.document_count());
    println!("  topic_routes:    {}", retriever.route_count());
    println!("  cold_load_ms:    {load_ms:.3} ms (budget < 150.0 ms)");
    println!("  queries:         {total_runs} (total hits returned: {total_hits})");
    println!("  query_min_ms:    {min_ms:.3} ms");
    println!("  query_p50_ms:    {p50_ms:.3} ms");
    println!("  query_p95_ms:    {p95_ms:.3} ms (budget < 10.0 ms)");
    println!("  query_max_ms:    {max_ms:.3} ms");

    if load_ms >= 150.0 || p95_ms >= 10.0 {
        eprintln!("FAIL: performance threshold exceeded");
        process::exit(1);
    }
    println!("  status:          PASS");
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("Usage:");
        eprintln!("  retrieval_probe \"<sentence>\"");
        eprintln!("  retrieval_probe --bench");
        process::exit(2);
    }

    let root = resolve_repo_root();
    let book_dir = root.join("knowledge/book");
    let prompts_dir = root.join("prompts");

    if args[0] == "--bench" {
        run_bench(&book_dir);
        return;
    }

    let query = args.join(" ");

    let load_start = Instant::now();
    let retriever = Bm25BookRetriever::from_book_dir(&book_dir).unwrap_or_else(|e| {
        eprintln!("error loading book index: {e}");
        process::exit(1);
    });
    let load_ms = load_start.elapsed().as_secs_f64() * 1000.0;

    let query_start = Instant::now();
    let hits = retriever.retrieve(&query, 3);
    let query_ms = query_start.elapsed().as_secs_f64() * 1000.0;

    println!("Query: {query}");
    println!(
        "Index: {} sections | cold load: {:.3} ms | query: {:.3} ms",
        retriever.document_count(),
        load_ms,
        query_ms
    );
    println!("Top-{} hits:", hits.len());
    for (idx, hit) in hits.iter().enumerate() {
        println!(
            "  {}. [{} | {} | {}] score={:.3}\n     {}",
            idx + 1,
            hit.chapter,
            hit.section,
            hit.passage_id.as_deref().unwrap_or("n/a"),
            hit.score,
            hit.excerpt
        );
    }

    if let Ok(builder) = PromptBuilder::from_dir(&prompts_dir, PromptConfig::default()) {
        let prompt = builder.build(&query, &hits);
        println!(
            "Prompt: chars={} (cap={}), est_tokens={}, caveats_included={}, hits_injected={}, truncated={}",
            prompt.total_chars,
            PromptConfig::default().max_total_chars,
            prompt.estimated_tokens,
            prompt.included_caveats,
            prompt.included_hits,
            prompt.truncated
        );
    }
}
