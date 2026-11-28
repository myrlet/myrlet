/**
 * Board card offsets: persisted manual positions plus Arrange / Restore.
 *
 * `Arrange` returns every card to the deterministic layout and stashes the
 * manual offsets so `Restore` can put them back. Storage failures never block
 * either action.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import type { MutableRefObject } from "react";

import {
  clearStoredOffsets,
  manualOffsets,
  readStoredOffsets,
  writeStoredOffsets,
} from "./atlasOffsets";
import type { AtlasOffsets } from "./atlasOffsets";

interface UseAtlasOffsetsResult {
  readonly offsets: AtlasOffsets;
  readonly offsetsRef: MutableRefObject<AtlasOffsets>;
  readonly setOffsets: (next: AtlasOffsets) => void;
  readonly restoreSnapshot: AtlasOffsets | null;
  readonly saveOffsets: (next: AtlasOffsets) => void;
  readonly arrange: () => void;
  readonly restore: () => void;
}

export function useAtlasOffsets(storageKey: string): UseAtlasOffsetsResult {
  const [offsets, setOffsetsState] = useState<AtlasOffsets>(() => readStoredOffsets(storageKey));
  const [restoreSnapshot, setRestoreSnapshot] = useState<AtlasOffsets | null>(null);
  const offsetsRef = useRef<AtlasOffsets>(offsets);
  const storageKeyRef = useRef(storageKey);

  const setOffsets = useCallback((next: AtlasOffsets) => {
    offsetsRef.current = next;
    setOffsetsState(next);
  }, []);

  // A different storage key starts with its own offsets.
  useEffect(() => {
    if (storageKeyRef.current === storageKey) return;
    storageKeyRef.current = storageKey;
    const stored = readStoredOffsets(storageKey);
    offsetsRef.current = stored;
    setOffsetsState(stored);
    setRestoreSnapshot(null);
  }, [storageKey]);

  const saveOffsets = useCallback((next: AtlasOffsets) => {
    setOffsets(next);
    writeStoredOffsets(storageKeyRef.current, next);
  }, [setOffsets]);

  const arrange = useCallback(() => {
    const manual = manualOffsets(offsetsRef.current);
    setRestoreSnapshot(Object.keys(manual).length > 0 ? manual : null);
    setOffsets({});
    clearStoredOffsets(storageKeyRef.current);
  }, [setOffsets]);

  const restore = useCallback(() => {
    if (restoreSnapshot === null) return;
    saveOffsets(restoreSnapshot);
    setRestoreSnapshot(null);
  }, [restoreSnapshot, saveOffsets]);

  return { offsets, offsetsRef, setOffsets, saveOffsets, restoreSnapshot, arrange, restore };
}
