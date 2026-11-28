/**
 * Live Worker presence.
 *
 * A Worker is online only while the Hub still reports it connected and its
 * credentials have not been revoked; a missing record is offline.
 */

import type { WorkerRecord } from "../api/types";

export function isWorkerOnline(worker: WorkerRecord | undefined): boolean {
  return worker !== undefined && worker.online && !worker.revoked;
}
