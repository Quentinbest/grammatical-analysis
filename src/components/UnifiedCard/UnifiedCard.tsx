import { Fragment, useCallback, useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import {
  type FoldedClauses,
  areAllClausesFolded,
  createEmptyFoldState,
  findTokenNote,
  hasFoldableClauses,
  hasGapSlots,
  hasOmittedHints,
  isTokenVisible,
  setAllClausesFolded,
  shouldDrawGapArc,
  toggleClauseFold,
} from "../../domain/cardViewModel.ts";
import type {
  CardTheme,
  ClauseGroup,
  ClauseId,
  ClauseItem,
  FallbackGrammarCard,
  GapIndex,
  GrammarCard,
  MainToken,
  StructuredGrammarCard,
  SubToken,
  TokenId,
} from "../../domain/types.ts";
import "../../styles/tokens.css";

export interface UnifiedCardProps {
  readonly card: GrammarCard;
  readonly theme?: CardTheme;
  readonly compact?: boolean;
  readonly onSpeak?: (sourceText: string) => void;
}

interface TraceArc {
  readonly key: string;
  readonly index: GapIndex;
  readonly d: string;
}

interface TraceColors {
  readonly 1: string;
  readonly 2: string;
  readonly 3: string;
}

const DEFAULT_TRACE_COLORS: TraceColors = {
  1: "#3b82f6",
  2: "#10b981",
  3: "#f59e0b",
};

function gapIndexClass(index: GapIndex): string {
  switch (index) {
    case 1:
      return "";
    case 2:
      return "idx-2";
    case 3:
      return "idx-3";
    default: {
      const _exhaustive: never = index;
      return _exhaustive;
    }
  }
}

function parseGapIndexAttr(raw: string | undefined): GapIndex | null {
  if (raw === "1") {
    return 1;
  }
  if (raw === "2") {
    return 2;
  }
  if (raw === "3") {
    return 3;
  }
  return null;
}

function ConnectorOvalSvg() {
  return (
    <svg viewBox="0 0 100 44" preserveAspectRatio="none" aria-hidden="true">
      <path
        d="M12,24 C8,10 34,3 60,4 C88,5 97,15 95,25 C93,36 68,41 40,40 C15,39 4,31 9,19 C13,10 38,5 68,6"
        fill="none"
        stroke="currentColor"
        strokeWidth="2.1"
        strokeLinecap="round"
      />
    </svg>
  );
}

function AudioIcon() {
  return (
    <svg
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.9"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5" />
      <path d="M15.54 8.46a5 5 0 0 1 0 7.07" />
    </svg>
  );
}

function CardLegend({ theme }: { readonly theme: CardTheme }) {
  if (theme === "warm-paper") {
    return (
      <div className="legend">
        <span>
          <i className="swatch swatch-subj" aria-hidden="true" />
          主语
        </span>
        <span>
          <i className="swatch swatch-pred" aria-hidden="true" />
          谓语
        </span>
        <span>
          <i className="swatch swatch-obj" aria-hidden="true" />
          宾语
        </span>
      </div>
    );
  }
  return (
    <div className="legend">
      <span className="leg-dark-subj">主语</span>
      <span className="leg-dark-pred">谓语</span>
      <span className="leg-dark-obj">宾语</span>
    </div>
  );
}

function FallbackCardView({
  card,
  theme,
  compact,
}: {
  readonly card: FallbackGrammarCard;
  readonly theme: CardTheme;
  readonly compact: boolean;
}) {
  return (
    <article
      className="unified-card fallback-card"
      data-card-theme={theme}
      data-compact={compact ? "true" : undefined}
      data-card-kind="fallback"
    >
      <p className="fallback-source">{card.sourceText}</p>
      {card.meaning.trim().length > 0 ? (
        <div className="translation-box">{card.meaning}</div>
      ) : null}
      <div className="caveat-strip" role="status">
        结构化解析降级：{card.parseError}
      </div>
      {card.rawText.trim().length > 0 ? (
        <pre className="fallback-raw">{card.rawText}</pre>
      ) : null}
    </article>
  );
}

function StructuredCardView({
  card,
  theme,
  compact,
  onSpeak,
}: {
  readonly card: StructuredGrammarCard;
  readonly theme: CardTheme;
  readonly compact: boolean;
  readonly onSpeak?: (sourceText: string) => void;
}) {
  const [prevCard, setPrevCard] = useState<StructuredGrammarCard>(card);
  const [foldedClauses, setFoldedClauses] = useState<FoldedClauses>(createEmptyFoldState);
  const [showGapLayer, setShowGapLayer] = useState(true);
  const [showOmitted, setShowOmitted] = useState(false);
  const [selectedId, setSelectedId] = useState<TokenId | null>(null);
  const [traceArcs, setTraceArcs] = useState<readonly TraceArc[]>([]);
  const [traceColors, setTraceColors] = useState<TraceColors>(DEFAULT_TRACE_COLORS);

  if (prevCard !== card) {
    setPrevCard(card);
    setFoldedClauses(createEmptyFoldState());
    setSelectedId(null);
    setTraceArcs([]);
  }

  const cardRef = useRef<HTMLElement | null>(null);
  const wrapRef = useRef<HTMLDivElement | null>(null);
  const markerPrefix = useId().replace(/:/g, "");

  const clausesPresent = hasFoldableClauses(card);
  const gapsPresent = hasGapSlots(card);
  const omittedPresent = hasOmittedHints(card);
  const foldedNow = areAllClausesFolded(card, foldedClauses);
  const traceVisible = gapsPresent && showGapLayer && !foldedNow;
  const activeNote = findTokenNote(card, selectedId);

  const recomputeTraces = useCallback(() => {
    const wrap = wrapRef.current;
    const cardEl = cardRef.current;
    if (!wrap || !cardEl || !gapsPresent || !showGapLayer) {
      setTraceArcs([]);
      return;
    }

    const styles = getComputedStyle(cardEl);
    setTraceColors({
      1: styles.getPropertyValue("--gap-badge-bg").trim() || DEFAULT_TRACE_COLORS[1],
      2: styles.getPropertyValue("--gap2-badge-bg").trim() || DEFAULT_TRACE_COLORS[2],
      3: styles.getPropertyValue("--gap3-badge-bg").trim() || DEFAULT_TRACE_COLORS[3],
    });

    const wrapRect = wrap.getBoundingClientRect();
    const gapNodes = wrap.querySelectorAll<HTMLElement>("[data-gap-idx]");
    if (gapNodes.length === 0) {
      setTraceArcs([]);
      return;
    }

    const nextArcs: TraceArc[] = [];
    gapNodes.forEach((gapEl, order) => {
      const idx = parseGapIndexAttr(gapEl.dataset.gapIdx);
      if (idx === null) {
        return;
      }
      const antEl = wrap.querySelector<HTMLElement>(`[data-antecedent-idx="${idx}"]`);
      if (!antEl) {
        return;
      }
      const antRect = antEl.getBoundingClientRect();
      const gapRect = gapEl.getBoundingClientRect();
      const sameLine = Math.abs(gapRect.top - antRect.top) < 16;
      const isSelected =
        selectedId !== null &&
        (selectedId === gapEl.dataset.tokId || selectedId === antEl.dataset.tokId);

      if (!shouldDrawGapArc(sameLine, isSelected)) {
        return;
      }

      const ax = antRect.left - wrapRect.left + antRect.width / 2;
      const ay = antRect.bottom - wrapRect.top + 3;
      const gx = gapRect.left - wrapRect.left + gapRect.width / 2;
      const gy = gapRect.bottom - wrapRect.top + 3;
      const depth = sameLine ? 22 + (order % 2) * 12 : 18;
      const midX = (ax + gx) / 2;
      const ctrlY = Math.max(ay, gy) + depth;

      nextArcs.push({
        key: `${gapEl.dataset.tokId ?? order}-${idx}`,
        index: idx,
        d: `M ${gx} ${gy} Q ${midX} ${ctrlY} ${ax} ${ay}`,
      });
    });

    setTraceArcs(nextArcs);
  }, [gapsPresent, showGapLayer, selectedId]);

  useLayoutEffect(() => {
    recomputeTraces();
  }, [recomputeTraces, card, theme, compact, foldedClauses, showOmitted]);

  useEffect(() => {
    const wrap = wrapRef.current;
    if (!wrap || typeof ResizeObserver === "undefined") {
      return undefined;
    }
    const observer = new ResizeObserver(() => {
      recomputeTraces();
    });
    observer.observe(wrap);
    return () => {
      observer.disconnect();
    };
  }, [recomputeTraces]);

  const handleSelectToken = (id: TokenId) => {
    setSelectedId((prev) => (prev === id ? null : id));
  };

  const handleToggleClause = (clauseId: ClauseId) => {
    const nextFolded = toggleClauseFold(foldedClauses, clauseId);
    setFoldedClauses(nextFolded);
    if (selectedId !== null && !isTokenVisible(card, selectedId, nextFolded, showGapLayer)) {
      setSelectedId(null);
    }
  };

  const handleToggleFoldAll = () => {
    const nextFolded = setAllClausesFolded(card, !foldedNow);
    setFoldedClauses(nextFolded);
    if (selectedId !== null && !isTokenVisible(card, selectedId, nextFolded, showGapLayer)) {
      setSelectedId(null);
    }
  };

  const handleToggleGapLayer = () => {
    const nextShowGap = !showGapLayer;
    setShowGapLayer(nextShowGap);
    if (selectedId !== null && !isTokenVisible(card, selectedId, foldedClauses, nextShowGap)) {
      setSelectedId(null);
    }
  };

  const renderWordToken = (token: MainToken | SubToken) => {
    const isMain = token.kind === "token";
    const isSelected = selectedId === token.id;
    const subTypeClass = token.role === "pred" ? "pred" : "muted";
    const antIdx = token.antecedentIndex;

    return (
      <span
        key={token.id}
        className={`tok ${isMain ? "main-tok" : "sub-tok"} u-${token.role}${isSelected ? " selected" : ""}`}
        data-tok-id={token.id}
        data-antecedent-idx={antIdx !== undefined && showGapLayer ? String(antIdx) : undefined}
        onClick={() => handleSelectToken(token.id)}
      >
        {token.text}
        {antIdx !== undefined && showGapLayer ? (
          <span className={`idx-dot ${gapIndexClass(antIdx)}`}>{antIdx}</span>
        ) : null}
        {token.star ? <span className="star-sup">★</span> : null}
        {token.subLabel ? (
          <span className={`micro-sub ${subTypeClass}`}>{token.subLabel}</span>
        ) : null}
      </span>
    );
  };

  const renderClauseItem = (item: ClauseItem) => {
    switch (item.kind) {
      case "sub-token":
        return renderWordToken(item);
      case "gap": {
        if (!showGapLayer) {
          return null;
        }
        const idxCls = gapIndexClass(item.index);
        const isSelected = selectedId === item.id;
        return (
          <span
            key={item.id}
            className={`gap-slot ${idxCls}${isSelected ? " selected" : ""}`}
            data-tok-id={item.id}
            data-gap-idx={String(item.index)}
            onClick={() => handleSelectToken(item.id)}
          >
            □<span className={`idx-dot ${idxCls}`}>{item.index}</span>
          </span>
        );
      }
      case "connector": {
        const isSelected = selectedId === item.id;
        return (
          <Fragment key={item.id}>
            <span className="clause-badge">
              {item.badge}
              {item.star ? <span className="star-sup">★</span> : null}
            </span>
            <span
              className={`conn-oval conn-interactive${isSelected ? " selected" : ""}`}
              data-tok-id={item.id}
              onClick={() => handleSelectToken(item.id)}
            >
              {item.text}
              <ConnectorOvalSvg />
              {item.subLabel ? <span className="micro-sub conn">{item.subLabel}</span> : null}
            </span>
          </Fragment>
        );
      }
      default: {
        const _exhaustive: never = item;
        return _exhaustive;
      }
    }
  };

  const renderClause = (clause: ClauseGroup) => {
    if (foldedClauses.has(clause.id)) {
      return (
        <button
          key={clause.id}
          type="button"
          className="fold-pill"
          data-fold-clause={clause.id}
          title="点击原位展开"
          onClick={() => handleToggleClause(clause.id)}
        >
          [… {clause.badge}]
        </button>
      );
    }

    const hasLeadingInlineConnector = clause.items[0]?.kind === "connector";
    const showClauseConnector = Boolean(clause.connector) && !hasLeadingInlineConnector;

    return (
      <span key={clause.id} className="clause-box" data-clause-id={clause.id}>
        <button
          type="button"
          className="clause-badge"
          data-fold-clause={clause.id}
          title="点击折叠此从句"
          onClick={() => handleToggleClause(clause.id)}
        >
          {clause.badge}
          {clause.star ? <span className="star-sup">★</span> : null}
        </button>
        {showClauseConnector ? (
          <>
            <span className="conn-oval">
              {clause.connector}
              <ConnectorOvalSvg />
            </span>{" "}
          </>
        ) : null}
        {clause.omittedHint && showOmitted ? (
          <>
            <span className="omitted-hint">{clause.omittedHint}</span>{" "}
          </>
        ) : null}
        {clause.items.map((item, idx) => {
          const rendered = renderClauseItem(item);
          if (rendered === null) {
            return null;
          }
          return (
            <Fragment key={item.id}>
              {idx > 0 ? " " : null}
              {rendered}
            </Fragment>
          );
        })}
      </span>
    );
  };

  return (
    <article
      ref={cardRef}
      className="unified-card"
      data-card-theme={theme}
      data-compact={compact ? "true" : undefined}
      data-card-kind="structured"
    >
      <div ref={wrapRef} className={`sent-wrap${traceVisible ? " has-trace" : ""}`}>
        <svg className="trace-svg" aria-hidden="true">
          <defs>
            <marker
              id={`${markerPrefix}-arr-1`}
              viewBox="0 0 10 10"
              refX="7"
              refY="5"
              markerWidth="5.5"
              markerHeight="5.5"
              orient="auto"
            >
              <path d="M 0 1 L 10 5 L 0 9 z" fill={traceColors[1]} />
            </marker>
            <marker
              id={`${markerPrefix}-arr-2`}
              viewBox="0 0 10 10"
              refX="7"
              refY="5"
              markerWidth="5.5"
              markerHeight="5.5"
              orient="auto"
            >
              <path d="M 0 1 L 10 5 L 0 9 z" fill={traceColors[2]} />
            </marker>
            <marker
              id={`${markerPrefix}-arr-3`}
              viewBox="0 0 10 10"
              refX="7"
              refY="5"
              markerWidth="5.5"
              markerHeight="5.5"
              orient="auto"
            >
              <path d="M 0 1 L 10 5 L 0 9 z" fill={traceColors[3]} />
            </marker>
          </defs>
          {traceArcs.map((arc) => (
            <path
              key={arc.key}
              d={arc.d}
              fill="none"
              stroke={traceColors[arc.index]}
              strokeWidth="1.65"
              strokeDasharray="4 3.5"
              opacity="0.88"
              markerEnd={`url(#${markerPrefix}-arr-${arc.index})`}
            />
          ))}
        </svg>

        <button
          type="button"
          className="audio-btn"
          title="朗读原句"
          aria-label="朗读原句"
          onClick={() => onSpeak?.(card.sourceText)}
        >
          <AudioIcon />
        </button>

        <div className="sent-body">
          {card.parts.map((part, idx) => {
            const leadingSpace = idx > 0 && part.kind !== "punct" ? " " : null;
            switch (part.kind) {
              case "punct":
                return <Fragment key={`punct-${idx}`}>{part.text}</Fragment>;
              case "token":
                return (
                  <Fragment key={part.id}>
                    {leadingSpace}
                    {renderWordToken(part)}
                  </Fragment>
                );
              case "clause":
                return (
                  <Fragment key={part.id}>
                    {leadingSpace}
                    {renderClause(part)}
                  </Fragment>
                );
              default: {
                const _exhaustive: never = part;
                return _exhaustive;
              }
            }
          })}
        </div>
      </div>

      <div className="controls-row">
        <div className="ctrl-btns">
          {clausesPresent ? (
            <button
              type="button"
              className={`ctrl-btn${foldedNow ? " active" : ""}`}
              data-action="fold-all"
              onClick={handleToggleFoldAll}
            >
              {foldedNow ? "展开全部从句" : "折叠修饰看主干"}
            </button>
          ) : null}
          {gapsPresent ? (
            <button
              type="button"
              className={`ctrl-btn${showGapLayer ? " active" : ""}`}
              data-action="toggle-gap"
              onClick={handleToggleGapLayer}
            >
              空位回指
            </button>
          ) : null}
          {omittedPresent ? (
            <button
              type="button"
              className={`ctrl-btn${showOmitted ? " active" : ""}`}
              data-action="toggle-omitted"
              onClick={() => setShowOmitted((prev) => !prev)}
            >
              显隐省略词
            </button>
          ) : null}
        </div>
        <CardLegend theme={theme} />
      </div>

      <div className="translation-box">{card.meaning}</div>

      <div className="inspector-box">
        <div className="inspector-head">
          <span>{activeNote ? `成分说明 · ${activeNote.title}` : "结构要点"}</span>
          {card.bookRef ? (
            <span className="book-chip">
              {card.bookRef.chapter} · {card.bookRef.section}
            </span>
          ) : null}
        </div>
        <div>{activeNote ? activeNote.note : card.recombination}</div>
        {card.caveatNote ? <div className="caveat-strip">{card.caveatNote}</div> : null}
      </div>
    </article>
  );
}

export function UnifiedCard({
  card,
  theme = "dark-ink",
  compact = false,
  onSpeak,
}: UnifiedCardProps) {
  switch (card.kind) {
    case "structured":
      return (
        <StructuredCardView
          card={card}
          theme={theme}
          compact={compact}
          onSpeak={onSpeak}
        />
      );
    case "fallback":
      return <FallbackCardView card={card} theme={theme} compact={compact} />;
    default: {
      const _exhaustive: never = card;
      return _exhaustive;
    }
  }
}
