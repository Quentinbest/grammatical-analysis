// @vitest-environment jsdom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  type ActionId,
  type AnalysisStreamEvent,
  type ProviderId,
  isActionId,
  isProviderId,
} from "../../domain/types.ts";
import {
  type AnalysisTransport,
  type CompletedAnalysisPayload,
  type RunAnalysisParams,
  type UseAnalysisStreamResult,
  useAnalysisStream,
} from "../useAnalysisStream.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const VALID_CARD_JSON = JSON.stringify({
  kind: "structured",
  sourceText: "The book Maya recommended won a prize.",
  meaning: "玛雅推荐的那本书获得了奖项。",
  parts: [
    {
      kind: "token",
      id: "t1",
      text: "The book",
      role: "subj",
      antecedentIndex: 1,
      note: "主语中心语",
    },
    {
      kind: "clause",
      id: "c1",
      badge: "定语从句",
      omittedHint: "(that)",
      items: [
        { kind: "sub-token", id: "st1", text: "Maya", role: "subj", note: "从句主语" },
        { kind: "sub-token", id: "st2", text: "recommended", role: "pred", note: "从句谓语" },
        { kind: "gap", id: "g1", index: 1, note: "空位回指 The book" },
      ],
    },
    { kind: "token", id: "t2", text: "won", role: "pred", note: "主句谓语" },
    { kind: "token", id: "t3", text: "a prize", role: "obj", note: "主句宾语" },
    { kind: "punct", text: "." },
  ],
  recombination: "先抓主干 The book won a prize，再把 Maya recommended 回填修饰 The book。",
  caveatNote: null,
  bookRef: { chapter: "ch19", section: "关系子句", passageId: "ch19-b0012" },
});

function requireProviderId(raw: string): ProviderId {
  if (!isProviderId(raw)) {
    throw new Error(`Invalid ProviderId: ${raw}`);
  }
  return raw;
}

function requireActionId(raw: string): ActionId {
  if (!isActionId(raw)) {
    throw new Error(`Invalid ActionId: ${raw}`);
  }
  return raw;
}

function mountHook(
  transport: AnalysisTransport,
  onCompleted?: (payload: CompletedAnalysisPayload) => void,
) {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root: Root = createRoot(container);
  let latest: UseAnalysisStreamResult | null = null;

  function Harness() {
    latest = useAnalysisStream({ transport, onCompleted });
    return null;
  }

  act(() => {
    root.render(createElement(Harness));
  });

  return {
    get current(): UseAnalysisStreamResult {
      if (latest === null) {
        throw new Error("Hook did not mount");
      }
      return latest;
    },
    unmount() {
      act(() => {
        root.unmount();
      });
      container.remove();
    },
  };
}

describe("useAnalysisStream", () => {
  const providerId = requireProviderId("prov-ollama");
  const actionId = requireActionId("act-english-structure");
  let cleanupFns: Array<() => void> = [];

  beforeEach(() => {
    cleanupFns = [];
  });

  afterEach(() => {
    for (const fn of cleanupFns) {
      fn();
    }
    cleanupFns = [];
  });

  it("transitions idle -> streaming -> ready on valid structured JSON", async () => {
    let emit: ((event: AnalysisStreamEvent) => void) | null = null;
    let resolveRun: (() => void) | null = null;
    const onCompleted = vi.fn();

    const transport: AnalysisTransport = {
      runAnalysis: (_params: RunAnalysisParams, onEvent) => {
        emit = onEvent;
        return new Promise<void>((resolve) => {
          resolveRun = resolve;
        });
      },
      cancelAnalysis: vi.fn(async () => {}),
    };

    const hook = mountHook(transport, onCompleted);
    cleanupFns.push(() => hook.unmount());

    expect(hook.current.state.kind).toBe("idle");

    let runPromise: Promise<void> = Promise.resolve();
    act(() => {
      runPromise = hook.current.start({
        sourceText: "The book Maya recommended won a prize.",
        providerId,
        actionId,
      });
    });

    expect(hook.current.state.kind).toBe("streaming");

    act(() => {
      emit?.({
        kind: "retrieval-ready",
        hits: [
          {
            chapter: "ch19",
            section: "关系子句",
            passageId: "ch19-b0012",
            excerpt: "关系子句省略受格关系代名词。",
            score: 12.4,
          },
        ],
      });
      emit?.({ kind: "token-delta", chunk: "{\"meaning\":" });
      emit?.({ kind: "token-delta", chunk: "\"ok\"}" });
    });

    const streamingState = hook.current.state;
    if (streamingState.kind !== "streaming") {
      throw new Error(`Expected streaming, got ${streamingState.kind}`);
    }
    expect(streamingState.hits).toHaveLength(1);
    expect(streamingState.streamedText).toBe("{\"meaning\":\"ok\"}");

    await act(async () => {
      emit?.({
        kind: "completed",
        rawJson: VALID_CARD_JSON,
        latencyMs: 325,
      });
      resolveRun?.();
      await runPromise;
    });

    const readyState = hook.current.state;
    expect(readyState.kind).toBe("ready");
    if (readyState.kind === "ready") {
      expect(readyState.card.kind).toBe("structured");
      expect(readyState.card.meaning).toBe("玛雅推荐的那本书获得了奖项。");
      expect(readyState.latencyMs).toBe(325);
      expect(readyState.hits).toHaveLength(1);
    }
    expect(onCompleted).toHaveBeenCalledTimes(1);
    expect(onCompleted).toHaveBeenCalledWith({
      sourceText: "The book Maya recommended won a prize.",
      providerId,
      actionId,
      rawJson: VALID_CARD_JSON,
      latencyMs: 325,
    });
  });

  it("transitions idle -> streaming -> fallback when LLM returns malformed JSON", async () => {
    const transport: AnalysisTransport = {
      async runAnalysis(_params, onEvent) {
        onEvent({ kind: "token-delta", chunk: "{not valid json" });
        onEvent({
          kind: "completed",
          rawJson: "{not valid json",
          latencyMs: 180,
        });
      },
      cancelAnalysis: vi.fn(async () => {}),
    };

    const hook = mountHook(transport);
    cleanupFns.push(() => hook.unmount());

    await act(async () => {
      await hook.current.start({
        sourceText: "While walking home, the rain started.",
        providerId,
        actionId,
      });
    });

    expect(hook.current.state.kind).toBe("fallback");
    if (hook.current.state.kind === "fallback") {
      expect(hook.current.state.card.kind).toBe("fallback");
      expect(hook.current.state.card.rawText).toBe("{not valid json");
      expect(hook.current.state.card.parseError).toContain("JSON parse error");
    }
  });

  it("transitions idle -> streaming -> error on provider failure event", async () => {
    const transport: AnalysisTransport = {
      async runAnalysis(_params, onEvent) {
        onEvent({
          kind: "failed",
          code: "unauthorized",
          message: "HTTP 401: invalid api key",
        });
        throw new Error("HTTP 401: invalid api key");
      },
      cancelAnalysis: vi.fn(async () => {}),
    };

    const hook = mountHook(transport);
    cleanupFns.push(() => hook.unmount());

    await act(async () => {
      await hook.current.start({
        sourceText: "Cats sleep.",
        providerId,
        actionId,
      });
    });

    expect(hook.current.state.kind).toBe("error");
    if (hook.current.state.kind === "error") {
      expect(hook.current.state.code).toBe("unauthorized");
      expect(hook.current.state.message).toContain("401");
    }
  });

  it("cancels an in-flight stream cleanly and ignores late events", async () => {
    let emit: ((event: AnalysisStreamEvent) => void) | null = null;
    let resolveRun: (() => void) | null = null;
    const cancelledIds: string[] = [];

    const transport: AnalysisTransport = {
      runAnalysis: (_params, onEvent) => {
        emit = onEvent;
        return new Promise<void>((resolve) => {
          resolveRun = resolve;
        });
      },
      async cancelAnalysis(requestId) {
        cancelledIds.push(requestId);
      },
    };

    const hook = mountHook(transport);
    cleanupFns.push(() => hook.unmount());

    let runPromise: Promise<void> = Promise.resolve();
    act(() => {
      runPromise = hook.current.start({
        sourceText: "Long sentence under analysis.",
        providerId,
        actionId,
      });
    });

    expect(hook.current.state.kind).toBe("streaming");
    const activeId =
      hook.current.state.kind === "streaming" ? hook.current.state.requestId : "";

    await act(async () => {
      await hook.current.cancel();
    });

    expect(hook.current.state.kind).toBe("idle");
    expect(cancelledIds).toEqual([activeId]);

    await act(async () => {
      emit?.({ kind: "token-delta", chunk: "stale" });
      emit?.({ kind: "completed", rawJson: VALID_CARD_JSON, latencyMs: 99 });
      resolveRun?.();
      await runPromise;
    });

    expect(hook.current.state.kind).toBe("idle");
  });
});
