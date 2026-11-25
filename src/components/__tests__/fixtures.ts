import { parseGrammarCard } from "../../domain/parseCard.ts";
import {
  type ClauseId,
  type StructuredGrammarCard,
  type TokenId,
  isClauseId,
  isTokenId,
} from "../../domain/types.ts";

export function toClauseId(raw: string): ClauseId {
  if (!isClauseId(raw)) {
    throw new Error(`Invalid ClauseId: ${raw}`);
  }
  return raw;
}

export function toTokenId(raw: string): TokenId {
  if (!isTokenId(raw)) {
    throw new Error(`Invalid TokenId: ${raw}`);
  }
  return raw;
}

function mustParseStructured(payload: unknown, sourceText: string): StructuredGrammarCard {
  const card = parseGrammarCard(payload, sourceText);
  if (card.kind !== "structured") {
    throw new Error(`Expected structured card, got fallback: ${card.parseError}`);
  }
  return card;
}

export const SIMPLE_CARD = mustParseStructured(
  {
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
  "The students finished the assignment early.",
);

export const SINGLE_GAP_CARD = mustParseStructured(
  {
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
  "The book Maya recommended won a prize.",
);

export const COMPLEX_34_WORD_CARD = mustParseStructured(
  {
    meaning:
      "尽管受到很少在现代小说上达成一致的评论家们广泛赞誉，埃琳娜历经三个冬天翻译的这部小说仍令那些她原本希望能在纸媒上力推其实验性结构的编辑感到不安。",
    parts: [
      {
        kind: "clause",
        id: "c1",
        badge: "让步减化子句",
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
  "Although widely praised by critics who rarely agree on modern fiction, the novel Elena translated over three winters still unsettled the editors she had hoped would champion its experimental structure in print.",
);
