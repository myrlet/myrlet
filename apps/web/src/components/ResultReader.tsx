/**
 * Full Result reader, built on the native `<dialog>` element.
 *
 * The dialog is modal: the workspace goes inert, focus stays inside, and the
 * browser handles the wide desktop / full-screen phone layout through CSS.
 * Opening never mutates Task state, tab choice, or the Overview reading
 * position; closing restores the trigger's focus and the scroll offset the
 * caller recorded before opening.
 *
 * The rendered Markdown is the complete immutable result text. Tables and
 * fenced code scroll horizontally inside their own containers so the dialog
 * itself never scrolls sideways.
 */

import { Suspense, lazy, useLayoutEffect, useRef } from "react";
import type {
  KeyboardEvent as ReactKeyboardEvent,
  MouseEvent as ReactMouseEvent,
  SyntheticEvent,
} from "react";
import { Copy, X } from "lucide-react";

import { formatRelativeTime, formatTimestamp } from "../format";

const ResultMarkdown = lazy(() => import("./ResultMarkdown"));

export interface ResultReaderProps {
  /** Task title, shown as the reader heading. */
  readonly title: string;
  /** Complete Result Markdown; never a preview. */
  readonly markdown: string;
  readonly recordedAtMs: number | null;
  readonly onCopyText: (label: string, text: string) => void;
  /** Called for every close path: close button, Escape, or a backdrop click. */
  readonly onRequestClose: () => void;
}

export function ResultReader({
  title,
  markdown,
  recordedAtMs,
  onCopyText,
  onRequestClose,
}: ResultReaderProps) {
  const dialogRef = useRef<HTMLDialogElement | null>(null);
  const closeButtonRef = useRef<HTMLButtonElement | null>(null);

  // Open modally once mounted. jsdom (tests) implements `<dialog>` without
  // `showModal`, so fall back to the reflected `open` attribute there.
  useLayoutEffect(() => {
    const dialog = dialogRef.current;
    if (dialog === null) {
      return;
    }
    if (typeof dialog.showModal === "function") {
      if (!dialog.open) {
        dialog.showModal();
      }
    } else {
      dialog.setAttribute("open", "");
    }
    closeButtonRef.current?.focus();
    return () => {
      // Unmounting a modal dialog leaves the top layer explicitly.
      if (typeof dialog.close === "function" && dialog.open) {
        dialog.close();
      }
    };
  }, []);

  const handleKeyDown = (event: ReactKeyboardEvent<HTMLDialogElement>): void => {
    // The reader is modal: workspace shortcuts never fire underneath it.
    event.stopPropagation();
    if (event.key === "Escape") {
      // Escape closes only the reader; it must not clear the Task selection.
      event.preventDefault();
      onRequestClose();
    }
  };

  const handleCancel = (event: SyntheticEvent<HTMLDialogElement>): void => {
    // Own the close so focus and scroll restoration run on every path.
    event.preventDefault();
    onRequestClose();
  };

  const handleClick = (event: ReactMouseEvent<HTMLDialogElement>): void => {
    // The dialog element itself is the backdrop; clicks inside land on content.
    if (event.target === event.currentTarget) {
      onRequestClose();
    }
  };

  return (
    <dialog
      ref={dialogRef}
      className="result-reader"
      aria-label={`Result — ${title}`}
      onCancel={handleCancel}
      onKeyDown={handleKeyDown}
      onClick={handleClick}
    >
      <header className="result-reader-header">
        <div className="result-reader-heading">
          <span className="result-reader-kicker">Result</span>
          <h2 className="result-reader-title" id="result-reader-title">
            {title}
          </h2>
          {recordedAtMs === null ? null : (
            <span
              className="result-reader-recorded"
              title={`Recorded ${formatTimestamp(recordedAtMs)}`}
            >
              Synced {formatRelativeTime(recordedAtMs)}
            </span>
          )}
        </div>
        <span className="header-spacer" />
        <button
          type="button"
          className="button button-sm button-ghost"
          onClick={() => onCopyText("Result", markdown)}
          title="Copy the complete result Markdown"
        >
          <Copy size={12} aria-hidden="true" />
          <span>Copy Markdown</span>
        </button>
        <button
          ref={closeButtonRef}
          type="button"
          className="icon-button"
          aria-label="Close result reader"
          title="Close result reader"
          onClick={onRequestClose}
        >
          <X size={15} aria-hidden="true" />
        </button>
      </header>
      <div className="result-reader-body">
        <Suspense fallback={<pre className="result-reader-fallback">{markdown}</pre>}>
          <ResultMarkdown markdown={markdown} />
        </Suspense>
      </div>
    </dialog>
  );
}
