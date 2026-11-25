import { describe, expect, it } from "vitest";
import { parseGrammarCard } from "../parseCard.ts";

export const GOLDEN_SENTENCES = {
  relativeClauseSingleGap: {
    sourceText: "The book Maya recommended won a prize.",
    payload: {
      meaning: "玛雅推荐的那本书获得了奖项。",
      parts: [
        {
          kind: "token",
          id: "t1",
          text: "The book",
          role: "subj",
          antecedentIndex: 1,
          note: "主句主语，同时作为关系子句的先行词。",
        },
        {
          kind: "clause",
          id: "c1",
          badge: "关系子句",
          omittedHint: "(that)",
          items: [
            { kind: "sub-token", id: "t2", text: "Maya", role: "subj", note: "子句主语。" },
            { kind: "sub-token", id: "t3", text: "recommended", role: "pred", note: "子句谓语动词。" },
            { kind: "gap", id: "g1", index: 1, note: "省略的关系代词 that 在此处作宾语。" },
          ],
        },
        { kind: "token", id: "t4", text: "won", role: "pred", note: "主句谓语动词。" },
        { kind: "token", id: "t5", text: "a prize", role: "obj", note: "主句宾语。" },
        { kind: "punct", text: "." },
      ],
      recombination: "主干是 The book won a prize，中间的关系子句 Maya recommended 修饰 The book。",
      caveatNote: null,
      bookRef: { chapter: "ch19", section: "关系子句", passageId: "ch19-p04" },
    },
  },
  dualGap: {
    sourceText: "Those apples the farmer picked this morning taste much sweeter than the ones we bought yesterday.",
    payload: {
      meaning: "农夫今天早上摘的那些苹果比我们昨天买的甜得多。",
      parts: [
        {
          kind: "token",
          id: "t1",
          text: "Those apples",
          role: "subj",
          antecedentIndex: 1,
          note: "主句主语，先行词 1。",
        },
        {
          kind: "clause",
          id: "c1",
          badge: "关系子句",
          omittedHint: "(that)",
          items: [
            { kind: "sub-token", id: "t2", text: "the farmer", role: "subj", note: "子句主语。" },
            { kind: "sub-token", id: "t3", text: "picked", role: "pred", note: "子句谓语。" },
            { kind: "gap", id: "g1", index: 1, note: "对应先行词 Those apples。" },
            { kind: "sub-token", id: "t4", text: "this morning", role: "mod", note: "时间状语。" },
          ],
        },
        { kind: "token", id: "t5", text: "taste", role: "pred", note: "连缀动词。" },
        { kind: "token", id: "t6", text: "much sweeter", role: "obj", subLabel: "补语", note: "主语补语。" },
        {
          kind: "clause",
          id: "c2",
          badge: "比较状语",
          connector: "than",
          items: [
            { kind: "connector", id: "t7", text: "than", badge: "比较连接词", note: "引出比较对象。" },
            {
              kind: "sub-token",
              id: "t8",
              text: "the ones",
              role: "subj",
              antecedentIndex: 2,
              note: "替代 apples，先行词 2。",
            },
            { kind: "sub-token", id: "t9", text: "we", role: "subj", note: "定语子句主语。" },
            { kind: "sub-token", id: "t10", text: "bought", role: "pred", note: "定语子句谓语。" },
            { kind: "gap", id: "g2", index: 2, note: "对应先行词 the ones。" },
            { kind: "sub-token", id: "t11", text: "yesterday", role: "mod", note: "时间状语。" },
          ],
        },
        { kind: "punct", text: "." },
      ],
      recombination: "主干是 Those apples taste much sweeter，两个省略 that 的关系子句分别限定两边比较的苹果。",
      caveatNote: "两个关系代词均因作宾语而省略。",
      bookRef: { chapter: "ch19", section: "关系代词省略" },
    },
  },
  multiClause: {
    sourceText: "The committee announced that the policy, which had sparked debate for months, would take effect immediately.",
    payload: {
      meaning: "委员会宣布，那项引发了数月争论的政策将立即生效。",
      parts: [
        { kind: "token", id: "t1", text: "The committee", role: "subj", note: "主句主语。" },
        { kind: "token", id: "t2", text: "announced", role: "pred", note: "主句谓语。" },
        {
          kind: "clause",
          id: "c1",
          badge: "名词子句",
          connector: "that",
          items: [
            { kind: "connector", id: "t3", text: "that", badge: "连接词", note: "引出宾语从句。" },
            {
              kind: "sub-token",
              id: "t4",
              text: "the policy",
              role: "subj",
              antecedentIndex: 1,
              note: "宾语从句主语。",
            },
            { kind: "gap", id: "g1", index: 1, note: "非限定关系子句修饰 the policy。" },
            { kind: "sub-token", id: "t5", text: "would take effect", role: "pred", note: "宾语从句谓语。" },
            { kind: "sub-token", id: "t6", text: "immediately", role: "mod", note: "方式状语。" },
          ],
        },
        { kind: "punct", text: "." },
      ],
      recombination: "主句 The committee announced 后接 that 名词子句，内部嵌入修饰 the policy 的非限定关系子句。",
      caveatNote: null,
      bookRef: { chapter: "ch17", section: "名词子句" },
    },
  },
  simpleSentence: {
    sourceText: "The students finished the assignment early.",
    payload: {
      meaning: "学生们提前完成了作业。",
      parts: [
        { kind: "token", id: "t1", text: "The students", role: "subj", note: "主语。" },
        { kind: "token", id: "t2", text: "finished", role: "pred", note: "及物动词。" },
        { kind: "token", id: "t3", text: "the assignment", role: "obj", note: "直接宾语。" },
        { kind: "token", id: "t4", text: "early", role: "mod", note: "副词修饰 finished。" },
        { kind: "punct", text: "." },
      ],
      recombination: "简单 S+V+O+Adv 结构：The students finished the assignment early。",
      caveatNote: null,
      bookRef: null,
    },
  },
  complex34WordDualIndex: {
    sourceText:
      "Although widely praised by critics who rarely agree on modern fiction, the novel Elena translated over three winters still unsettled the editors she had hoped would champion its experimental structure in print.",
    payload: {
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
            { kind: "connector", id: "t1", text: "Although", badge: "让步连接词", note: "引出省略主语和 be 动词的让步分句。" },
            { kind: "sub-token", id: "t2", text: "widely praised", role: "pred", subLabel: "过去分词", note: "被动语态减化。" },
            {
              kind: "sub-token",
              id: "t3",
              text: "by critics",
              role: "mod",
              antecedentIndex: 3,
              note: "动作发出者，同时是 who 子句的先行词。",
            },
            { kind: "sub-token", id: "t4", text: "who rarely agree on modern fiction", role: "mod", note: "限定关系子句修饰 critics。" },
            { kind: "gap", id: "g3", index: 3, note: "who 指代 critics。" },
          ],
        },
        { kind: "punct", text: "," },
        {
          kind: "token",
          id: "t5",
          text: "the novel",
          role: "subj",
          star: true,
          antecedentIndex: 1,
          note: "全句主语，先行词 1。",
        },
        {
          kind: "clause",
          id: "c2",
          badge: "关系子句",
          omittedHint: "(that)",
          items: [
            { kind: "sub-token", id: "t6", text: "Elena", role: "subj", note: "子句主语。" },
            { kind: "sub-token", id: "t7", text: "translated", role: "pred", note: "子句谓语。" },
            { kind: "gap", id: "g1", index: 1, note: "省略关系代词指代 the novel。" },
            { kind: "sub-token", id: "t8", text: "over three winters", role: "mod", note: "时间状语。" },
          ],
        },
        { kind: "token", id: "t9", text: "still", role: "mod", note: "副词强调转折。" },
        { kind: "token", id: "t10", text: "unsettled", role: "pred", star: true, note: "主句核心谓语。" },
        {
          kind: "token",
          id: "t11",
          text: "the editors",
          role: "obj",
          antecedentIndex: 2,
          note: "主句宾语，先行词 2。",
        },
        {
          kind: "clause",
          id: "c3",
          badge: "连锁关系子句",
          omittedHint: "(who)",
          star: true,
          items: [
            { kind: "sub-token", id: "t12", text: "she had hoped", role: "mod", subLabel: "插入语", note: "插入的观点从句。" },
            { kind: "gap", id: "g2", index: 2, note: "关系代词在 would champion 前作主语，指代 the editors。" },
            { kind: "sub-token", id: "t13", text: "would champion", role: "pred", note: "关系子句谓语。" },
            { kind: "sub-token", id: "t14", text: "its experimental structure", role: "obj", note: "宾语。" },
            { kind: "sub-token", id: "t15", text: "in print", role: "mod", note: "介词短语作状语。" },
          ],
        },
        { kind: "punct", text: "." },
      ],
      recombination:
        "主干只有 the novel still unsettled the editors；句首是让步减化子句，主语和宾语各自后接一个含省略关系代词的修饰子句。",
      caveatNote: "she had hoped 为插入语结构，勿误判为主句谓语。",
      bookRef: { chapter: "ch25", section: "减化子句与连锁关系子句", passageId: "ch25-p12" },
    },
  },
} as const;

describe("parseGrammarCard", () => {
  it("parses all five prototype golden sentences into StructuredGrammarCard", () => {
    for (const fixture of Object.values(GOLDEN_SENTENCES)) {
      const card = parseGrammarCard(JSON.stringify(fixture.payload), fixture.sourceText);
      expect(card.kind).toBe("structured");
      if (card.kind !== "structured") {
        continue;
      }
      expect(card.sourceText).toBe(fixture.sourceText);
      expect(card.meaning).toBe(fixture.payload.meaning);
      expect(card.recombination).toBe(fixture.payload.recombination);
      expect(card.parts.length).toBe(fixture.payload.parts.length);
    }
  });

  it("strips markdown json fences around valid payloads", () => {
    const fixture = GOLDEN_SENTENCES.simpleSentence;
    const fenced = "```json\n" + JSON.stringify(fixture.payload, null, 2) + "\n```";
    const card = parseGrammarCard(fenced, fixture.sourceText);
    expect(card.kind).toBe("structured");
    if (card.kind === "structured") {
      expect(card.parts).toHaveLength(5);
    }
  });

  it("accepts an already-parsed object input", () => {
    const fixture = GOLDEN_SENTENCES.relativeClauseSingleGap;
    const card = parseGrammarCard(fixture.payload, fixture.sourceText);
    expect(card.kind).toBe("structured");
  });

  it("drops orphan GapSlot entries whose index has no matching antecedentIndex", () => {
    const raw = {
      meaning: "测试孤立空位清理。",
      parts: [
        { kind: "token", id: "t1", text: "The book", role: "subj", antecedentIndex: 1, note: "主语" },
        {
          kind: "clause",
          id: "c1",
          badge: "关系子句",
          items: [
            { kind: "gap", id: "g1", index: 1, note: "有效空位" },
            { kind: "gap", id: "g2", index: 2, note: "孤立空位，应被移除" },
            { kind: "sub-token", id: "t2", text: "Maya", role: "subj", note: "子句主语" },
          ],
        },
      ],
      recombination: "保留有效空位 1，清理孤立空位 2。",
    };

    const card = parseGrammarCard(raw, "The book Maya read.");
    expect(card.kind).toBe("structured");
    if (card.kind === "structured") {
      const clause = card.parts[1];
      expect(clause?.kind).toBe("clause");
      if (clause?.kind === "clause") {
        expect(clause.items.map((item) => item.id)).toEqual(["g1", "t2"]);
      }
    }
  });

  it("defaults omitted optional fields caveatNote and bookRef to null", () => {
    const raw = {
      meaning: "简单句。",
      parts: [{ kind: "token", id: "t1", text: "Run", role: "pred", note: "祈使句" }],
      recombination: "祈使句。",
    };
    const card = parseGrammarCard(raw, "Run.");
    expect(card.kind).toBe("structured");
    if (card.kind === "structured") {
      expect(card.caveatNote).toBeNull();
      expect(card.bookRef).toBeNull();
    }
  });

  it("degrades malformed JSON to FallbackGrammarCard without throwing", () => {
    const card = parseGrammarCard("{not valid json", "The book fell.");
    expect(card.kind).toBe("fallback");
    if (card.kind === "fallback") {
      expect(card.sourceText).toBe("The book fell.");
      expect(card.rawText).toBe("{not valid json");
      expect(card.parseError).toContain("JSON parse error");
    }
  });

  it("degrades non-object, empty, and invalid-field inputs cleanly", () => {
    const cases: unknown[] = [
      null,
      undefined,
      42,
      [],
      "",
      "   ",
      { meaning: "ok", recombination: "ok", parts: [] },
      { meaning: "ok", recombination: "ok", parts: [{ kind: "token", id: "t1", text: "x", role: "invalid", note: "" }] },
      { meaning: "ok", recombination: "ok", parts: [{ kind: "punct", text: "." }], bookRef: { chapter: "" } },
    ];

    for (const input of cases) {
      const card = parseGrammarCard(input, "Sample.");
      expect(card.kind).toBe("fallback");
      if (card.kind === "fallback") {
        expect(card.sourceText).toBe("Sample.");
        expect(card.parseError.length).toBeGreaterThan(0);
      }
    }
  });
});
