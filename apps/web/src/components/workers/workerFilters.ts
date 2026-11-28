/**
 * Worker inventory predicates for the Workers & Pairing surface.
 *
 * A Worker's filter state is derived from live Hub facts only: `revoked` wins
 * over connection, and search matches the enrolled Worker ID plus its reported
 * inventory (host, Slots, roles, capabilities, executors). Nothing here infers
 * health, capacity, or a verdict.
 */

import type { WorkerRecord } from "../../api/types";

export type WorkerFilter = "all" | "online" | "offline" | "revoked";

export const WORKER_FILTERS: readonly {
  readonly value: WorkerFilter;
  readonly label: string;
}[] = [
  { value: "all", label: "All" },
  { value: "online", label: "Online" },
  { value: "offline", label: "Offline" },
  { value: "revoked", label: "Revoked" },
];

export function workerState(worker: WorkerRecord): Exclude<WorkerFilter, "all"> {
  if (worker.revoked) {
    return "revoked";
  }
  return worker.online ? "online" : "offline";
}

export function workerMatchesQuery(worker: WorkerRecord, query: string): boolean {
  if (query === "") {
    return true;
  }
  const parts: string[] = [worker.workerId];
  const host = worker.inventory?.host;
  if (host) {
    parts.push(host.os, host.arch, host.deploymentMode);
  }
  for (const slot of worker.inventory?.slots ?? []) {
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
  readonly activeDeliveries: number;
}

/** KPI facts: revoked Workers are registered but neither online nor offline. */
export function workerCounts(workers: readonly WorkerRecord[]): WorkerCounts {  let online = 0;
  let offline = 0;
  let activeDeliveries = 0;
  for (const worker of workers) {
    if (!worker.revoked) {
      if (worker.online) {
        online += 1;
      } else {
        offline += 1;
      }
    }
    activeDeliveries += worker.activeDeliveries;
  }
  return { registered: workers.length, online, offline, activeDeliveries };
}
