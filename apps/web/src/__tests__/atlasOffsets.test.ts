/**
 * Persisted Board offsets: reading, writing, clearing, and the manual snapshot
 * used by Arrange / Restore. Storage failures must stay non-fatal.
 */

import { afterEach, describe, expect, it, vi } from "vitest";

import {
  DEFAULT_ATLAS_STORAGE_KEY,
  clearStoredOffsets,
  manualOffsets,
  readStoredOffsets,
  writeStoredOffsets,
} from "../components/atlas/atlasOffsets";

afterEach(() => {
  window.localStorage.clear();
  vi.restoreAllMocks();
});

describe("stored Board offsets", () => {
  it("defaults to the Board layout key and no offsets", () => {
    expect(DEFAULT_ATLAS_STORAGE_KEY).toBe("myrlet.atlas.positions");
    expect(readStoredOffsets(DEFAULT_ATLAS_STORAGE_KEY)).toEqual({});
  });

  it("round-trips finite offsets and drops everything else", () => {
    writeStoredOffsets(DEFAULT_ATLAS_STORAGE_KEY, {
      "task-1": { x: 24, y: -48 },
      "task-2": { x: 0, y: 0 },
    });
    const raw = JSON.parse(window.localStorage.getItem(DEFAULT_ATLAS_STORAGE_KEY) ?? "null");
    // A zero offset is the layout default and earns no stored entry.
    expect(raw).toEqual({ "task-1": { x: 24, y: -48 } });
    expect(readStoredOffsets(DEFAULT_ATLAS_STORAGE_KEY)).toEqual({ "task-1": { x: 24, y: -48 } });

    window.localStorage.setItem(
      DEFAULT_ATLAS_STORAGE_KEY,
      JSON.stringify({
        good: { x: 1, y: 2 },
        extra: { x: 3, y: 4, z: 5 },
        missingY: { x: 1 },
        notNumber: { x: "1", y: 2 },
        nanValue: { x: Number.NaN, y: 2 },
        notObject: 42,
        nullValue: null,
        arrayValue: [1, 2],
        nested: { x: {}, y: 2 },
      }),
    );
    expect(readStoredOffsets(DEFAULT_ATLAS_STORAGE_KEY)).toEqual({
      good: { x: 1, y: 2 },
      extra: { x: 3, y: 4 },
    });
  });

  it("treats malformed payloads as unavailable offsets", () => {
    for (const raw of ["{", "null", "[]", '"positions"', "42", ""]) {
      window.localStorage.setItem(DEFAULT_ATLAS_STORAGE_KEY, raw);
      expect(readStoredOffsets(DEFAULT_ATLAS_STORAGE_KEY)).toEqual({});
    }
  });

  it("removes the key when every offset is zero and on explicit clear", () => {
    window.localStorage.setItem(DEFAULT_ATLAS_STORAGE_KEY, JSON.stringify({ a: { x: 5, y: 5 } }));
    writeStoredOffsets(DEFAULT_ATLAS_STORAGE_KEY, { a: { x: 0, y: 0 } });
    expect(window.localStorage.getItem(DEFAULT_ATLAS_STORAGE_KEY)).toBeNull();

    window.localStorage.setItem(DEFAULT_ATLAS_STORAGE_KEY, JSON.stringify({ a: { x: 5, y: 5 } }));
    clearStoredOffsets(DEFAULT_ATLAS_STORAGE_KEY);
    expect(window.localStorage.getItem(DEFAULT_ATLAS_STORAGE_KEY)).toBeNull();
  });

  it("never throws when storage is unavailable", () => {
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("SecurityError");
    });
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("QuotaExceededError");
    });
    vi.spyOn(Storage.prototype, "removeItem").mockImplementation(() => {
      throw new Error("SecurityError");
    });

    expect(readStoredOffsets(DEFAULT_ATLAS_STORAGE_KEY)).toEqual({});
    expect(() => writeStoredOffsets(DEFAULT_ATLAS_STORAGE_KEY, { a: { x: 1, y: 2 } })).not.toThrow();
    expect(() => clearStoredOffsets(DEFAULT_ATLAS_STORAGE_KEY)).not.toThrow();
  });

  it("keeps only non-zero offsets in the Arrange snapshot", () => {
    expect(manualOffsets({ a: { x: 0, y: 0 }, b: { x: 12, y: 0 }, c: { x: 0, y: -12 } })).toEqual({
      b: { x: 12, y: 0 },
      c: { x: 0, y: -12 },
    });
    expect(manualOffsets({})).toEqual({});
  });
});
