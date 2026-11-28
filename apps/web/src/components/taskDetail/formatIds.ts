/** Identifier display helpers shared by the Task Detail views. */

/**
 * Identifier shortened for dense rows. It stays a prefix so two long IDs you
 * see together remain distinguishable; use `middleTruncateId` in single-row
 * headers where both ends carry meaning.
 */
export function shortId(id: string): string {
  return id.length <= 12 ? id : id.slice(0, 12);
}

/** Keeps both ends of a Task ID readable inside a single non-wrapping row. */
export function middleTruncateId(id: string): string {
  return id.length > 18 ? `${id.slice(0, 10)}…${id.slice(-4)}` : id;
}
