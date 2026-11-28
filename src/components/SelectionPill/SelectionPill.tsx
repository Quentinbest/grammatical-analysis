import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useState } from "react";
import "../../styles/tokens.css";

function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

function extractCandidatePreview(payload: unknown): string | null {
  if (typeof payload === "string" && payload.trim().length > 0) {
    return payload.trim();
  }
  if (typeof payload === "object" && payload !== null) {
    if ("previewText" in payload && typeof payload.previewText === "string") {
      const trimmed = payload.previewText.trim();
      if (trimmed.length > 0) {
        return trimmed;
      }
    }
    if ("text" in payload && typeof payload.text === "string") {
      const trimmed = payload.text.trim();
      if (trimmed.length > 0) {
        return trimmed;
      }
    }
  }
  return null;
}

export function SelectionPill() {
  const [previewText, setPreviewText] = useState<string | null>(null);

  const handleConfirm = useCallback(() => {
    if (!isTauriRuntime()) {
      return;
    }
    void invoke("confirm_selection_pill").catch(() => {});
  }, []);

  const handleDismiss = useCallback(() => {
    if (!isTauriRuntime()) {
      return;
    }
    void invoke("dismiss_selection_pill").catch(() => {});
  }, []);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        handleDismiss();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [handleDismiss]);

  useEffect(() => {
    if (!isTauriRuntime()) {
      return undefined;
    }
    let active = true;
    let unlisten: (() => void) | null = null;

    void listen<unknown>("selection://candidate", (event) => {
      setPreviewText(extractCandidatePreview(event.payload));
    })
      .then((fn) => {
        if (active) {
          unlisten = fn;
        } else {
          fn();
        }
      })
      .catch(() => {});

    return () => {
      active = false;
      unlisten?.();
    };
  }, []);

  return (
    <div className="selection-pill-root">
      <div className="selection-pill" role="toolbar" aria-label="Selection actions">
        <button
          type="button"
          className="selection-pill-action"
          title={previewText ? `句型分析：${previewText}` : "分析选中英文句型"}
          onClick={handleConfirm}
        >
          <svg
            width="14"
            height="14"
            viewBox="0 0 16 16"
            fill="none"
            aria-hidden="true"
          >
            <path
              d="M5.2 2.5H3.3v11h1.9M10.8 2.5h1.9v11h-1.9M6.2 8h3.6"
              stroke="currentColor"
              strokeWidth="1.5"
              strokeLinecap="round"
              strokeLinejoin="round"
            />
          </svg>
          <span className="selection-pill-label">句型分析</span>
        </button>
        <button
          type="button"
          className="selection-pill-dismiss"
          aria-label="Dismiss selection popup"
          title="关闭"
          onClick={handleDismiss}
        >
          ×
        </button>
      </div>
    </div>
  );
}
