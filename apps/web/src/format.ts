/**
 * Single source of truth for human-facing facts.
 *
 * Every view renders Task status, Delivery milestones, capacity, and time
 * through these helpers so wording and provenance stay truthful and identical
 * everywhere. Notably `finished` reads "Handling ended" — never "Completed",
 * "Succeeded", or a green checkmark. Pure Task Tree projection lives in
 * `model/taskTree.ts`.
 */

import type {
  CancelState,
  CapabilityState,
  DeliveryStatus,
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
