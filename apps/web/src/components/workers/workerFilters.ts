/**
 * Worker inventory predicates for the Workers & Pairing surface.
 *
 * A Worker's filter state is derived from live Hub facts only: `revoked` wins
 * over everything, an unapproved Worker is `pending`, and only an approved,
 * connected Worker is `online`. Search matches the enrolled Worker ID plus the
 * applied schedule (host, Slots, roles, capabilities, executors). Nothing here
 * infers health, capacity, or a verdict.
 */

import type { WorkerRecord } from "../../api/types";

export type WorkerFilter = "all" | "pending" | "approved" | "offline" | "revoked";

export const WORKER_FILTERS: readonly {
  readonly value: WorkerFilter;
  readonly label: string;
}[] = [
  { value: "all", label: "All" },
  { value: "pending", label: "Pending" },
  { value: "approved", label: "Approved" },
  { value: "offline", label: "Offline" },
  { value: "revoked", label: "Revoked" },
];

export function workerState(worker: WorkerRecord): Exclude<WorkerFilter, "all"> {
  if (worker.revoked) {
    return "revoked";
  }
  if (worker.approval === "pending") {
    return "pending";
  }
  return worker.online ? "approved" : "offline";
}

export function workerMatchesQuery(worker: WorkerRecord, query: string): boolean {
  if (query === "") {
    return true;
  }
  const parts: string[] = [worker.workerId, worker.approval];
  const host = worker.host;
  if (host) {
    parts.push(host.os, host.arch, host.deploymentMode);
  }
  for (const slot of worker.appliedConfig?.slots ?? []) {
    parts.push(slot.slotId, ...slot.roles);
    for (const capability of slot.capabilities) {
      parts.push(capability.name);
    }
    for (const executor of slot.executors) {
      parts.push(executor.id, executor.kind);
    }
  }
  return parts.join(" ").toLowerCase().includes(query);
}

export interface WorkerCounts {
  readonly registered: number;
  readonly online: number;
  readonly offline: number;
  readonly pending: number;
  readonly activeDeliveries: number;
}

/**
 * KPI facts: revoked Workers are registered but neither online, offline, nor
 * pending; only an approved, connected Worker counts as online.
 */
export function workerCounts(workers: readonly WorkerRecord[]): WorkerCounts {
  let online = 0;
  let offline = 0;
  let pending = 0;
  let activeDeliveries = 0;
  for (const worker of workers) {
    if (!worker.revoked) {
      if (worker.approval === "pending") {
        pending += 1;
      } else if (worker.online) {
        online += 1;
      } else {
        offline += 1;
      }
    }
    activeDeliveries += worker.activeDeliveries;
  }
  return { registered: workers.length, online, offline, pending, activeDeliveries };
}

/** Whether the latest report still waits for an Admin Apply. */
export function applyPending(worker: WorkerRecord): boolean {
  return (
    worker.reportedRevision !== null && worker.reportedRevision !== worker.appliedRevision
  );
}
