/**
 * Action dialogs for Task creation, Bundle import, Dispatch, Cancel, and Finish.
 *
 * Every dialog is a plain `role="dialog" aria-modal="true"` panel that closes on
 * backdrop click and Escape, disables its submit control while busy, and shows
 * `HubApiError` failures inline instead of inventing a success state.
 */

import { useCallback, useEffect, useId, useMemo, useRef, useState } from "react";
import type { ReactNode } from "react";

import { HubApiError } from "../api/client";
import type {
  CancelTaskRequest,
  DispatchSelection,
  DispatchTaskRequest,
  FinishTaskRequest,
  TaskBundleImportRequest,
  TaskDefinitionInput,
  TaskRecord,
  WorkerRecord,
} from "../api/types";
import { formatCapacity, isCoordinationFinishable } from "../format";
import { TaskStatusBadge } from "./StatusBadge";

/* --------------------------------------------------------------------- */
/* Shared helpers                                                         */
/* --------------------------------------------------------------------- */

function describeError(error: unknown): string {
  if (error instanceof HubApiError) {
    return error.message;
  }
  if (error instanceof Error) {
    return error.message;
  }
  return "The Hub command could not be completed.";
}

function parseCapabilities(raw: string): string[] {
  const seen = new Set<string>();
  const result: string[] = [];
  for (const part of raw.split(",")) {
    const value = part.trim();
    if (value !== "" && !seen.has(value)) {
      seen.add(value);
      result.push(value);
    }
  }
  return result;
}

/* --------------------------------------------------------------------- */
/* Dialog shell                                                           */
/* --------------------------------------------------------------------- */

interface DialogShellProps {
  readonly open: boolean;
  readonly title: string;
  readonly description?: string;
  readonly wide?: boolean;
  readonly busy?: boolean;
  readonly error?: string | null;
  readonly footerNote?: string;
  readonly submitLabel: string;
  readonly submitBusyLabel: string;
  readonly submitDisabled?: boolean;
  readonly danger?: boolean;
  readonly onClose: () => void;
  readonly onSubmit: () => void;
  readonly children: ReactNode;
}

function DialogShell({
  open,
  title,
  description,
  wide = false,
  busy = false,
  error = null,
  footerNote,
  submitLabel,
  submitBusyLabel,
  submitDisabled = false,
  danger = false,
  onClose,
  onSubmit,
  children,
}: DialogShellProps) {
  const titleId = useId();
  const descriptionId = useId();
  const panelRef = useRef<HTMLFormElement | null>(null);

  useEffect(() => {
    if (!open) {
      return;
    }
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.key === "Escape") {
        event.stopPropagation();
        if (!busy) {
          onClose();
        }
        return;
      }
      if (event.key !== "Tab") {
        return;
      }
      const panel = panelRef.current;
      if (!panel) {
        return;
      }
      const focusable = panel.querySelectorAll<HTMLElement>(
        'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
      );
      if (focusable.length === 0) {
        return;
      }
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      const active = document.activeElement;
      if (event.shiftKey && (active === first || !panel.contains(active))) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && active === last) {
        event.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", onKeyDown);
    const frame = window.setTimeout(() => {
      const panel = panelRef.current;
      if (!panel) {
        return;
      }
      const target = panel.querySelector<HTMLElement>("[data-autofocus]");
      (target ?? panel.querySelector<HTMLElement>("input, textarea, select, button"))?.focus();
    }, 0);
    return () => {
      document.removeEventListener("keydown", onKeyDown);
      window.clearTimeout(frame);
    };
  }, [open, busy, onClose]);

  if (!open) {
    return null;
  }

  return (
    <div
      className="backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget && !busy) {
          onClose();
        }
      }}
    >
      <form
        ref={panelRef}
        className="dialog-panel"
        data-size={wide ? "wide" : undefined}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={description ? descriptionId : undefined}
        noValidate
        onSubmit={(event) => {
          event.preventDefault();
          if (!busy && !submitDisabled) {
            onSubmit();
          }
        }}
      >
        <div className="dialog-header">
          <div className="dialog-heading">
            <h2 className="dialog-title" id={titleId}>
              {title}
            </h2>
            {description ? (
              <p className="dialog-description" id={descriptionId}>
                {description}
              </p>
            ) : null}
          </div>
          <button
            type="button"
            className="icon-button"
            aria-label="Close dialog"
            disabled={busy}
            onClick={onClose}
          >
            <span aria-hidden="true">✕</span>
          </button>
        </div>

        <div className="dialog-body">
          {error ? (
            <div className="alert alert-error" role="alert">
              <span aria-hidden="true">!</span>
              <span>{error}</span>
            </div>
          ) : null}
          {children}
        </div>

        <div className="dialog-footer">
          {footerNote ? <span className="dialog-footer-note">{footerNote}</span> : null}
          <button type="button" className="button" disabled={busy} onClick={onClose}>
            {busy ? "Working…" : "Close"}
          </button>
          <button
            type="submit"
            className={danger ? "button button-danger" : "button button-primary"}
            aria-busy={busy ? "true" : undefined}
            disabled={busy || submitDisabled}
          >
            {busy ? submitBusyLabel : submitLabel}
          </button>
        </div>
      </form>
    </div>
  );
}

/* --------------------------------------------------------------------- */
/* New Task / Copy / Retry                                                */
/* --------------------------------------------------------------------- */

export interface NewTaskDialogProps {
  readonly open: boolean;
  readonly sourceTask?: TaskRecord | null;
  readonly onClose: () => void;
  readonly onSubmit: (definition: TaskDefinitionInput) => Promise<void>;
}

interface NewTaskFormState {
  title: string;
  prompt: string;
  category: string;
  parentTaskId: string;
  requiredCapabilities: string;
  preferredWorkerId: string;
  preferredExecutorId: string;
}

function newTaskForm(sourceTask: TaskRecord | null | undefined): NewTaskFormState {
  if (!sourceTask) {
    return {
      title: "",
      prompt: "",
      category: "",
      parentTaskId: "",
      requiredCapabilities: "",
      preferredWorkerId: "",
      preferredExecutorId: "",
    };
  }
  return {
    title: `${sourceTask.task.title} (retry)`,
    prompt: sourceTask.task.prompt,
    category: sourceTask.task.category,
    parentTaskId: sourceTask.task.parentTaskId ?? "",
    requiredCapabilities: (sourceTask.task.routing.requiredCapabilities ?? []).join(", "),
    preferredWorkerId: sourceTask.task.routing.preferredWorkerId ?? "",
    preferredExecutorId: sourceTask.task.routing.preferredExecutorId ?? "",
  };
}

export function NewTaskDialog({ open, sourceTask, onClose, onSubmit }: NewTaskDialogProps) {
  const [form, setForm] = useState<NewTaskFormState>(() => newTaskForm(sourceTask));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [showValidation, setShowValidation] = useState(false);

  useEffect(() => {
    if (open) {
      setForm(newTaskForm(sourceTask));
      setError(null);
      setBusy(false);
      setShowValidation(false);
    }
  }, [open, sourceTask]);

  const update = useCallback(
    (patch: Partial<NewTaskFormState>) => {
      setForm((current) => ({ ...current, ...patch }));
    },
    [],
  );

  const titleMissing = form.title.trim() === "";
  const promptMissing = form.prompt.trim() === "";

  const submit = useCallback(async (): Promise<void> => {
    if (titleMissing || promptMissing) {
      setShowValidation(true);
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const capabilities = parseCapabilities(form.requiredCapabilities);
      const routing: TaskDefinitionInput["routing"] = {};
      if (capabilities.length > 0) {
        routing.requiredCapabilities = capabilities;
      }
      if (form.preferredWorkerId.trim() !== "") {
        routing.preferredWorkerId = form.preferredWorkerId.trim();
      }
      if (form.preferredExecutorId.trim() !== "") {
        routing.preferredExecutorId = form.preferredExecutorId.trim();
      }
      const definition: TaskDefinitionInput = {
        title: form.title.trim(),
        prompt: form.prompt,
      };
      const category = form.category.trim();
      if (category !== "") {
        definition.category = category;
      }
      if (form.parentTaskId.trim() !== "") {
        definition.parentTaskId = form.parentTaskId.trim();
      }
      if (sourceTask) {
        definition.sourceTaskId = sourceTask.id;
      }
      if (Object.keys(routing).length > 0) {
        definition.routing = routing;
      }
      await onSubmit(definition);
      onClose();
    } catch (submitError) {
      setError(describeError(submitError));
    } finally {
      setBusy(false);
    }
  }, [form, onClose, onSubmit, promptMissing, sourceTask, titleMissing]);

  return (
    <DialogShell
      open={open}
      title={sourceTask ? "Copy Task as Retry" : "New Task"}
      description={
        sourceTask
          ? "Creates a new held Task linked to the source by sourceTaskId. The source Task is never modified."
          : "Creates one durable held Task. Nothing executes until you dispatch it."
      }
      wide
      busy={busy}
      error={error}
      footerNote="Creates a held Task; execution requires explicit Dispatch."
      submitLabel={sourceTask ? "Create retry Task" : "Create Task"}
      submitBusyLabel="Creating…"
      onClose={onClose}
      onSubmit={() => {
        void submit();
      }}
    >
      {sourceTask ? (
        <div className="alert alert-info">
          <span className="alert-title">Source Task</span>
          <span className="inline">
            <span className="mono">{sourceTask.id}</span>
            <TaskStatusBadge status={sourceTask.status} />
          </span>
        </div>
      ) : null}

      <div className="form-grid">
        <div className="field field-wide">
          <label className="field-label" htmlFor="new-task-title">
            Title
          </label>
          <input
            id="new-task-title"
            className="input"
            data-autofocus
            value={form.title}
            disabled={busy}
            aria-invalid={showValidation && titleMissing ? true : undefined}
            onChange={(event) => update({ title: event.target.value })}
          />
          {showValidation && titleMissing ? (
            <span className="field-error">A title is required.</span>
          ) : null}
        </div>

        <div className="field field-wide">
          <label className="field-label" htmlFor="new-task-prompt">
            Prompt
          </label>
          <textarea
            id="new-task-prompt"
            className="textarea"
            rows={5}
            value={form.prompt}
            disabled={busy}
            aria-invalid={showValidation && promptMissing ? true : undefined}
            onChange={(event) => update({ prompt: event.target.value })}
          />
          {showValidation && promptMissing ? (
            <span className="field-error">A prompt is required.</span>
          ) : null}
        </div>

        <div className="field">
          <label className="field-label" htmlFor="new-task-category">
            Category
          </label>
          <input
            id="new-task-category"
            className="input input-mono"
            placeholder="general"
            value={form.category}
            disabled={busy}
            onChange={(event) => update({ category: event.target.value })}
          />
        </div>

        <div className="field">
          <label className="field-label" htmlFor="new-task-parent">
            Parent Task ID
          </label>
          <input
            id="new-task-parent"
            className="input input-mono"
            placeholder="optional"
            value={form.parentTaskId}
            disabled={busy}
            onChange={(event) => update({ parentTaskId: event.target.value })}
          />
        </div>

        <div className="field field-wide">
          <label className="field-label" htmlFor="new-task-capabilities">
            Required Capabilities
          </label>
          <input
            id="new-task-capabilities"
            className="input input-mono"
            placeholder="rust, docker"
            value={form.requiredCapabilities}
            disabled={busy}
            onChange={(event) => update({ requiredCapabilities: event.target.value })}
          />
          <span className="field-hint">
            Comma-separated hard constraints. A Slot must report every capability as available.
          </span>
        </div>

        <div className="field">
          <label className="field-label" htmlFor="new-task-worker">
            Preferred Worker ID
          </label>
          <input
            id="new-task-worker"
            className="input input-mono"
            placeholder="optional"
            value={form.preferredWorkerId}
            disabled={busy}
            onChange={(event) => update({ preferredWorkerId: event.target.value })}
          />
        </div>

        <div className="field">
          <label className="field-label" htmlFor="new-task-executor">
            Preferred Executor ID
          </label>
          <input
            id="new-task-executor"
            className="input input-mono"
            placeholder="optional"
            value={form.preferredExecutorId}
            disabled={busy}
            onChange={(event) => update({ preferredExecutorId: event.target.value })}
          />
        </div>
      </div>
    </DialogShell>
  );
}

/* --------------------------------------------------------------------- */
/* Bundle import                                                          */
/* --------------------------------------------------------------------- */

export interface ImportBundleDialogProps {
  readonly open: boolean;
  readonly onClose: () => void;
  readonly onSubmit: (request: TaskBundleImportRequest) => Promise<void>;
}

const BUNDLE_PLACEHOLDER = `{
  "tasks": [
    {
      "clientRef": "root",
      "title": "Prepare release",
      "prompt": "..."
    },
    {
      "clientRef": "child",
      "title": "Run tests",
      "prompt": "...",
      "parentTaskId": "root"
    }
  ]
}`;

function parseBundle(raw: string): TaskBundleImportRequest {
  const trimmed = raw.trim();
  if (trimmed === "") {
    throw new Error("Paste a Bundle JSON document first.");
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(trimmed) as unknown;
  } catch {
    throw new Error("This is not valid JSON.");
  }
  if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
    throw new Error('Expected an object with a "tasks" array.');
  }
  const rawTasks: unknown = (parsed as { tasks?: unknown }).tasks;
  if (!Array.isArray(rawTasks)) {
    throw new Error('Expected a "tasks" array.');
  }
  const tasks: readonly unknown[] = rawTasks;
  if (tasks.length === 0) {
    throw new Error("A Bundle needs at least one Task.");
  }
  if (tasks.length > 100) {
    throw new Error("A Bundle may contain at most 100 Tasks.");
  }
  tasks.forEach((item, index) => {
    if (typeof item !== "object" || item === null || Array.isArray(item)) {
      throw new Error(`Task ${index + 1} is not an object.`);
    }
    const title = (item as { title?: unknown }).title;
    const prompt = (item as { prompt?: unknown }).prompt;
    if (typeof title !== "string" || title.trim() === "") {
      throw new Error(`Task ${index + 1} needs a title.`);
    }
    if (typeof prompt !== "string" || prompt.trim() === "") {
      throw new Error(`Task ${index + 1} needs a prompt.`);
    }
  });
  return { tasks: tasks as TaskBundleImportRequest["tasks"] };
}

export function ImportBundleDialog({ open, onClose, onSubmit }: ImportBundleDialogProps) {
  const [raw, setRaw] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (open) {
      setRaw("");
      setError(null);
      setBusy(false);
    }
  }, [open]);

  const submit = useCallback(async (): Promise<void> => {
    let request: TaskBundleImportRequest;
    try {
      request = parseBundle(raw);
    } catch (parseError) {
      setError(describeError(parseError));
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await onSubmit(request);
      onClose();
    } catch (submitError) {
      setError(describeError(submitError));
    } finally {
      setBusy(false);
    }
  }, [onClose, onSubmit, raw]);

  return (
    <DialogShell
      open={open}
      title="Import Task Bundle"
      description="Creates up to 100 held Tasks atomically, resolving bundle-local clientRef parents and sibling dependencies. It never dispatches."
      wide
      busy={busy}
      error={error}
      submitLabel="Import Bundle"
      submitBusyLabel="Importing…"
      onClose={onClose}
      onSubmit={() => {
        void submit();
      }}
    >
      <div className="field">
        <label className="field-label" htmlFor="bundle-json">
          Bundle JSON
        </label>
        <textarea
          id="bundle-json"
          className="textarea code-block"
          data-autofocus
          rows={14}
          spellCheck={false}
          placeholder={BUNDLE_PLACEHOLDER}
          value={raw}
          disabled={busy}
          onChange={(event) => setRaw(event.target.value)}
        />
        <span className="field-hint">
          Shape: <span className="mono">{'{"tasks":[{"clientRef":"root","title":"...","prompt":"..."}]}'}</span>
        </span>
      </div>
    </DialogShell>
  );
}

/* --------------------------------------------------------------------- */
/* Dispatch                                                               */
/* --------------------------------------------------------------------- */

export interface DispatchDialogProps {
  readonly task: TaskRecord | null;
  readonly workers: readonly WorkerRecord[];
  readonly onClose: () => void;
  readonly onSubmit: (taskId: string, request: DispatchTaskRequest) => Promise<void>;
}

type DispatchMode = "match" | "targeted";

export function DispatchDialog({ task, workers, onClose, onSubmit }: DispatchDialogProps) {
  const [mode, setMode] = useState<DispatchMode>("match");
  const [fallbackAllowed, setFallbackAllowed] = useState(false);
  const [queue, setQueue] = useState(false);
  const [workerId, setWorkerId] = useState("");
  const [slotId, setSlotId] = useState("");
  const [executorId, setExecutorId] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const open = task !== null;

  // Snapshot refreshes replace `workers` frequently; read the freshest list when
  // the dialog opens without resetting the operator's choices on every refresh.
  const workersRef = useRef<readonly WorkerRecord[]>(workers);
  useEffect(() => {
    workersRef.current = workers;
  }, [workers]);

  useEffect(() => {
    if (!open) {
      return;
    }
    setMode("match");
    setFallbackAllowed(false);
    setQueue(false);
    setError(null);
    setBusy(false);
    const current = workersRef.current;
    const firstOnline = current.find((worker) => worker.online && !worker.revoked);
    const initial = firstOnline ?? current.find((worker) => !worker.revoked) ?? null;
    setWorkerId(initial?.workerId ?? "");
    const initialSlot = initial?.inventory?.slots.find((slot) => slot.enabled) ?? null;
    setSlotId(initialSlot?.slotId ?? "");
    setExecutorId("");
  }, [open, task?.id]);

  const selectableWorkers = useMemo(
    () => workers.filter((worker) => !worker.revoked),
    [workers],
  );

  const selectedWorker = useMemo(
    () => selectableWorkers.find((worker) => worker.workerId === workerId) ?? null,
    [selectableWorkers, workerId],
  );

  const slots = useMemo(() => selectedWorker?.inventory?.slots ?? [], [selectedWorker]);

  const selectedSlot = useMemo(
    () => slots.find((slot) => slot.slotId === slotId) ?? null,
    [slots, slotId],
  );

  const executors = useMemo(() => selectedSlot?.executors ?? [], [selectedSlot]);

  const targetedIncomplete = mode === "targeted" && (workerId === "" || slotId === "");

  const submit = useCallback(async (): Promise<void> => {
    if (!task) {
      return;
    }
    if (targetedIncomplete) {
      setError("Choose a Worker and a Slot for a targeted dispatch.");
      return;
    }
    const selection: DispatchSelection =
      mode === "targeted"
        ? {
            mode: "targeted",
            workerId,
            slotId,
            executorId: executorId === "" ? null : executorId,
          }
        : fallbackAllowed
          ? { mode: "match", fallbackAllowed: true }
          : { mode: "match" };
    const request: DispatchTaskRequest = {
      expectedTaskVersion: task.version,
      selection,
      queue,
    };
    setBusy(true);
    setError(null);
    try {
      await onSubmit(task.id, request);
      onClose();
    } catch (submitError) {
      setError(describeError(submitError));
    } finally {
      setBusy(false);
    }
  }, [
    executorId,
    fallbackAllowed,
    mode,
    onClose,
    onSubmit,
    queue,
    slotId,
    task,
    targetedIncomplete,
    workerId,
  ]);

  return (
    <DialogShell
      open={open}
      title="Dispatch Task"
      description={
        task
          ? `Authorizes execution of ${task.task.title} — this creates a Delivery, it does not run anything locally.`
          : undefined
      }
      busy={busy}
      error={error}
      footerNote={
        task ? `Expected Task version ${task.version}.` : undefined
      }
      submitLabel="Dispatch"
      submitBusyLabel="Dispatching…"
      submitDisabled={targetedIncomplete}
      onClose={onClose}
      onSubmit={() => {
        void submit();
      }}
    >
      {task ? (
        <div className="inline">
          <TaskStatusBadge status={task.status} showDescription />
        </div>
      ) : null}

      <fieldset className="form-section" style={{ border: 0, padding: 0, margin: 0 }}>
        <legend className="form-section-title">Selection</legend>
        <div className="inline" role="radiogroup" aria-label="Dispatch selection mode">
          <label className="checkbox-row">
            <input
              type="radio"
              name="dispatch-mode"
              value="match"
              checked={mode === "match"}
              disabled={busy}
              onChange={() => setMode("match")}
            />
            Simple Match
          </label>
          <label className="checkbox-row">
            <input
              type="radio"
              name="dispatch-mode"
              value="targeted"
              checked={mode === "targeted"}
              disabled={busy}
              onChange={() => setMode("targeted")}
            />
            Targeted
          </label>
        </div>
        <span className="field-hint">
          {mode === "match"
            ? "Deterministic preference order: required capabilities, preferred Worker, role, executor, fewer active Deliveries, greater advisory capacity, then lexical ids."
            : "Names an exact Worker and Slot. Hub never falls back to another Worker or Slot."}
        </span>
      </fieldset>

      {mode === "match" ? (
        <label className="checkbox-row">
          <input
            type="checkbox"
            checked={fallbackAllowed}
            disabled={busy}
            onChange={(event) => setFallbackAllowed(event.target.checked)}
          />
          Allow fallback to another Worker when the preferred Worker is unavailable
        </label>
      ) : (
        <div className="form-grid">
          <div className="field">
            <label className="field-label" htmlFor="dispatch-worker">
              Worker
            </label>
            <select
              id="dispatch-worker"
              className="select"
              value={workerId}
              disabled={busy}
              onChange={(event) => {
                const nextWorkerId = event.target.value;
                setWorkerId(nextWorkerId);
                const nextWorker = selectableWorkers.find(
                  (worker) => worker.workerId === nextWorkerId,
                );
                const nextSlot =
                  nextWorker?.inventory?.slots.find((slot) => slot.enabled) ?? null;
                setSlotId(nextSlot?.slotId ?? "");
                setExecutorId("");
              }}
            >
              <option value="">Select a Worker…</option>
              {selectableWorkers.map((worker) => (
                <option key={worker.workerId} value={worker.workerId}>
                  {worker.workerId}
                  {worker.online ? "" : " (offline)"}
                </option>
              ))}
            </select>
          </div>

          <div className="field">
            <label className="field-label" htmlFor="dispatch-slot">
              Slot
            </label>
            <select
              id="dispatch-slot"
              className="select"
              value={slotId}
              disabled={busy || slots.length === 0}
              onChange={(event) => {
                setSlotId(event.target.value);
                setExecutorId("");
              }}
            >
              <option value="">
                {slots.length === 0 ? "No reported Slots" : "Select a Slot…"}
              </option>
              {slots.map((slot) => (
                <option key={slot.slotId} value={slot.slotId}>
                  {slot.slotId}
                  {slot.enabled ? "" : " (disabled)"} · capacity{" "}
                  {formatCapacity(slot.advisoryCapacity)}
                </option>
              ))}
            </select>
          </div>

          <div className="field field-wide">
            <label className="field-label" htmlFor="dispatch-executor">
              Executor
            </label>
            <select
              id="dispatch-executor"
              className="select"
              value={executorId}
              disabled={busy || executors.length === 0}
              onChange={(event) => setExecutorId(event.target.value)}
            >
              <option value="">
                {executors.length === 0 ? "No Executors on this Slot" : "Worker default"}
              </option>
              {executors.map((executor) => (
                <option key={executor.id} value={executor.id}>
                  {executor.id} · {executor.kind}
                </option>
              ))}
            </select>
          </div>
        </div>
      )}

      <label className="checkbox-row">
        <input
          type="checkbox"
          checked={queue}
          disabled={busy}
          onChange={(event) => setQueue(event.target.checked)}
        />
        Queue if unavailable
      </label>
      <span className="field-hint">
        A queued Task waits in FIFO order without reserving capacity.
      </span>
    </DialogShell>
  );
}

/* --------------------------------------------------------------------- */
/* Cancel                                                                 */
/* --------------------------------------------------------------------- */

export interface CancelDialogProps {
  readonly task: TaskRecord | null;
  readonly onClose: () => void;
  readonly onSubmit: (taskId: string, request: CancelTaskRequest) => Promise<void>;
}

export function CancelDialog({ task, onClose, onSubmit }: CancelDialogProps) {
  const [reason, setReason] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const open = task !== null;

  useEffect(() => {
    if (open) {
      setReason("");
      setError(null);
      setBusy(false);
    }
  }, [open, task?.id]);

  const submit = useCallback(async (): Promise<void> => {
    if (!task) {
      return;
    }
    const trimmed = reason.trim();
    const request: CancelTaskRequest = {
      expectedTaskVersion: task.version,
      reason: trimmed === "" ? null : trimmed,
    };
    setBusy(true);
    setError(null);
    try {
      await onSubmit(task.id, request);
      onClose();
    } catch (submitError) {
      setError(describeError(submitError));
    } finally {
      setBusy(false);
    }
  }, [onClose, onSubmit, reason, task]);

  return (
    <DialogShell
      open={open}
      title="Cancel Task"
      description={task ? `Cancels ${task.task.title}.` : undefined}
      busy={busy}
      error={error}
      danger
      submitLabel="Cancel Task"
      submitBusyLabel="Cancelling…"
      onClose={onClose}
      onSubmit={() => {
        void submit();
      }}
    >
      <div className="alert alert-error">
        <span>
          Cancellation is terminal: a cancelled Task never changes again and a later Worker finish
          report does not overwrite it. The Worker reports its own cancellation result honestly —
          a Hub cancellation never proves the local process actually stopped.
        </span>
      </div>

      <div className="field">
        <label className="field-label" htmlFor="cancel-reason">
          Reason (optional)
        </label>
        <input
          id="cancel-reason"
          className="input"
          data-autofocus
          placeholder="e.g. superseded by a newer Task"
          value={reason}
          disabled={busy}
          onChange={(event) => setReason(event.target.value)}
        />
      </div>
    </DialogShell>
  );
}

/* --------------------------------------------------------------------- */
/* Finish (coordination-only held Parent)                                 */
/* --------------------------------------------------------------------- */

export interface FinishDialogProps {
  readonly task: TaskRecord | null;
  readonly onClose: () => void;
  readonly onSubmit: (taskId: string, request: FinishTaskRequest) => Promise<void>;
}

export function FinishDialog({ task, onClose, onSubmit }: FinishDialogProps) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const open = task !== null;

  useEffect(() => {
    if (open) {
      setError(null);
      setBusy(false);
    }
  }, [open, task?.id]);

  const submit = useCallback(async (): Promise<void> => {
    if (!task) {
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await onSubmit(task.id, { expectedTaskVersion: task.version });
      onClose();
    } catch (submitError) {
      setError(describeError(submitError));
    } finally {
      setBusy(false);
    }
  }, [onClose, onSubmit, task]);

  return (
    <DialogShell
      open={open}
      title="Finish Parent Task"
      description={
        task
          ? `Marks the coordination-only Parent ${task.task.title} as finished.`
          : undefined
      }
      busy={busy}
      error={error}
      submitLabel="Finish Task"
      submitBusyLabel="Finishing…"
      onClose={onClose}
      onSubmit={() => {
        void submit();
      }}
    >
      <div className="alert">
        <span>
          This Parent has no Delivery, so nothing executed. Finishing records that coordination
          ended — it does not report a Worker result and never fabricates one.
        </span>
      </div>
      <div className="detail-grid">
        <span className="detail-label">Subtree</span>
        <span className="detail-value tabular">
          {task
            ? `${task.subtree.finished} of ${task.subtree.total} Tasks finished`
            : "—"}
        </span>
        <span className="detail-label">Eligible</span>
        <span className="detail-value">
          {task && isCoordinationFinishable(task)
            ? "Yes — coordination-only held Parent"
            : "Hub will re-check on submit"}
        </span>
      </div>
    </DialogShell>
  );
}
