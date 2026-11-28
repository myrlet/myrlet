/**
 * One enrolled Worker: live trust state, credential lifecycle, and its Slots.
 *
 * The card is the presentation surface for `useWorkerActions`, which owns the
 * armed confirmation, the in-flight flag, honest error wording, the reviewed
 * revision, and the once-shown rotated credential. Apply is offered only while
 * the Hub reports a newer revision than the one it schedules with; its report is
 * loaded as soon as the Snapshot says one is pending, so the review is ready
 * before an Admin can act on it.
 */

import { useEffect } from "react";

import type {
  RotateWorkerResponse,
  WorkerRecord,
  WorkerSchedulingConfig,
} from "../../api/types";
import { formatRelativeTime } from "../../format";
import { WorkerStatusBadge } from "../StatusBadge";
import { ConfigReview } from "./ConfigReview";
import { SlotTable } from "./WorkerInventory";
import { useWorkerActions } from "./useWorkerActions";
import { applyPending } from "./workerFilters";

export interface WorkerCardProps {
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

export function WorkerCard({
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
  const {
    pending,
    busy,
    error,
    rotatedCredential,
    reviewedRevision,
    request,
    cancel,
    rotate,
    revoke,
    approve,
    review,
    apply,
  } = useWorkerActions({
    workerId: worker.workerId,
    reportedRevision: worker.reportedRevision,
    onLoadReported,
    onApproveWorker,
    onApplyWorkerConfig,
    onRotateWorker,
    onRevokeWorker,
  });

  const host = worker.host;
  const pendingApply = applyPending(worker);
  const canApprove = !worker.revoked && worker.approval === "pending";

  // Load the report under review whenever the Snapshot says one is pending. The
  // card still shows the pending state if the read fails; Apply stays armed for
  // the exact revision only after `review` succeeds.
  useEffect(() => {
    if (!pendingApply) {
      return;
    }
    void onLoadReported(worker.workerId).catch(() => {
      // Apply stays unavailable without the report the Admin is entitled to see.
    });
  }, [onLoadReported, pendingApply, worker.reportedRevision, worker.workerId]);

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
              pending === "apply" ? (
                <>
                  <button type="button" className="button button-sm" disabled={busy} onClick={cancel}>
                    Cancel
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
              ) : (
                <button
                  type="button"
                  className="button button-sm"
                  disabled={busy}
                  onClick={() => {
                    void review();
                  }}
                >
                  Review &amp; apply
                </button>
              )
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

      {applyPending(worker) || pending === "apply" ? (
        <ConfigReview worker={worker} reported={reported} />
      ) : (
        <SlotTable
          slots={worker.appliedConfig?.slots ?? []}
          caption={`Applied Slots for ${worker.workerId}: enabled state, advisory capacity, roles, capabilities, and executors`}
        />
      )}
    </article>
  );
}
