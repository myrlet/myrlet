/**
 * One enrolled Worker: live trust state, credential lifecycle, and its Slots.
 *
 * The card is the presentation surface for `useWorkerActions`, which owns the
 * armed confirmation, the in-flight flag, honest error wording, and the once-shown
 * rotated credential. The report under review comes from `useWorkerConfigReview`,
 * which binds the displayed body to the exact revision Apply submits. The review is
 * loaded as soon as the Snapshot says a report is pending, so it is ready before an
 * Admin can act on it.
 */

import { useEffect } from "react";

import type { RotateWorkerResponse, WorkerDetail, WorkerRecord } from "../../api/types";
import { formatRelativeTime } from "../../format";
import { useWorkerConfigReview } from "../../state/useWorkerConfigReview";
import { WorkerStatusBadge } from "../StatusBadge";
import { ConfigReview } from "./ConfigReview";
import { SlotTable } from "./WorkerInventory";
import { useWorkerActions } from "./useWorkerActions";
import { applyPending } from "./workerFilters";

export interface WorkerCardProps {
  readonly worker: WorkerRecord;
  /** Read one Worker's detail; revision and report come from that single response. */
  readonly fetchWorkerDetail: (workerId: string) => Promise<WorkerDetail>;
  readonly onApproveWorker: (workerId: string) => Promise<void>;
  readonly onApplyWorkerConfig: (workerId: string, revision: number) => Promise<void>;
  readonly onRotateWorker: (workerId: string) => Promise<RotateWorkerResponse>;
  readonly onRevokeWorker: (workerId: string) => Promise<void>;
  readonly onOpenPairing: () => void;
  readonly onCopyText: (label: string, text: string) => void;
}

export function WorkerCard({
  worker,
  fetchWorkerDetail,
  onApproveWorker,
  onApplyWorkerConfig,
  onRotateWorker,
  onRevokeWorker,
  onOpenPairing,
  onCopyText,
}: WorkerCardProps) {
  const review = useWorkerConfigReview({
    workerId: worker.workerId,
    reportedRevision: worker.reportedRevision,
    fetchDetail: fetchWorkerDetail,
  });

  const {
    pending,
    busy,
    error,
    rotatedCredential,
    request,
    cancel,
    rotate,
    revoke,
    approve,
    review: reloadReview,
    apply,
  } = useWorkerActions({
    workerId: worker.workerId,
    review,
    onApproveWorker,
    onApplyWorkerConfig,
    onRotateWorker,
    onRevokeWorker,
  });

  const host = worker.host;
  const pendingApply = applyPending(worker);
  const canApprove = !worker.revoked && worker.approval === "pending";
  const reviewedRevision = review.reviewed?.revision ?? null;

  // Load the report under review as soon as the Snapshot says one is pending, so the
  // Admin sees the exact body (and revision) they would apply. A failed read leaves the
  // card in its pending state; Apply stays disabled until a report is showing.
  const { load: loadReview } = review;
  useEffect(() => {
    if (!pendingApply) {
      return;
    }
    void loadReview();
  }, [loadReview, pendingApply]);

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
                  void approve();
                }}
              >
                Approve
              </button>
            ) : null}
            {pendingApply || pending === "apply" ? (
              <>
                <button
                  type="button"
                  className="button button-sm"
                  disabled={busy || review.loading}
                  onClick={() => {
                    void reloadReview();
                  }}
                >
                  {review.loading ? "Reloading…" : "Reload report"}
                </button>
                <button
                  type="button"
                  className="button button-sm button-primary"
                  aria-busy={busy ? "true" : undefined}
                  disabled={busy || reviewedRevision === null}
                  onClick={() => {
                    void apply();
                  }}
                >
                  {busy ? "Applying…" : `Apply revision ${reviewedRevision ?? "?"}`}
                </button>
              </>
            ) : null}
            {pending === "rotate" ? (
              <>
                <button type="button" className="button button-sm" disabled={busy} onClick={cancel}>
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
                onClick={() => request("rotate")}
              >
                Rotate credential
              </button>
            )}
            {pending === "revoke" ? (
              <>
                <button type="button" className="button button-sm" disabled={busy} onClick={cancel}>
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
                onClick={() => request("revoke")}
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

      {error ?? review.error ? (
        <div className="alert alert-error" role="alert">
          <span>{error ?? review.error}</span>
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

      {applyPending(worker) || pending === "apply" ? (
        <ConfigReview
          worker={worker}
          reviewed={review.reviewed}
          loading={review.loading}
        />
      ) : (
        <SlotTable
          slots={worker.appliedConfig?.slots ?? []}
          caption={`Applied Slots for ${worker.workerId}: enabled state, advisory capacity, roles, capabilities, and executors`}
        />
      )}
    </article>
  );
}
