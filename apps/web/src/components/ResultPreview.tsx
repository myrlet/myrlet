/**
 * Overview Result block: synced provenance, a readable Markdown preview, and
 * the way into the full reader when the text does not fit.
 *
 * The preview renders the complete Markdown through the normal renderer, then
 * measures the painted result. It keeps whole top-level blocks until roughly
 * six rendered lines fit; when even the first block is taller than that, it is
 * cropped at a whole-line boundary. Content below the crop is made `inert`, so
 * links the reader never shows cannot be reached with the keyboard. The copy
 * action and the reader always receive the original, unmodified text.
 */

import { Suspense, lazy, useCallback, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { Ref } from "react";
import { BookOpen, Copy } from "lucide-react";

import { formatRelativeTime, formatTimestamp } from "../format";

const ResultMarkdown = lazy(() => import("./ResultMarkdown"));

/** Roughly how many rendered lines the Overview preview offers. */
const PREVIEW_LINES = 6;
const INERT_MARK = "data-result-clipped";

export interface ResultPreviewProps {
  readonly markdown: string;
  readonly recordedAtMs: number | null;
  readonly onCopyText: (label: string, text: string) => void;
  /** Opens the full-result reader. */
  readonly onOpenReader: () => void;
  /** The `Read full result` trigger; focus returns here after closing. */
  readonly triggerRef?: Ref<HTMLButtonElement>;
}

interface PreviewClip {
  /** Visible height of the preview in CSS pixels. */
  readonly height: number;
  readonly truncated: boolean;
}

export function ResultPreview({
  markdown,
  recordedAtMs,
  onCopyText,
  onOpenReader,
  triggerRef,
}: ResultPreviewProps) {
  const clipRef = useRef<HTMLDivElement | null>(null);
  const [clip, setClip] = useState<PreviewClip | null>(null);
  const hasResult = markdown.trim() !== "";

  const measure = useCallback(() => {
    const container = clipRef.current;
    const content = container?.querySelector<HTMLElement>(".result-markdown");
    if (!container || !content) {
      return;
    }
    const next = measurePreview(content);
    markClippedContent(content, next);
    setClip((previous) =>
      previous !== null &&
      previous.height === next.height &&
      previous.truncated === next.truncated
        ? previous
        : next,
    );
  }, []);

  useLayoutEffect(() => {
    measure();
    const container = clipRef.current;
    if (container === null) {
      return;
    }

    const cleanups: (() => void)[] = [];

    // Re-measure when the rendered Markdown reflows: its own height changes on
    // every width change (Inspector drag, window resize, phone rotation).
    const content = container.querySelector<HTMLElement>(".result-markdown");
    if (typeof ResizeObserver === "function" && content !== null) {
      const observer = new ResizeObserver(() => measure());
      observer.observe(content);
      cleanups.push(() => observer.disconnect());
    }
    // Window resizes are cheap to observe and cover engines without observers.
    window.addEventListener("resize", measure);
    cleanups.push(() => window.removeEventListener("resize", measure));
    // The lazy renderer swaps its placeholder for the real Markdown later.
    if (typeof MutationObserver === "function") {
      const observer = new MutationObserver(() => measure());
      observer.observe(container, { childList: true, subtree: true });
      cleanups.push(() => observer.disconnect());
    }

    return () => {
      for (const cleanup of cleanups) {
        cleanup();
      }
    };
  }, [measure, markdown]);

  const clipStyle = useMemo(
    () => (clip?.truncated ? { maxHeight: `${clip.height}px` } : undefined),
    [clip],
  );
  const hiddenFromReaders = clip?.truncated === true;

  if (!hasResult) {
    return null;
  }

  return (
    <section className="detail-section result-section" aria-label="Task result">
      <div className="result-head">
        <h3 className="detail-section-title">Result</h3>
        <span className="header-spacer" />
        {recordedAtMs === null ? null : (
          <span
            className="result-recorded tabular"
            title={`Recorded ${formatTimestamp(recordedAtMs)}`}
          >
            Synced {formatRelativeTime(recordedAtMs)}
          </span>
        )}
        <button
          type="button"
          className="button button-sm button-ghost"
          onClick={() => onCopyText("Result", markdown)}
          title="Copy the complete result Markdown"
        >
          <Copy size={12} aria-hidden="true" />
          <span>Copy Markdown</span>
        </button>
      </div>

      <div className="result-preview">
        <div className="result-preview-clip" style={clipStyle} ref={clipRef}>
          <Suspense fallback={<pre className="result-preview-fallback">{markdown}</pre>}>
            <ResultMarkdown markdown={markdown} />
          </Suspense>
        </div>
      </div>

      {hiddenFromReaders ? (
        <button
          ref={triggerRef}
          type="button"
          className="button button-sm result-read-full"
          onClick={onOpenReader}
        >
          <BookOpen size={12} aria-hidden="true" />
          <span>Read full result</span>
        </button>
      ) : null}
    </section>
  );
}

/* --------------------------------------------------------------------- */
/* Measurement                                                            */
/* --------------------------------------------------------------------- */

/** Effective line box height, with a sane fallback when CSS is unavailable. */
function readLineHeight(content: HTMLElement): number {
  let style: CSSStyleDeclaration | null = null;
  try {
    style = window.getComputedStyle(content);
  } catch {
    style = null;
  }
  const lineHeight = Number.parseFloat(style?.lineHeight ?? "");
  if (Number.isFinite(lineHeight) && lineHeight > 0) {
    return lineHeight;
  }
  const fontSize = Number.parseFloat(style?.fontSize ?? "");
  return (Number.isFinite(fontSize) && fontSize > 0 ? fontSize : 12.5) * 1.6;
}

/**
 * Keep whole top-level blocks until the six-line budget would be exceeded.
 * A first block taller than the budget is cropped at a whole-line boundary.
 */
function measurePreview(content: HTMLElement): PreviewClip {
  const blocks = Array.from(content.children).filter(
    (child): child is HTMLElement => child instanceof HTMLElement,
  );
  if (blocks.length === 0) {
    return { height: 0, truncated: false };
  }

  const lineHeight = readLineHeight(content);
  const budget = lineHeight * PREVIEW_LINES;
  const contentTop = content.getBoundingClientRect().top;

  let cutoff = 0;
  let lastBottom = 0;
  for (const block of blocks) {
    const rect = block.getBoundingClientRect();
    const bottom = rect.bottom - contentTop;
    lastBottom = Math.max(lastBottom, bottom);
    if (bottom <= budget + 0.5) {
      cutoff = bottom;
    }
  }

  if (lastBottom <= budget + 0.5) {
    return { height: Math.max(0, lastBottom), truncated: false };
  }

  if (cutoff <= 0) {
    // Not even the first block fits: crop by whole rendered lines.
    cutoff = Math.max(lineHeight, Math.floor(budget / lineHeight) * lineHeight);
  }
  return { height: cutoff, truncated: true };
}

/**
 * Make everything below the crop unreachable: whole hidden blocks and any
 * focusable whose painted box runs past the cut (a partially visible link).
 */
function markClippedContent(content: HTMLElement, clip: PreviewClip): void {
  for (const element of Array.from(content.querySelectorAll<HTMLElement>(`[${INERT_MARK}]`))) {
    element.removeAttribute("inert");
    element.removeAttribute(INERT_MARK);
  }
  if (!clip.truncated) {
    return;
  }

  const mark = (element: HTMLElement): void => {
    if (!element.hasAttribute(INERT_MARK)) {
      element.setAttribute("inert", "");
      element.setAttribute(INERT_MARK, "");
    }
  };

  const contentTop = content.getBoundingClientRect().top;
  for (const block of Array.from(content.children)) {
    if (!(block instanceof HTMLElement)) {
      continue;
    }
    const rect = block.getBoundingClientRect();
    if (rect.top - contentTop >= clip.height - 0.5) {
      mark(block);
    }
  }

  const focusables = content.querySelectorAll<HTMLElement>(
    "a[href], button, input, select, textarea, [tabindex]",
  );
  for (const element of Array.from(focusables)) {
    if (element.closest(`[${INERT_MARK}]`) !== null) {
      continue;
    }
    const rect = element.getBoundingClientRect();
    const top = rect.top - contentTop;
    const bottom = rect.bottom - contentTop;
    if (top >= clip.height - 0.5 || bottom > clip.height + 0.5) {
      mark(element);
    }
  }
}
