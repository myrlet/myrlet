/**
 * Honest, verbatim error wording for Worker commands.
 *
 * A Hub command failure keeps the Hub's own message; the issue form additionally
 * prefixes the HTTP status so an operator can tell a rejected code from an
 * unreachable Hub. Nothing is retried or rewritten here.
 */

import { HubApiError } from "../../api/client";

export function describeError(error: unknown): string {
  if (error instanceof HubApiError) {
    return error.message;
  }
  if (error instanceof Error) {
    return error.message;
  }
  return "The Hub command could not be completed.";
}

/** Pairing-code issuance wording: `HTTP 409: …` for a rejected request. */
export function describeIssueError(error: unknown): string {
  return error instanceof HubApiError
    ? `HTTP ${error.status}: ${error.message}`
    : describeError(error);
}
