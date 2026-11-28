/**
 * Three lightweight console surfaces: the ⌘K Command Palette, the `?` keyboard
 * Shortcuts sheet, and the mutation Toast stack.
 *
 * The palette opens and closes instantly (no animation) per Emil Kowalski's
 * guidance: a command surface should feel like it was already there.
 */

import { useCallback, useEffect, useId, useMemo, useRef, useState } from "react";
import type { KeyboardEvent as ReactKeyboardEvent } from "react";

import type { SnapshotTaskSummary, TaskRecord } from "../api/types";
import { TASK_STATUS_META, taskTitle } from "../format";
import type { Theme } from "../state/useAppearance";
import type { WorkspaceMode } from "./TopBar";

/* --------------------------------------------------------------------- */
/* Command Palette                                                        */
/* --------------------------------------------------------------------- */

type CommandGroup = "Tasks" | "Views" | "Appearance" | "Actions";

interface Command {
  readonly id: string;
  readonly group: CommandGroup;
  readonly label: string;
  readonly hint?: string;
  readonly keywords: string;
  readonly run: () => void;
}

const VIEW_OPTIONS: readonly { readonly mode: WorkspaceMode; readonly label: string }[] = [
  { mode: "board", label: "Board" },
  { mode: "list", label: "List" },
];

const THEME_OPTIONS: readonly {
  readonly theme: Theme;
  readonly label: string;
  readonly hint: string;
}[] = [
  { theme: "system", label: "System", hint: "follow browser" },
  { theme: "light", label: "Light", hint: "appearance" },
  { theme: "dark", label: "Dark", hint: "appearance" },
];

const GROUP_ORDER: readonly CommandGroup[] = ["Tasks", "Views", "Appearance", "Actions"];

export interface CommandPaletteProps {
  readonly open: boolean;
  readonly tasks: readonly SnapshotTaskSummary[];
  readonly selectedTask: TaskRecord | null;
  readonly theme: Theme;
  readonly onClose: () => void;
  readonly onSelectTask: (taskId: string) => void;
  readonly onSelectMode: (mode: WorkspaceMode) => void;
  readonly onOpenWorkers: () => void;
  readonly onToggleInspector: () => void;
  readonly onFitBoard: () => void;
  readonly onArrangeBoard: () => void;
  readonly onChangeTheme: (theme: Theme) => void;
}

export function CommandPalette(props: CommandPaletteProps) {
  const {
    open,
    tasks,
    selectedTask,
    theme,
    onClose,
    onSelectTask,
    onSelectMode,
    onOpenWorkers,
    onToggleInspector,
    onFitBoard,
    onArrangeBoard,
    onChangeTheme,
  } = props;

  const [query, setQuery] = useState("");
  const [activeIndex, setActiveIndex] = useState(0);
  const listRef = useRef<HTMLDivElement | null>(null);
  const titleId = useId();

  useEffect(() => {
    if (open) {
      setQuery("");
      setActiveIndex(0);
    }
  }, [open]);

  const commands = useMemo<Command[]>(() => {
    const taskCommands: Command[] = tasks.slice(0, 30).map((task) => ({
      id: `task:${task.id}`,
      group: "Tasks",
      label: taskTitle(task),
      hint: `${TASK_STATUS_META[task.status].label} · ${task.id}`,
      keywords: `${task.id} ${taskTitle(task)} ${task.category}`.toLowerCase(),
      run: () => onSelectTask(task.id),
    }));

    const viewCommands: Command[] = VIEW_OPTIONS.map((option) => ({
      id: `view:${option.mode}`,
      group: "Views",
      label: option.label,
      hint: "Switch view",
      keywords: `${option.mode} view ${option.label}`.toLowerCase(),
      run: () => onSelectMode(option.mode),
    }));

    const actionCommands: Command[] = [
      {
        id: "action:workers",
        group: "Actions",
        label: "Workers & Pairing",
        hint: "drawer",
        keywords: "workers pairing slots pair enrollment rotate revoke",
        run: onOpenWorkers,
      },
      {
        id: "action:inspector",
        group: "Actions",
        label: "Toggle Inspector",
        hint: selectedTask === null ? "i" : `i · ${selectedTask.id}`,
        keywords: "inspector detail panel toggle",
        run: onToggleInspector,
      },
      {
        id: "action:fit-board",
        group: "Actions",
        label: "Fit Board to view",
        hint: "F",
        keywords: "fit board zoom frame atlas",
        run: onFitBoard,
      },
      {
        id: "action:arrange-board",
        group: "Actions",
        label: "Arrange Board layout",
        hint: "layout",
        keywords: "arrange board layout reorganize atlas",
        run: onArrangeBoard,
      },
    ];

    const themeCommands: Command[] = THEME_OPTIONS.map((option) => ({
      id: `theme:${option.theme}`,
      group: "Appearance",
      label: option.label,
      hint: theme === option.theme ? "current" : option.hint,
      keywords: `theme appearance ${option.theme} ${option.label}`.toLowerCase(),
      run: () => onChangeTheme(option.theme),
    }));

    return [...taskCommands, ...viewCommands, ...actionCommands, ...themeCommands];
  }, [
    tasks,
    theme,
    selectedTask,
    onSelectTask,
    onSelectMode,
    onOpenWorkers,
    onToggleInspector,
    onFitBoard,
    onArrangeBoard,
    onChangeTheme,
  ]);

  const trimmedQuery = query.trim().toLowerCase();

  const visible = useMemo(
    () =>
      trimmedQuery === ""
        ? commands
        : commands.filter((command) => command.keywords.includes(trimmedQuery)),
    [commands, trimmedQuery],
  );

  // Keep the active index in range as the filter narrows.
  useEffect(() => {
    setActiveIndex((current) => {
      if (visible.length === 0) {
        return 0;
      }
      return Math.min(current, visible.length - 1);
    });
  }, [visible.length]);

  useEffect(() => {
    if (!open) {
      return;
    }
    const node = listRef.current?.querySelector<HTMLElement>('[data-active="true"]');
    node?.scrollIntoView?.({ block: "nearest" });
  }, [activeIndex, open]);

  const activate = useCallback(
    (command: Command | undefined) => {
      if (!command) {
        return;
      }
      command.run();
      onClose();
    },
    [onClose],
  );

  const handleKeyDown = useCallback(
    (event: ReactKeyboardEvent<HTMLInputElement>) => {
      switch (event.key) {
        case "Escape":
          // Stop the global handler from also clearing selection.
          event.stopPropagation();
          event.preventDefault();
          onClose();
          break;
        case "ArrowDown":
          event.preventDefault();
          setActiveIndex((current) =>
            visible.length === 0 ? 0 : (current + 1) % visible.length,
          );
          break;
        case "ArrowUp":
          event.preventDefault();
          setActiveIndex((current) =>
            visible.length === 0 ? 0 : (current - 1 + visible.length) % visible.length,
          );
          break;
        case "Enter":
          event.preventDefault();
          activate(visible[activeIndex]);
          break;
        default:
          break;
      }
    },
    [activate, activeIndex, onClose, visible],
  );

  if (!open) {
    return null;
  }

  let flatIndex = -1;

  return (
    <div
      className="palette-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) {
          onClose();
        }
      }}
    >
      <div className="palette-panel" role="dialog" aria-modal="true" aria-labelledby={titleId}>
        <h2 className="visually-hidden" id={titleId}>
          Command Palette
        </h2>
        <div className="palette-search">
          <span className="palette-search-glyph" aria-hidden="true">
            ⌘K
          </span>
          <input
            className="palette-input"
            type="text"
            autoFocus
            aria-label="Search commands, Tasks, views, and appearance"
            placeholder="Search Tasks, views, actions, appearance…"
            value={query}
            onChange={(event) => {
              setQuery(event.target.value);
              setActiveIndex(0);
            }}
            onKeyDown={handleKeyDown}
          />
        </div>

        <div className="palette-list" ref={listRef} role="listbox" aria-label="Commands">
          {visible.length === 0 ? (
            <p className="palette-empty">No command matches “{query.trim()}”.</p>
          ) : (
            GROUP_ORDER.map((group) => {
              const items = visible.filter((command) => command.group === group);
              if (items.length === 0) {
                return null;
              }
              return (
                <div className="palette-group" key={group}>
                  <div className="palette-group-title">{group}</div>
                  {items.map((command) => {
                    flatIndex += 1;
                    const index = flatIndex;
                    const isActive = index === activeIndex;
                    return (
                      <button
                        key={command.id}
                        type="button"
                        className="palette-item"
                        role="option"
                        aria-selected={isActive}
                        data-active={isActive}
                        onMouseEnter={() => setActiveIndex(index)}
                        onClick={() => activate(command)}
                      >
                        <span className="palette-item-label">{command.label}</span>
                        {command.hint ? (
                          <span className="palette-item-hint mono">{command.hint}</span>
                        ) : null}
                      </button>
                    );
                  })}
                </div>
              );
            })
          )}
        </div>

        <div className="palette-footer">
          <span className="mono">↑ ↓</span>
          <span>navigate</span>
          <span className="mono">↵</span>
          <span>run</span>
          <span className="mono">Esc</span>
          <span>close</span>
        </div>
      </div>
    </div>
  );
}

/* --------------------------------------------------------------------- */
/* Shortcuts sheet                                                        */
/* --------------------------------------------------------------------- */

/**
 * Read-only shortcut sheet. Every entry here exists in `App.tsx`; there are no
 * Task write entry points to list because this UI never creates or mutates Tasks.
 */
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

/* --------------------------------------------------------------------- */
/* Toasts                                                                 */
/* --------------------------------------------------------------------- */

export interface ToastItem {
  readonly id: string;
  readonly message: string;
  readonly tone?: "neutral" | "error";
}

export interface ToastStackProps {
  readonly toasts: readonly ToastItem[];
  readonly onDismiss: (id: string) => void;
}

export function ToastStack({ toasts, onDismiss }: ToastStackProps) {
  if (toasts.length === 0) {
    return null;
  }

  return (
    <div className="toast-stack" role="region" aria-label="Notifications" aria-live="polite">
      {toasts.map((toast) => (
        <div
          className="toast"
          data-tone={toast.tone ?? "neutral"}
          role={toast.tone === "error" ? "alert" : "status"}
          key={toast.id}
        >
          <span className="toast-message">{toast.message}</span>
          <button
            type="button"
            className="toast-dismiss"
            aria-label={`Dismiss notification: ${toast.message}`}
            onClick={() => onDismiss(toast.id)}
          >
            <span aria-hidden="true">✕</span>
          </button>
        </div>
      ))}
    </div>
  );
}

