import { parseGrammarCard } from "../parseCard.ts";

const COMPLEX_34_WORD_SOURCE =
  "Although widely praised by critics who rarely agree on modern fiction, the novel Elena translated over three winters still unsettled the editors she had hoped would champion its experimental structure in print.";

const COMPLEX_34_WORD_JSON = JSON.stringify({
  meaning:
    "尽管受到很少在现代小说上达成一致的评论家们广泛赞誉，埃琳娜历经三个冬天翻译的这部小说仍令那些她原本希望能在纸媒上力推其实验性结构的编辑感到不安。",
  parts: [
    {
      kind: "clause",
      id: "c1",
      badge: "让步减化子句",
      connector: "Although",
      star: true,
      items: [
        { kind: "connector", id: "t1", text: "Although", badge: "让步连接词", note: "引出让步分句。" },
        { kind: "sub-token", id: "t2", text: "widely praised", role: "pred", subLabel: "过去分词", note: "被动语态减化。" },
        { kind: "sub-token", id: "t3", text: "by critics", role: "mod", antecedentIndex: 3, note: "先行词 3。" },
        { kind: "sub-token", id: "t4", text: "who rarely agree on modern fiction", role: "mod", note: "修饰 critics。" },
        { kind: "gap", id: "g3", index: 3, note: "who 指代 critics。" },
      ],
    },
    { kind: "punct", text: "," },
    { kind: "token", id: "t5", text: "the novel", role: "subj", star: true, antecedentIndex: 1, note: "主语，先行词 1。" },
    {
      kind: "clause",
      id: "c2",
      badge: "关系子句",
      omittedHint: "(that)",
      items: [
        { kind: "sub-token", id: "t6", text: "Elena", role: "subj", note: "子句主语。" },
        { kind: "sub-token", id: "t7", text: "translated", role: "pred", note: "子句谓语。" },
        { kind: "gap", id: "g1", index: 1, note: "指代 the novel。" },
        { kind: "sub-token", id: "t8", text: "over three winters", role: "mod", note: "时间状语。" },
      ],
    },
    { kind: "token", id: "t9", text: "still", role: "mod", note: "副词。" },
    { kind: "token", id: "t10", text: "unsettled", role: "pred", star: true, note: "主句谓语。" },
    { kind: "token", id: "t11", text: "the editors", role: "obj", antecedentIndex: 2, note: "主句宾语，先行词 2。" },
    {
      kind: "clause",
      id: "c3",
      badge: "连锁关系子句",
      omittedHint: "(who)",
      star: true,
      items: [
        { kind: "sub-token", id: "t12", text: "she had hoped", role: "mod", subLabel: "插入语", note: "插入语。" },
        { kind: "gap", id: "g2", index: 2, note: "指代 the editors。" },
        { kind: "sub-token", id: "t13", text: "would champion", role: "pred", note: "关系子句谓语。" },
        { kind: "sub-token", id: "t14", text: "its experimental structure", role: "obj", note: "宾语。" },
        { kind: "sub-token", id: "t15", text: "in print", role: "mod", note: "状语。" },
      ],
    },
    { kind: "punct", text: "." },
  ],
  recombination: "主干 the novel still unsettled the editors。",
  caveatNote: "she had hoped 为插入语。",
  bookRef: { chapter: "ch25", section: "减化子句与连锁关系子句", passageId: "ch25-p12" },
});

const ITERATIONS = 1000;
const RUNS = 5;
const WARMUP = 100;

function runPass(): { p50: number; p99: number; max: number; errors: number; partsChecksum: number } {
  const latencies = new Float64Array(ITERATIONS);
  let errors = 0;
  let partsChecksum = 0;

  for (let i = 0; i < WARMUP; i += 1) {
    const card = parseGrammarCard(COMPLEX_34_WORD_JSON, COMPLEX_34_WORD_SOURCE);
    if (card.kind === "structured") {
      partsChecksum += card.parts.length;
    }
  }

  partsChecksum = 0;
  for (let i = 0; i < ITERATIONS; i += 1) {
    const start = performance.now();
    const card = parseGrammarCard(COMPLEX_34_WORD_JSON, COMPLEX_34_WORD_SOURCE);
    latencies[i] = performance.now() - start;

    if (card.kind !== "structured" || card.parts.length !== 9) {
      errors += 1;
    } else {
      partsChecksum += card.parts.length;
    }
  }

  latencies.sort();
  const p50 = latencies[Math.floor(ITERATIONS * 0.5)] ?? 0;
  const p99 = latencies[Math.floor(ITERATIONS * 0.99)] ?? 0;
  const max = latencies[ITERATIONS - 1] ?? 0;
  return { p50, p99, max, errors, partsChecksum };
}

const p99Runs: number[] = [];
let totalErrors = 0;

for (let run = 1; run <= RUNS; run += 1) {
  const res = runPass();
  p99Runs.push(res.p99);
  totalErrors += res.errors;
  console.info(
    `run=${run} iterations=${ITERATIONS} p50=${res.p50.toFixed(4)}ms p99=${res.p99.toFixed(4)}ms max=${res.max.toFixed(4)}ms errors=${res.errors} checksum=${res.partsChecksum}`,
  );
}

const sortedP99 = [...p99Runs].sort((a, b) => a - b);
const medianP99 = sortedP99[Math.floor(sortedP99.length / 2)] ?? 0;
const minP99 = sortedP99[0] ?? 0;
const maxP99 = sortedP99[sortedP99.length - 1] ?? 0;

console.info(
  `summary runs=${RUNS} median_p99=${medianP99.toFixed(4)}ms range=[${minP99.toFixed(4)}ms, ${maxP99.toFixed(4)}ms] total_errors=${totalErrors} threshold=2.0000ms`,
);

if (totalErrors > 0 || medianP99 > 2.0) {
  throw new Error(
    `Benchmark failed: totalErrors=${totalErrors}, medianP99=${medianP99.toFixed(4)}ms (threshold 2.0ms)`,
  );
}
