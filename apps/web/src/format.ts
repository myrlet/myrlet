/**
 * Single source of truth for human-facing facts.
 *
 * Every view renders Task status, Delivery milestones, capacity, and time
 * through these helpers so wording and provenance stay truthful and identical
 * everywhere. Notably `finished` reads "Handling ended" — never "Completed",
 * "Succeeded", or a green checkmark.
 */

import type {
  CancelState,
  CapabilityState,
  DeliveryStatus,
  SnapshotTaskSummary,
  TaskRecord,
  TaskStatus,
} from "./api/types";

/* --------------------------------------------------------------------- */
/* Task status                                                            */
/* --------------------------------------------------------------------- */

/** Shape paired with every status colour so meaning survives without colour. */
export type StatusShape = "circle" | "queue" | "arrow" | "square" | "cross";

export interface TaskStatusMeta {
  /** Human label. `finished` is always "Handling ended". */
  readonly label: string;
  /** CSS tone suffix (`data-tone`), also the status token family name. */
  readonly tone: TaskStatus;
  /** Shape marker rendered next to the label. */
  readonly shape: StatusShape;
  /** Plain-text glyph fallback for the shape. */
  readonly glyph: string;
  /** Honest one-line meaning of the state. */
  readonly description: string;
  /** `finished` and `cancelled` never change again. */
  readonly terminal: boolean;
}

export const TASK_STATUS_META: Readonly<Record<TaskStatus, TaskStatusMeta>> = {
  held: {
    label: "Held",
    tone: "held",
    shape: "circle",
    glyph: "○",
    description: "Submitted, but a prerequisite has not finished yet.",
    terminal: false,
  },
  queued: {
    label: "Queued",
    tone: "queued",
    shape: "queue",
    glyph: "≡",
    description: "Authorized and waiting for an eligible Worker to claim it.",
    terminal: false,
  },
  dispatched: {
    label: "Dispatched",
    tone: "dispatched",
    shape: "arrow",
    glyph: "▸",
    description: "A Worker claimed this Task; handling may or may not have started.",
    terminal: false,
  },
  finished: {
    label: "Handling ended",
    tone: "finished",
    shape: "square",
    glyph: "■",
    description: "The assigned Worker reported that its handling ended. Not a success verdict.",
    terminal: true,
  },
  cancelled: {
    label: "Cancelled",
    tone: "cancelled",
    shape: "cross",
    glyph: "✕",
    description: "Explicitly cancelled. Terminal.",
    terminal: true,
  },
};

/** Canonical status order for filters and summaries. */
export const TASK_STATUSES: readonly TaskStatus[] = [
  "held",
  "queued",
  "dispatched",
  "finished",
  "cancelled",
];

/* --------------------------------------------------------------------- */
/* Delivery, cancel, rejection, capability labels                         */
/* --------------------------------------------------------------------- */

export const DELIVERY_STATUS_LABELS: Readonly<Record<DeliveryStatus, string>> = {
  accepted: "Claimed",
  finished: "Handling ended",
  cancelled: "Cancelled",
};

export const CANCEL_STATE_LABELS: Readonly<Record<CancelState, string>> = {
  confirmed: "Cancel confirmed",
  failed: "Cancel failed",
  uncertain: "Cancel uncertain",
  unsupported: "Cancel unsupported",
};

export const CAPABILITY_STATE_LABELS: Readonly<Record<CapabilityState, string>> = {
  available: "Available",
  degraded: "Degraded",
  unavailable: "Unavailable",
};

/* --------------------------------------------------------------------- */
/* Value formatting                                                       */
/* --------------------------------------------------------------------- */

/**
 * Advisory Slot capacity. Missing capacity is unknown (`null`), never `0`.
 */
export function formatCapacity(cap: number | null | undefined): string {
  return cap == null ? "—" : String(cap);
}

function pad2(value: number): string {
  return String(value).padStart(2, "0");
}

/** Compact relative age, e.g. `just now`, `42s ago`, `7m ago`, `in 2h`. */
export function formatRelativeTime(
  ms: number | null | undefined,
  nowMs: number = Date.now(),
): string {
  if (ms == null || !Number.isFinite(ms)) {
    return "—";
  }
  const delta = nowMs - ms;
  const magnitude = Math.abs(delta);
  if (magnitude < 5_000) {
    return "just now";
  }
  const wrap = (value: string): string => (delta < 0 ? `in ${value}` : `${value} ago`);
  if (magnitude < 60_000) {
    return wrap(`${Math.floor(magnitude / 1_000)}s`);
  }
  if (magnitude < 3_600_000) {
    return wrap(`${Math.floor(magnitude / 60_000)}m`);
  }
  if (magnitude < 86_400_000) {
    return wrap(`${Math.floor(magnitude / 3_600_000)}h`);
  }
  return wrap(`${Math.floor(magnitude / 86_400_000)}d`);
}

/** Local `YYYY-MM-DD HH:mm:ss` timestamp; `—` when unknown. */
export function formatTimestamp(ms: number | null | undefined): string {
  if (ms == null || !Number.isFinite(ms)) {
    return "—";
  }
  const date = new Date(ms);
  if (Number.isNaN(date.getTime())) {
    return "—";
  }
  return (
    `${date.getFullYear()}-${pad2(date.getMonth() + 1)}-${pad2(date.getDate())}` +
    ` ${pad2(date.getHours())}:${pad2(date.getMinutes())}:${pad2(date.getSeconds())}`
  );
}

/** Grouped integer for counts (`1,248`). */
export function formatCount(value: number): string {
  if (!Number.isFinite(value)) {
    return "—";
  }
  return new Intl.NumberFormat("en-US").format(value);
}

/** Copy-ready Worker pairing command. */
export function buildPairCommand(hubUrl: string, code: string): string {
  return `worker pair --hub-url '${hubUrl.trim()}' --code '${code.trim()}'`;
}

/* --------------------------------------------------------------------- */
/* Task tree rows                                                         */
/* --------------------------------------------------------------------- */

/** Either a Snapshot summary or a full Task record can feed the tree. */
export type TaskTreeSource = SnapshotTaskSummary | TaskRecord;

export interface TaskFilter {
  /** Free-text match against id, title, category, and assigned worker id. */
  readonly query?: string;
  readonly status?: TaskStatus | "all" | null;
  readonly workerId?: string | "all" | null;
  readonly executorId?: string | "all" | null;
}

export interface TreeRow {
  /** The underlying wire record (Snapshot summary or full Task record). */
  readonly task: TaskTreeSource;
  readonly id: string;
  readonly parentTaskId: string | null;
  readonly title: string;
  readonly category: string;
  readonly status: TaskStatus;
  /** 0-based visual depth inside the rendered tree (Root = 0). */
  readonly depth: number;
  /** Whether the loaded data contains at least one child of this Task. */
  readonly hasChildren: boolean;
  /** Number of loaded child Tasks. */
  readonly childCount: number;
  /** Whether this row's children are currently rendered. */
  readonly expanded: boolean;
  /** Whether this row itself satisfies the active filter (false when inactive). */
  readonly matchesFilter: boolean;
  /** Whether this row is the last visible child of its direct parent. */
  readonly isLastSibling: boolean;
  /** For each ancestor depth (0..depth-1), whether a vertical stem continues past this row. */
  readonly ancestorContinuations: readonly boolean[];
  /** Number of declared sibling dependencies. */
  readonly dependencyCount: number;
  /** Whether this Task carries a sourceTaskId (copy/retry provenance). */
  readonly hasSourceTask: boolean;
}

function isTaskRecord(task: TaskTreeSource): task is TaskRecord {
  return "task" in task && typeof (task as TaskRecord).task === "object";
}

/** Task parent id regardless of whether the source is a summary or record. */
export function taskParentId(task: TaskTreeSource): string | null {
  return isTaskRecord(task) ? task.task.parentTaskId : task.parentTaskId;
}

/** Task title regardless of whether the source is a summary or record. */
export function taskTitle(task: TaskTreeSource): string {
  return isTaskRecord(task) ? task.task.title : task.title;
}

/** Task category regardless of whether the source is a summary or record. */
export function taskCategory(task: TaskTreeSource): string {
  return isTaskRecord(task) ? task.task.category : task.category;
}

function compareText(a: string, b: string): number {
  return a < b ? -1 : a > b ? 1 : 0;
}

function compareTasks(a: TaskTreeSource, b: TaskTreeSource): number {
  if (a.createdAtMs !== b.createdAtMs) {
    return a.createdAtMs - b.createdAtMs;
  }
  return compareText(a.id, b.id);
}

function activeStatus(filter: TaskFilter): TaskStatus | null {
  const value = filter.status;
  return value && value !== "all" ? value : null;
}

function activeString(value: string | "all" | null | undefined): string | null {
  return value && value !== "all" ? value : null;
}

/**
 * Build deterministic collapsible tree rows from a flat Task list.
 *
 * - Preserves parent/child hierarchy up to the five-level Task Tree.
 * - Sibling order is `createdAtMs`, then lexical `id` (stable across renders).
 * - When a filter is active, a Task is shown if it matches *or* a descendant
 *   matches; ancestors of a match are kept visible and auto-expanded so the
 *   matched Task never loses its tree context.
 * - `expanded` controls which branches render when no filter is active.
 */
export function buildTaskTreeRows(
  tasks: readonly TaskTreeSource[],
  expanded: ReadonlySet<string>,
  filter: TaskFilter = {},
): TreeRow[] {
  const byId = new Map<string, TaskTreeSource>();
  for (const task of tasks) {
    byId.set(task.id, task);
  }

  const roots: TaskTreeSource[] = [];
  const childrenByParent = new Map<string, TaskTreeSource[]>();

  for (const task of tasks) {
    const parentId = taskParentId(task);
    if (parentId === null || parentId === task.id || !byId.has(parentId)) {
      roots.push(task);
      continue;
    }
    const siblings = childrenByParent.get(parentId);
    if (siblings) {
      siblings.push(task);
    } else {
      childrenByParent.set(parentId, [task]);
    }
  }

  roots.sort(compareTasks);
  for (const siblings of childrenByParent.values()) {
    siblings.sort(compareTasks);
  }

  const query = (filter.query ?? "").trim().toLowerCase();
  const statusFilter = activeStatus(filter);
  const workerFilter = activeString(filter.workerId);
  const executorFilter = activeString(filter.executorId);
  const filterActive =
    query !== "" ||
    statusFilter !== null ||
    workerFilter !== null ||
    executorFilter !== null;

  const matches = new Set<string>();
  if (filterActive) {
    for (const task of tasks) {
      if (statusFilter !== null && task.status !== statusFilter) {
        continue;
      }
      if (workerFilter !== null && task.currentDelivery?.workerId !== workerFilter) {
        continue;
      }
      if (executorFilter !== null && task.currentDelivery?.executorId !== executorFilter) {
        continue;
      }
      if (query !== "") {
        const haystack = [
          task.id,
          taskTitle(task),
          taskCategory(task),
          task.currentDelivery?.workerId ?? "",
        ];
        if (!haystack.some((value) => value.toLowerCase().includes(query))) {
          continue;
        }
      }
      matches.add(task.id);
    }
  }

  // Matched Tasks plus their ancestor chain, so tree context is never broken.
  const included = new Set<string>();
  const autoExpanded = new Set<string>();
  if (filterActive) {
    for (const id of matches) {
      let current: TaskTreeSource | undefined = byId.get(id);
      while (current && !included.has(current.id)) {
        included.add(current.id);
        const parentId = taskParentId(current);
        if (parentId === null) {
          break;
        }
        const parent = byId.get(parentId);
        if (!parent || parent.id === current.id) {
          break;
        }
        autoExpanded.add(parent.id);
        current = parent;
      }
    }
  }

  const rows: TreeRow[] = [];
  const visited = new Set<string>();

  const visit = (
    task: TaskTreeSource,
    depth: number,
    isLastSibling: boolean,
    ancestorContinuations: readonly boolean[],
  ): void => {
    if (visited.has(task.id)) {
      return;
    }
    visited.add(task.id);

    if (filterActive && !included.has(task.id)) {
      return;
    }

    const allChildren = childrenByParent.get(task.id) ?? [];
    const visibleChildren = filterActive
      ? allChildren.filter((child) => included.has(child.id))
      : allChildren;
    const isOpen = allChildren.length > 0 && (autoExpanded.has(task.id) || expanded.has(task.id));
    const dependencyCount = isTaskRecord(task)
      ? task.task.dependencies.length
      : task.dependencies.length;
    const hasSourceTask =
      (isTaskRecord(task) ? task.task.sourceTaskId : task.sourceTaskId) !== null;

    rows.push({
      task,
      id: task.id,
      parentTaskId: taskParentId(task),
      title: taskTitle(task),
      category: taskCategory(task),
      status: task.status,
      depth,
      hasChildren: allChildren.length > 0,
      childCount: allChildren.length,
      expanded: isOpen,
      matchesFilter: filterActive && matches.has(task.id),
      isLastSibling,
      ancestorContinuations,
      dependencyCount,
      hasSourceTask,
    });

    if (isOpen) {
      const childContinuations = depth === 0 ? [] : [...ancestorContinuations, !isLastSibling];
      visibleChildren.forEach((child, index) => {
        visit(child, depth + 1, index === visibleChildren.length - 1, childContinuations);
      });
    }
  };

  const visibleRoots = filterActive ? roots.filter((root) => included.has(root.id)) : roots;
  visibleRoots.forEach((root, index) => {
    visit(root, 0, index === visibleRoots.length - 1, []);
  });

  return rows;
}
