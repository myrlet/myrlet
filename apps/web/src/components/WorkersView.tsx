/**
 * Workers & Pairing — the inventory surface shared by the slide-over drawer.
 *
 * Three bands, top to bottom: a one-step pairing card that issues a single-use
 * enrollment code and a copy-ready `worker pair` command, a KPI strip, and the
 * enrolled Worker cards with their Slots.
 *
 * Trust is explicit. A pending Worker can connect but never claims until an
 * Admin approves it; a changed scheduling report stays under review until an
 * Admin applies the exact revision they saw. Inventory is shown verbatim:
 * advisory capacity of `null` is unknown and renders as `—`, never `0`.
 */

import { useMemo, useState } from "react";

import type {
  CreateEnrollmentCodeRequest,
  CreateEnrollmentCodeResponse,
  RotateWorkerResponse,
  WorkerRecord,
  WorkerSchedulingConfig,
} from "../api/types";
import { PairingCard } from "./workers/PairingCard";
import { WorkerCard } from "./workers/WorkerCard";
import { KpiStrip } from "./workers/WorkerInventory";
import {
  WORKER_FILTERS,
  workerCounts,
  workerMatchesQuery,
  workerState,
} from "./workers/workerFilters";
import type { WorkerFilter } from "./workers/workerFilters";

export interface WorkersViewProps {
  readonly baseUrl: string;
  readonly workers: readonly WorkerRecord[];
  /** Read one Worker's report under review, bound to the revision it was reported at. */
  readonly fetchReportedConfig: (workerId: string) => Promise<WorkerSchedulingConfig | null>;
  readonly issued: CreateEnrollmentCodeResponse | null;
  readonly busy: boolean;
  readonly onCreateEnrollmentCode: (
    request: CreateEnrollmentCodeRequest,
  ) => Promise<void>;
  readonly onApproveWorker: (workerId: string) => Promise<void>;
  readonly onApplyWorkerConfig: (workerId: string, revision: number) => Promise<void>;
  readonly onRotateWorker: (workerId: string) => Promise<RotateWorkerResponse>;
  readonly onRevokeWorker: (workerId: string) => Promise<void>;
  readonly onOpenPairing: () => void;
  readonly onCopyText: (label: string, text: string) => void;
}

export function WorkersView(props: WorkersViewProps) {
  const {
    baseUrl,
    workers,
    fetchReportedConfig,
    onCreateEnrollmentCode,
    onApproveWorker,
    onApplyWorkerConfig,
    onRotateWorker,
    onRevokeWorker,
    onOpenPairing,
    onCopyText,
  } = props;

  const [filter, setFilter] = useState<WorkerFilter>("all");
  const [query, setQuery] = useState("");

  const trimmedQuery = query.trim().toLowerCase();

  const visible = useMemo(
    () =>
      workers.filter(
        (worker) =>
          (filter === "all" || workerState(worker) === filter) &&
          workerMatchesQuery(worker, trimmedQuery),
      ),
    [workers, filter, trimmedQuery],
  );

  const counts = useMemo(() => workerCounts(workers), [workers]);

  return (
    <section className="workers-view" aria-label="Workers and pairing">
      <div className="workers-scroll">
        <PairingCard
          issued={props.issued}
          busy={props.busy}
          baseUrl={baseUrl}
          onCreateEnrollmentCode={onCreateEnrollmentCode}
          onCopyText={onCopyText}
        />

        <KpiStrip {...counts} />

        <div className="workers-toolbar">
          <div className="filter-pills" role="group" aria-label="Filter Workers by status">
            {WORKER_FILTERS.map((option) => (
              <button
                key={option.value}
                type="button"
                className="filter-pill"
                data-active={filter === option.value}
                aria-pressed={filter === option.value}
                onClick={() => setFilter(option.value)}
              >
                {option.label}
              </button>
            ))}
          </div>
          <span className="view-header-spacer" />
          <input
            className="input view-search"
            type="search"
            data-view-search
            aria-label="Search workers"
            placeholder="Search workers, Slots, capabilities…"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
          />
        </div>

        {workers.length === 0 ? (
          <div className="pane-empty">
            <span className="pane-empty-title">No Workers enrolled</span>
            <span>Pair a host with the code above, then start its Worker daemon.</span>
          </div>
        ) : visible.length === 0 ? (
          <div className="pane-empty">
            <span className="pane-empty-title">No matching Workers</span>
            <span>No Worker matches the current status filter or search.</span>
            <button
              type="button"
              className="button button-sm"
              onClick={() => {
                setFilter("all");
                setQuery("");
              }}
            >
              Clear filters
            </button>
          </div>
        ) : (
          <div className="worker-card-list">
            {visible.map((worker) => (
              <WorkerCard
                key={worker.workerId}
                worker={worker}
                fetchReportedConfig={fetchReportedConfig}
                onApproveWorker={onApproveWorker}
                onApplyWorkerConfig={onApplyWorkerConfig}
                onRotateWorker={onRotateWorker}
                onRevokeWorker={onRevokeWorker}
                onOpenPairing={onOpenPairing}
                onCopyText={onCopyText}
              />
            ))}
          </div>
        )}
      </div>
    </section>
  );
}
