/**
 * Live Worker presence.
 *
 * A Worker is online only while the Hub still reports it connected, its
 * credentials have not been revoked, and an Admin has approved it for
 * scheduling; a missing record is offline. A pending Worker is connected but
 * deliberately not shown as online, because the Hub sends it no work.
 */

import type { WorkerRecord } from "../api/types";

export function isWorkerOnline(worker: WorkerRecord | undefined): boolean {
  return (
    worker !== undefined &&
    worker.online &&
    !worker.revoked &&
    worker.approval === "approved"
  );
}
