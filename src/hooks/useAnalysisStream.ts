import { Channel, invoke } from "@tauri-apps/api/core";
import { useCallback, useRef, useState } from "react";
import { parseGrammarCard } from "../domain/parseCard.ts";
import type {
  ActionId,
  AnalysisStreamEvent,
  FallbackGrammarCard,
  ProviderId,
  RetrievalHit,
  StructuredGrammarCard,
} from "../domain/types.ts";

export interface RunAnalysisParams {
  readonly requestId: string;
  readonly sourceText: string;
  readonly providerId: ProviderId;
  readonly actionId: ActionId;
}

export interface AnalysisTransport {
  readonly runAnalysis: (
    params: RunAnalysisParams,
    onEvent: (event: AnalysisStreamEvent) => void,
  ) => Promise<void>;
  readonly cancelAnalysis: (requestId: string) => Promise<void>;
}

export type AnalysisStreamState =
  | { readonly kind: "idle" }
  | {
      readonly kind: "streaming";
      readonly requestId: string;
      readonly actionId?: ActionId;
      readonly sourceText: string;
      readonly hits: readonly RetrievalHit[];
      readonly streamedText: string;
    }
  | {
      readonly kind: "ready";
      readonly requestId: string;
      readonly actionId?: ActionId;
      readonly sourceText: string;
      readonly hits: readonly RetrievalHit[];
      readonly rawJson: string;
      readonly card: StructuredGrammarCard;
      readonly latencyMs: number;
    }
  | {
      readonly kind: "fallback";
      readonly requestId: string;
      readonly actionId?: ActionId;
      readonly sourceText: string;
      readonly hits: readonly RetrievalHit[];
      readonly rawJson: string;
      readonly card: FallbackGrammarCard;
      readonly latencyMs: number;
    }
  | {
      readonly kind: "error";
      readonly requestId: string;
      readonly sourceText: string;
      readonly code: string;
      readonly message: string;
    };

export interface CompletedAnalysisPayload {
  readonly sourceText: string;
  readonly providerId: ProviderId;
  readonly actionId: ActionId;
  readonly rawJson: string;
  readonly latencyMs: number;
}

export interface UseAnalysisStreamOptions {
  readonly transport?: AnalysisTransport;
  readonly onCompleted?: (payload: CompletedAnalysisPayload) => void;
}

export interface StartAnalysisArgs {
  readonly sourceText: string;
  readonly providerId: ProviderId;
  readonly actionId: ActionId;
}

export interface LoadSnapshotArgs {
  readonly sourceText: string;
  readonly rawJson: string;
  readonly latencyMs: number;
  readonly actionId?: ActionId;
}

export interface UseAnalysisStreamResult {
  readonly state: AnalysisStreamState;
  readonly start: (args: StartAnalysisArgs) => Promise<void>;
  readonly cancel: () => Promise<void>;
  readonly reset: () => void;
  readonly loadSnapshot: (args: LoadSnapshotArgs) => void;
}

const DEFAULT_TAURI_TRANSPORT: AnalysisTransport = {
  async runAnalysis(params, onEvent) {
    const channel = new Channel<AnalysisStreamEvent>();
    channel.onmessage = onEvent;
    await invoke("run_analysis", {
      request: params,
      channel,
    });
  },
  async cancelAnalysis(requestId) {
    await invoke("cancel_analysis", { requestId });
  },
};

function createRequestId(counter: number): string {
  return `req-${Date.now()}-${counter}`;
}

export function useAnalysisStream(
  options: UseAnalysisStreamOptions = {},
): UseAnalysisStreamResult {
  const transport = options.transport ?? DEFAULT_TAURI_TRANSPORT;
  const onCompletedRef = useRef(options.onCompleted);
  onCompletedRef.current = options.onCompleted;

  const [state, setState] = useState<AnalysisStreamState>({ kind: "idle" });
  const activeRequestIdRef = useRef<string | null>(null);
  const requestCounterRef = useRef(0);

  const start = useCallback(
    async ({ sourceText, providerId, actionId }: StartAnalysisArgs) => {
      const trimmed = sourceText.trim();
      if (trimmed.length === 0) {
        return;
      }

      const prevRequestId = activeRequestIdRef.current;
      if (prevRequestId !== null) {
        activeRequestIdRef.current = null;
        void transport.cancelAnalysis(prevRequestId).catch(() => {});
      }

      requestCounterRef.current += 1;
      const requestId = createRequestId(requestCounterRef.current);
      activeRequestIdRef.current = requestId;

      let latestHits: readonly RetrievalHit[] = [];
      let receivedTerminal = false;

      setState({
        kind: "streaming",
        requestId,
        actionId,
        sourceText: trimmed,
        hits: [],
        streamedText: "",
      });

      const handleEvent = (event: AnalysisStreamEvent) => {
        if (activeRequestIdRef.current !== requestId) {
          return;
        }

        switch (event.kind) {
          case "retrieval-ready": {
            latestHits = event.hits;
            setState((prev) =>
              prev.kind === "streaming" && prev.requestId === requestId
                ? { ...prev, hits: event.hits }
                : prev,
            );
            break;
          }
          case "token-delta": {
            setState((prev) =>
              prev.kind === "streaming" && prev.requestId === requestId
                ? { ...prev, streamedText: prev.streamedText + event.chunk }
                : prev,
            );
            break;
          }
          case "completed": {
            receivedTerminal = true;
            activeRequestIdRef.current = null;
            const parsed = parseGrammarCard(event.rawJson, trimmed);
            if (parsed.kind === "structured") {
              setState({
                kind: "ready",
                requestId,
                actionId,
                sourceText: trimmed,
                hits: latestHits,
                rawJson: event.rawJson,
                card: parsed,
                latencyMs: event.latencyMs,
              });
            } else {
              setState({
                kind: "fallback",
                requestId,
                actionId,
                sourceText: trimmed,
                hits: latestHits,
                rawJson: event.rawJson,
                card: parsed,
                latencyMs: event.latencyMs,
              });
            }
            onCompletedRef.current?.({
              sourceText: trimmed,
              providerId,
              actionId,
              rawJson: event.rawJson,
              latencyMs: event.latencyMs,
            });
            break;
          }
          case "failed": {
            receivedTerminal = true;
            activeRequestIdRef.current = null;
            if (event.code === "cancelled") {
              setState({ kind: "idle" });
            } else {
              setState({
                kind: "error",
                requestId,
                sourceText: trimmed,
                code: event.code,
                message: event.message,
              });
            }
            break;
          }
          default: {
            const _exhaustive: never = event;
            return _exhaustive;
          }
        }
      };

      try {
        await transport.runAnalysis(
          {
            requestId,
            sourceText: trimmed,
            providerId,
            actionId,
          },
          handleEvent,
        );
      } catch (err) {
        if (activeRequestIdRef.current !== requestId || receivedTerminal) {
          return;
        }
        activeRequestIdRef.current = null;
        const message = err instanceof Error ? err.message : String(err);
        setState({
          kind: "error",
          requestId,
          sourceText: trimmed,
          code: "transport",
          message,
        });
      }
    },
    [transport],
  );

  const cancel = useCallback(async () => {
    const requestId = activeRequestIdRef.current;
    activeRequestIdRef.current = null;
    setState({ kind: "idle" });
    if (requestId !== null) {
      try {
        await transport.cancelAnalysis(requestId);
      } catch {
      }
    }
  }, [transport]);

  const reset = useCallback(() => {
    activeRequestIdRef.current = null;
    setState({ kind: "idle" });
  }, []);

  const loadSnapshot = useCallback(
    ({ sourceText, rawJson, latencyMs, actionId }: LoadSnapshotArgs) => {
      activeRequestIdRef.current = null;
      const parsed = parseGrammarCard(rawJson, sourceText);
      const snapshotId = "history-snapshot";
      if (parsed.kind === "structured") {
        setState({
          kind: "ready",
          requestId: snapshotId,
          actionId,
          sourceText,
          hits: [],
          rawJson,
          card: parsed,
          latencyMs,
        });
      } else {
        setState({
          kind: "fallback",
          requestId: snapshotId,
          actionId,
          sourceText,
          hits: [],
          rawJson,
          card: parsed,
          latencyMs,
        });
      }
    },
    [],
  );

  return { state, start, cancel, reset, loadSnapshot };
}
