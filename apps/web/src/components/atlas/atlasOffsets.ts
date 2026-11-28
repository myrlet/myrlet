/**
 * Persisted Board offsets.
 *
 * Manual card positions are presentation convenience state, stored as one JSON
 * object of `{ taskId: { x, y } }` under `myrlet.atlas.positions`. Private-mode
 * or quota failures are non-fatal: the Board always renders and drags from the
 * deterministic layout.
 */

import type { AtlasPosition } from "../workAtlasLayout";

export type AtlasOffsets = Record<string, AtlasPosition>;

export const DEFAULT_ATLAS_STORAGE_KEY = "myrlet.atlas.positions";

const isFiniteNumber = (value: unknown): value is number =>
  typeof value === "number" && Number.isFinite(value);

export function readStoredOffsets(storageKey: string): AtlasOffsets {
  try {
    const raw = window.localStorage.getItem(storageKey);
    if (raw === null) {
      return {};
    }
    const parsed: unknown = JSON.parse(raw);
    if (parsed === null || typeof parsed !== "object" || Array.isArray(parsed)) {
      return {};
    }
    const offsets: AtlasOffsets = {};
    for (const [taskId, value] of Object.entries(parsed as Record<string, unknown>)) {
      if (value === null || typeof value !== "object") continue;
      const { x, y } = value as { x?: unknown; y?: unknown };
      if (isFiniteNumber(x) && isFiniteNumber(y)) {
        offsets[taskId] = { x, y };
      }
    }
    return offsets;
  } catch {
    return {};
  }
}

export function writeStoredOffsets(storageKey: string, offsets: AtlasOffsets): void {
  try {
    const entries = Object.entries(offsets).filter(([, offset]) => offset.x !== 0 || offset.y !== 0);
    if (entries.length === 0) {
      window.localStorage.removeItem(storageKey);
      return;
    }
    window.localStorage.setItem(storageKey, JSON.stringify(Object.fromEntries(entries)));
  } catch {
    /* Storage is optional: never break dragging over a persistence failure. */
  }
}

export function clearStoredOffsets(storageKey: string): void {
  try {
    window.localStorage.removeItem(storageKey);
  } catch {
    /* Ignore unavailable storage. */
  }
}

/** Only cards a person moved are worth snapshotting for `Restore`. */
export function manualOffsets(offsets: AtlasOffsets): AtlasOffsets {
  const manual: AtlasOffsets = {};
  for (const [taskId, offset] of Object.entries(offsets)) {
    if (offset.x !== 0 || offset.y !== 0) {
      manual[taskId] = offset;
    }
  }
  return manual;
}
