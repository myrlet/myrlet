/** Compact label for a Task or Worker ID; copy actions use the full ID. */
export function shortId(id: string): string {
  return id.slice(0, 12);
}
