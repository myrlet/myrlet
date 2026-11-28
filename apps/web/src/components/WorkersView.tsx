/**
 * Workers & Pairing — a dedicated full-page view.
 *
 * Three bands, top to bottom: a one-step pairing card that issues a single-use
 * enrollment code and a copy-ready `worker pair` command, a four-cell hardware
 * KPI strip, and the enrolled Worker cards with their applied Slots.
 *
 * Trust is explicit. A pending Worker can connect but never claims until an
 * Admin approves it; a changed scheduling report stays under review until an
 * Admin applies the exact revision they saw. Inventory is shown verbatim:
 * advisory capacity of `null` is unknown and renders as `—`, never `0`.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { HubApiError } from "../api/client";
import type {
  CreateEnrollmentCodeRequest,
  CreateEnrollmentCodeResponse,
  RotateWorkerResponse,
  SlotInventory,
  WorkerRecord,
  WorkerSchedulingConfig,
} from "../api/types";
import {
  CAPABILITY_STATE_LABELS,
  buildPairCommand,
  formatCapacity,
  formatRelativeTime,
  formatTimestamp,
} from "../format";
import { WorkerStatusBadge } from "./StatusBadge";

const DEFAULT_TTL_SECONDS = 600;

function describeError(error: unknown): string {
  if (error instanceof HubApiError) {
    return error.message;
  }
  if (error instanceof Error) {
    return error.message;
  }
  return "The Hub command could not be completed.";
}

/* --------------------------------------------------------------------- */
/* Filters                                                                */
/* --------------------------------------------------------------------- */

type WorkerFilter = "all" | "pending" | "approved" | "offline" | "revoked";

const WORKER_FILTERS: readonly { readonly value: WorkerFilter; readonly label: string }[] = [
  { value: "all", label: "All" },
  { value: "pending", label: "Pending" },
  { value: "approved", label: "Approved" },
  { value: "offline", label: "Offline" },
  { value: "revoked", label: "Revoked" },
];

function workerState(worker: WorkerRecord): Exclude<WorkerFilter, "all"> {
  if (worker.revoked) {
    return "revoked";
  }
  if (worker.approval === "pending") {
    return "pending";
  }
  return worker.online ? "approved" : "offline";
}

function workerMatchesQuery(worker: WorkerRecord, query: string): boolean {
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

/** Whether the latest report still waits for an Admin Apply. */
function applyPending(worker: WorkerRecord): boolean {
  return (
    worker.reportedRevision !== null && worker.reportedRevision !== worker.appliedRevision
  );
}

/* --------------------------------------------------------------------- */
/* Pairing card                                                           */
/* --------------------------------------------------------------------- */

interface PairingCardProps {
  readonly baseUrl: string;
  readonly issued: CreateEnrollmentCodeResponse | null;
  readonly busy: boolean;
  readonly onCreateEnrollmentCode: (
    request: CreateEnrollmentCodeRequest,
  ) => Promise<void>;
  readonly onCopyText: (label: string, text: string) => void;
}

function PairingCard({ baseUrl, issued, busy, onCreateEnrollmentCode, onCopyText }: PairingCardProps) {
  const [ttlSeconds, setTtlSeconds] = useState(String(DEFAULT_TTL_SECONDS));
  const [suggestedWorkerId, setSuggestedWorkerId] = useState("");
  const locked = busy || issued !== null;
  const [error, setError] = useState<string | null>(null);

  const generate = useCallback(async (): Promise<void> => {
    const parsedTtl = Number.parseInt(ttlSeconds, 10);
    if (!Number.isFinite(parsedTtl) || parsedTtl <= 0) {
      setError("The code lifetime must be a positive number of seconds.");
      return;
    }
    const suggested = suggestedWorkerId.trim();
    if (locked) return;
    setError(null);
    try {
      await onCreateEnrollmentCode({
        ttlSeconds: parsedTtl,
        suggestedWorkerId: suggested === "" ? null : suggested,
      });
    } catch (issueError) {
      setError(issueError instanceof HubApiError ? `HTTP ${issueError.status}: ${issueError.message}` : describeError(issueError));
    }
  }, [locked, onCreateEnrollmentCode, suggestedWorkerId, ttlSeconds]);

  const pairCommand = issued ? buildPairCommand(baseUrl, issued.enrollmentCode) : "";

  return (
    <section className="pairing-card" aria-label="One-step pairing">
      <div className="pairing-heading">
        <h2 className="pairing-title">Pair a Worker</h2>
        <p className="pairing-description">
          Issue a single-use code, then run the command once on the new host. The code is shown
          once and expires automatically. A new Worker stays pending until an Admin approves it.
        </p>
      </div>

      <div className="pairing-controls">
        <div className="field">
          <label className="field-label" htmlFor="pairing-ttl">
            Code lifetime (seconds)
          </label>
          <input
            id="pairing-ttl"
            className="input input-mono"
            inputMode="numeric"
            value={ttlSeconds}
            disabled={locked}
            onChange={(event) => setTtlSeconds(event.target.value)}
          />
        </div>
        <div className="field">
          <label className="field-label" htmlFor="pairing-worker-id">
            Suggested Worker ID (optional)
          </label>
          <input
            id="pairing-worker-id"
            className="input input-mono"
            placeholder="wkr_…"
            value={suggestedWorkerId}
            disabled={locked}
            onChange={(event) => setSuggestedWorkerId(event.target.value)}
          />
        </div>
        <div className="pairing-action">
          <button
            type="button"
            className="button button-primary"
            aria-busy={busy ? "true" : undefined}
            disabled={locked}
            onClick={() => {
              void generate();
            }}
          >
            {busy ? "Generating…" : issued ? "Code issued" : "Generate Pairing Code"}
          </button>
        </div>
      </div>

      {error ? (
        <div className="alert alert-error" role="alert">
          <span>{error}</span>
        </div>
      ) : null}

      {issued ? (
        <div className="pairing-result">
          <div className="pairing-code-line">
            <span className="field-label">Pairing code</span>
            <span className="pair-code">{issued.enrollmentCode}</span>
            <span className="field-hint">
              Expires {formatTimestamp(issued.expiresAtMs)}
              {issued.suggestedWorkerId ? ` · suggested Worker ${issued.suggestedWorkerId}` : ""}
            </span>
          </div>
          <div className="terminal-bar">
            <code className="terminal-command">{pairCommand}</code>
            <button
              type="button"
              className="button button-sm"
              onClick={() => onCopyText("pair command", pairCommand)}
            >
              Copy
            </button>
          </div>
        </div>
      ) : null}
    </section>
  );
}

/* --------------------------------------------------------------------- */
/* KPI strip                                                              */
/* --------------------------------------------------------------------- */

interface KpiStripProps {
  readonly registered: number;
  readonly online: number;
  readonly offline: number;
  readonly pending: number;
  readonly activeDeliveries: number;
}

function KpiStrip({ registered, online, offline, pending, activeDeliveries }: KpiStripProps) {
  const cells: readonly { readonly label: string; readonly value: number }[] = [
    { label: "Registered", value: registered },
    { label: "Online", value: online },
    { label: "Awaiting approval", value: pending },
    { label: "Offline", value: offline },
    { label: "Active Deliveries", value: activeDeliveries },
  ];
  return (
    <div className="kpi-strip" aria-label="Worker hardware summary">
      {cells.map((cell) => (
        <div className="kpi-cell" key={cell.label}>
          <span className="kpi-value tabular">{cell.value}</span>
          <span className="kpi-label">{cell.label}</span>
        </div>
      ))}
    </div>
  );
}

/* --------------------------------------------------------------------- */
/* Slots                                                                  */
/* --------------------------------------------------------------------- */

function SlotTable({ slots }: { readonly slots: readonly SlotInventory[] }) {
  if (slots.length === 0) {
    return <p className="detail-empty">No Slots reported yet.</p>;
  }
  return (
    <table className="slot-table">
      <caption className="visually-hidden">
        Applied Slots: enabled state, advisory capacity, roles, capabilities, and executors
      </caption>
      <thead>
        <tr>
          <th scope="col">Slot</th>
          <th scope="col">Advisory Capacity</th>
          <th scope="col">Roles</th>
          <th scope="col">Capabilities</th>
          <th scope="col">Executors</th>
        </tr>
      </thead>
      <tbody>
        {slots.map((slot) => (
          <tr key={slot.slotId} data-slot-id={slot.slotId}>
            <td>
              <span className={slot.enabled ? "slot-name" : "slot-name slot-disabled"}>
                {slot.slotId}
              </span>
              <span className="slot-state" data-enabled={slot.enabled}>
                {slot.enabled ? "enabled" : "disabled"}
              </span>
            </td>
            <td className="tabular">{formatCapacity(slot.advisoryCapacity)}</td>
            <td className="cell-muted">
              {slot.roles.length === 0 ? "—" : slot.roles.join(", ")}
            </td>
            <td>
              {slot.capabilities.length === 0 ? (
                <span className="cell-empty">—</span>
              ) : (
                <span className="capability-list">
                  {slot.capabilities.map((capability) => (
                    <span
                      className="capability-chip"
                      key={capability.name}
                      title={CAPABILITY_STATE_LABELS[capability.state]}
                    >
                      <span
                        className="state-dot"
                        data-state={capability.state}
                        aria-hidden="true"
                      />
                      {capability.name}
                    </span>
                  ))}
                </span>
              )}
            </td>
            <td className="mono cell-muted">
              {slot.executors.length === 0
                ? "—"
                : slot.executors.map((executor) => `${executor.id} · ${executor.kind}`).join(", ")}
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

/* --------------------------------------------------------------------- */
/* Snapshot review                                                        */
/* --------------------------------------------------------------------- */

interface ConfigReviewProps {
  readonly worker: WorkerRecord;
  readonly reported: WorkerSchedulingConfig | null;
}

/** Compare what the Worker reports with what the Hub schedules, field by field. */
function ConfigReview({ worker, reported }: ConfigReviewProps) {
  const reportedSlots = reported?.slots ?? [];
  const appliedSlots = worker.appliedConfig?.slots ?? [];
  const offered = reportedSlots.filter(
    (slot) => !appliedSlots.some((applied) => applied.slotId === slot.slotId),
  );
  const withdrawn = appliedSlots.filter(
    (slot) => !reportedSlots.some((reviewed) => reviewed.slotId === slot.slotId),
  );
  const changed = reportedSlots.filter((slot) => {
    const applied = appliedSlots.find((candidate) => candidate.slotId === slot.slotId);
    return applied !== undefined && JSON.stringify(applied) !== JSON.stringify(slot);
  });
  const differences = [
    ...offered.map((slot) => `new Slot ${slot.slotId}`),
    ...withdrawn.map((slot) => `removed Slot ${slot.slotId}`),
    ...changed.map((slot) => `changed Slot ${slot.slotId}`),
  ];

  if (worker.reportedRevision === null) {
    return <p className="detail-empty">No scheduling inventory reported yet.</p>;
  }

  return (
    <div className="config-review">
      <div className="config-review-grid">
        <section aria-label="Reported scheduling inventory">
          <h3 className="config-review-title">
            Reported <span className="tabular">rev {worker.reportedRevision}</span>
          </h3>
          <SlotTable slots={reportedSlots} />
        </section>
        <section aria-label="Applied scheduling inventory">
          <h3 className="config-review-title">
            Applied{" "}
            <span className="tabular">
              {worker.appliedRevision === null ? "none" : `rev ${worker.appliedRevision}`}
            </span>
          </h3>
          <SlotTable slots={appliedSlots} />
        </section>
      </div>
      <p className="config-diff" data-empty={differences.length === 0}>
        {differences.length === 0 ? "No field differences." : differences.join(" · ")}
      </p>
    </div>
  );
}

/* --------------------------------------------------------------------- */
/* Worker card                                                            */
/* --------------------------------------------------------------------- */

interface WorkerCardProps {
  readonly worker: WorkerRecord;
  readonly reported: WorkerSchedulingConfig | null;
  readonly onLoadReported: (workerId: string) => Promise<void>;
  readonly onApproveWorker: (workerId: string) => Promise<void>;
  readonly onApplyWorkerConfig: (workerId: string, revision: number) => Promise<void>;
  readonly onRotateWorker: (workerId: string) => Promise<RotateWorkerResponse>;
  readonly onRevokeWorker: (workerId: string) => Promise<void>;
  readonly onOpenPairing: () => void;
  readonly onCopyText: (label: string, text: string) => void;
}

type PendingAction = "rotate" | "revoke" | "apply" | null;

function WorkerCard({
  worker,
  reported,
  onLoadReported,
  onApproveWorker,
  onApplyWorkerConfig,
  onRotateWorker,
  onRevokeWorker,
  onOpenPairing,
  onCopyText,
}: WorkerCardProps) {
  const [pending, setPending] = useState<PendingAction>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [rotatedCredential, setRotatedCredential] = useState<string | null>(null);
  const [reviewedRevision, setReviewedRevision] = useState<number | null>(null);
  const mountedRef = useRef(true);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  const host = worker.host;
  const pendingApply = applyPending(worker);

  // Load the report under review whenever the snapshot says one is pending.
  useEffect(() => {
    if (!pendingApply) {
      return;
    }
    void onLoadReported(worker.workerId).catch(() => {
      // The card shows the pending state; the Apply button stays disabled without a report.
    });
  }, [onLoadReported, pendingApply, worker.reportedRevision, worker.workerId]);

  const action = useCallback(
    async (operation: () => Promise<void>): Promise<void> => {
      setBusy(true);
      setError(null);
      try {
        await operation();
        if (mountedRef.current) {
          setPending(null);
        }
      } catch (actionError) {
        if (mountedRef.current) {
          setError(describeError(actionError));
        }
      } finally {
        if (mountedRef.current) {
          setBusy(false);
        }
      }
    },
    [],
  );

  const rotate = useCallback(async (): Promise<void> => {
    setBusy(true);
    setError(null);
    try {
      const response = await onRotateWorker(worker.workerId);
      if (mountedRef.current) {
        setRotatedCredential(response.credential);
        setPending(null);
      }
    } catch (rotateError) {
      if (mountedRef.current) {
        setError(describeError(rotateError));
      }
    } finally {
      if (mountedRef.current) {
        setBusy(false);
      }
    }
  }, [onRotateWorker, worker.workerId]);

  const revoke = useCallback(async (): Promise<void> => {
    setBusy(true);
    setError(null);
    try {
      await onRevokeWorker(worker.workerId);
      if (mountedRef.current) {
        setPending(null);
        setRotatedCredential(null);
      }
    } catch (revokeError) {
      if (mountedRef.current) {
        setError(describeError(revokeError));
      }
    } finally {
      if (mountedRef.current) {
        setBusy(false);
      }
    }
  }, [onRevokeWorker, worker.workerId]);

  /** Open Apply only after the Hub supplied the report the Admin is reviewing. */
  const beginApply = useCallback(async (): Promise<void> => {
    setBusy(true);
    setError(null);
    try {
      await onLoadReported(worker.workerId);
      if (mountedRef.current) {
        setReviewedRevision(worker.reportedRevision);
        setPending("apply");
      }
    } catch (loadError) {
      if (mountedRef.current) {
        setError(describeError(loadError));
      }
    } finally {
      if (mountedRef.current) {
        setBusy(false);
      }
    }
  }, [onLoadReported, worker.reportedRevision, worker.workerId]);

  const applyRevision = useCallback(async (): Promise<void> => {
    if (reviewedRevision === null) {
      return;
    }
    await action(async () => {
      await onApplyWorkerConfig(worker.workerId, reviewedRevision);
      setReviewedRevision(null);
    });
  }, [action, onApplyWorkerConfig, reviewedRevision, worker.workerId]);

  const canApprove = !worker.revoked && worker.approval === "pending";

  return (
    <article className="worker-card" data-worker-id={worker.workerId}>
      <header className="worker-card-header">
        <WorkerStatusBadge
          online={worker.online}
          revoked={worker.revoked}
          approval={worker.approval}
        />
        <span className="worker-id mono">{worker.workerId}</span>
        {worker.revoked ? (
          <button type="button" className="button button-sm" onClick={onOpenPairing}>
            Pair again
          </button>
        ) : (
          <span className="worker-actions">
            {canApprove ? (
              <button
                type="button"
                className="button button-sm"
                aria-label={`Approve ${worker.workerId}`}
                disabled={busy}
                onClick={() => {
                  void action(() => onApproveWorker(worker.workerId));
                }}
              >
                Approve
              </button>
            ) : null}
            {pendingApply || pending === "apply" ? (
              pending === "apply" ? (
                <>
                  <button
                    type="button"
                    className="button button-sm"
                    disabled={busy}
                    onClick={() => {
                      setPending(null);
                      setReviewedRevision(null);
                    }}
                  >
                    Cancel
                  </button>
                  <button
                    type="button"
                    className="button button-sm button-primary"
                    aria-busy={busy ? "true" : undefined}
                    disabled={busy || reviewedRevision === null}
                    onClick={() => {
                      void applyRevision();
                    }}
                  >
                    {busy ? "Applying…" : `Apply revision ${reviewedRevision ?? "?"}`}
                  </button>
                </>
              ) : (
                <button
                  type="button"
                  className="button button-sm"
                  disabled={busy}
                  onClick={() => {
                    void beginApply();
                  }}
                >
                  Review &amp; apply
                </button>
              )
            ) : null}
            {pending === "rotate" ? (
              <>
                <button
                  type="button"
                  className="button button-sm"
                  disabled={busy}
                  onClick={() => setPending(null)}
                >
                  Cancel
                </button>
                <button
                  type="button"
                  className="button button-sm button-primary"
                  aria-busy={busy ? "true" : undefined}
                  disabled={busy}
                  onClick={() => {
                    void rotate();
                  }}
                >
                  {busy ? "Rotating…" : "Confirm rotate"}
                </button>
              </>
            ) : (
              <button
                type="button"
                className="button button-sm"
                disabled={busy}
                onClick={() => {
                  setError(null);
                  setPending("rotate");
                }}
              >
                Rotate credential
              </button>
            )}
            {pending === "revoke" ? (
              <>
                <button
                  type="button"
                  className="button button-sm"
                  disabled={busy}
                  onClick={() => setPending(null)}
                >
                  Cancel
                </button>
                <button
                  type="button"
                  className="button button-sm button-danger"
                  aria-busy={busy ? "true" : undefined}
                  disabled={busy}
                  onClick={() => {
                    void revoke();
                  }}
                >
                  {busy ? "Revoking…" : "Confirm revoke"}
                </button>
              </>
            ) : (
              <button
                type="button"
                className="button button-sm"
                disabled={busy}
                onClick={() => {
                  setError(null);
                  setPending("revoke");
                }}
              >
                Revoke
              </button>
            )}
          </span>
        )}
      </header>

      {pending === "revoke" ? (
        <div className="alert alert-error" role="alert">
          <span>
            Revoking disconnects this Worker immediately and its stored credential stops working.
            This cannot be undone; pair the host again to reconnect.
          </span>
        </div>
      ) : null}

      {error ? (
        <div className="alert alert-error" role="alert">
          <span>{error}</span>
        </div>
      ) : null}

      {rotatedCredential ? (
        <div className="progress-block">
          <span className="field-label">New credential (shown once)</span>
          <div className="copy-row">
            <code className="code-block">{rotatedCredential}</code>
            <button
              type="button"
              className="button button-sm"
              onClick={() => onCopyText("worker credential", rotatedCredential)}
            >
              Copy
            </button>
          </div>
          <span className="field-hint">
            Import it on the host with <span className="mono">worker credential-import</span>.
          </span>
        </div>
      ) : null}

      {worker.approval === "pending" ? (
        <div className="alert alert-info" role="status">
          <span>
            Pending approval: this Worker is connected and reporting, but the Hub schedules no
            work to it until an Admin approves it.
          </span>
        </div>
      ) : null}

      <div className="worker-meta">
        <span>
          approval <strong>{worker.approval}</strong>
        </span>
        <span>
          reported{" "}
          <strong className="tabular">
            {worker.reportedRevision === null ? "—" : worker.reportedRevision}
          </strong>
        </span>
        <span>
          applied{" "}
          <strong className="tabular">
            {worker.appliedRevision === null ? "—" : worker.appliedRevision}
          </strong>
        </span>
        <span>
          epoch <strong className="tabular">{worker.connectionEpoch}</strong>
        </span>
        <span>
          last seen <strong>{formatRelativeTime(worker.lastSeenAtMs)}</strong>
        </span>
        <span>
          active Deliveries <strong className="tabular">{worker.activeDeliveries}</strong>
        </span>
        {host ? (
          <span className="mono">
            {host.os} / {host.arch} · {host.deploymentMode}
          </span>
        ) : (
          <span>no inventory reported</span>
        )}
      </div>

      {applyPending(worker) ? (
        <ConfigReview worker={worker} reported={reported} />
      ) : (
        <SlotTable slots={worker.appliedConfig?.slots ?? []} />
      )}
    </article>
  );
}

/* --------------------------------------------------------------------- */
/* View                                                                   */
/* --------------------------------------------------------------------- */

export interface WorkersViewProps {
  readonly baseUrl: string;
  readonly workers: readonly WorkerRecord[];
  readonly reported: Readonly<Record<string, WorkerSchedulingConfig | null>>;
  readonly issued: CreateEnrollmentCodeResponse | null;
  readonly busy: boolean;
  readonly onCreateEnrollmentCode: (
    request: CreateEnrollmentCodeRequest,
  ) => Promise<void>;
  readonly onLoadReported: (workerId: string) => Promise<void>;
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
    reported,
    onCreateEnrollmentCode,
    onLoadReported,
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

  const online = workers.filter(
    (worker) => !worker.revoked && worker.approval === "approved" && worker.online,
  ).length;
  const offline = workers.filter(
    (worker) => !worker.revoked && worker.approval === "approved" && !worker.online,
  ).length;
  const pending = workers.filter((worker) => !worker.revoked && worker.approval === "pending").length;
  const activeDeliveries = workers.reduce((total, worker) => total + worker.activeDeliveries, 0);

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

        <KpiStrip
          registered={workers.length}
          online={online}
          offline={offline}
          pending={pending}
          activeDeliveries={activeDeliveries}
        />

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
                reported={reported[worker.workerId] ?? null}
                onLoadReported={onLoadReported}
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
