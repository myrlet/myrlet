/**
 * The `?` keyboard Shortcuts sheet — read-only.
 *
 * Every entry here exists in `useWorkspaceShortcuts`; there are no Task write
 * entry points to list because this UI never creates or mutates Tasks. It is
 * separate from the ⌘K palette so each overlay owns its own focus lifecycle.
 */

import { useEffect, useId, useRef } from "react";

const SHORTCUTS: readonly { readonly keys: string; readonly label: string }[] = [
  { keys: "⌘K", label: "Open or close the Command Palette" },
  { keys: "F", label: "Fit the Board to all Task groups" },
  { keys: "Shift+F", label: "Center the selected Task on the Board" },
  { keys: "I", label: "Show or hide the Inspector" },
  { keys: "↑ / ↓", label: "Move the Task selection through the List" },
  { keys: "Esc", label: "Close the active overlay, or clear the selection" },
  { keys: "?", label: "Show this shortcut sheet" },
];

export interface ShortcutsModalProps {
  readonly open: boolean;
  readonly onClose: () => void;
}

export function ShortcutsModal({ open, onClose }: ShortcutsModalProps) {
  const titleId = useId();
  const closeRef = useRef<HTMLButtonElement | null>(null);

  useEffect(() => {
    if (!open) {
      return;
    }
    const frame = window.setTimeout(() => {
      closeRef.current?.focus();
    }, 0);
    return () => window.clearTimeout(frame);
  }, [open]);

  if (!open) {
    return null;
  }

  return (
    <div
      className="backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) {
          onClose();
        }
      }}
    >
      <div className="dialog-panel" role="dialog" aria-modal="true" aria-labelledby={titleId}>
        <div className="dialog-header">
          <div className="dialog-heading">
            <h2 className="dialog-title" id={titleId}>
              Keyboard shortcuts
            </h2>
            <p className="dialog-description">
              Everything in this console is reachable without the mouse.
            </p>
          </div>
          <button
            type="button"
            ref={closeRef}
            className="icon-button"
            aria-label="Close keyboard shortcuts"
            onClick={onClose}
          >
            <span aria-hidden="true">✕</span>
          </button>
        </div>

        <div className="dialog-body">
          <dl className="shortcut-list">
            {SHORTCUTS.map((shortcut) => (
              <div className="shortcut-row" key={shortcut.keys}>
                <dt>
                  <kbd className="kbd mono">{shortcut.keys}</kbd>
                </dt>
                <dd>{shortcut.label}</dd>
              </div>
            ))}
          </dl>
        </div>

        <div className="dialog-footer">
          <button type="button" className="button" onClick={onClose}>
            Close
          </button>
        </div>
      </div>
    </div>
  );
}
