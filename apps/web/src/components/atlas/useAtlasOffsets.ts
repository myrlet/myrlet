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
  readonly storageKeyRef: MutableRefObject<string>;
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

  // A different board (Wave 9B passes its own key) starts from that board's offsets.
  useEffect(() => {
    if (storageKeyRef.current === storageKey) return;
    storageKeyRef.current = storageKey;
    const stored = readStoredOffsets(storageKey);
    offsetsRef.current = stored;
    setOffsetsState(stored);
    setRestoreSnapshot(null);
  }, [storageKey]);

  const arrange = useCallback(() => {
    const manual = manualOffsets(offsetsRef.current);
    setRestoreSnapshot(Object.keys(manual).length > 0 ? manual : null);
    setOffsets({});
    clearStoredOffsets(storageKeyRef.current);
  }, [setOffsets]);

  const restore = useCallback(() => {
    if (restoreSnapshot === null) return;
    setOffsets(restoreSnapshot);
    writeStoredOffsets(storageKeyRef.current, restoreSnapshot);
    setRestoreSnapshot(null);
  }, [restoreSnapshot, setOffsets]);

  return { offsets, offsetsRef, setOffsets, restoreSnapshot, storageKeyRef, arrange, restore };
}
