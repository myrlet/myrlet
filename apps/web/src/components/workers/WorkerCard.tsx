/**
 * One enrolled Worker: live status, credential lifecycle, and reported Slots.
 *
 * The card is the presentation surface for `useWorkerActions`; it owns no
 * request state of its own. Revoking first shows what withdrawing the credential
 * means, and a rotated credential is displayed once with its import command.
 */

import type { RotateWorkerResponse, WorkerRecord } from "../../api/types";
import { formatRelativeTime } from "../../format";
import { WorkerStatusBadge } from "../StatusBadge";
import { SlotTable } from "./WorkerInventory";
import { useWorkerActions } from "./useWorkerActions";

export interface WorkerCardProps {
  readonly worker: WorkerRecord;
  readonly onRotateWorker: (workerId: string) => Promise<RotateWorkerResponse>;
  readonly onRevokeWorker: (workerId: string) => Promise<void>;
  readonly onCopyText: (label: string, text: string) => void;
}

export function WorkerCard({ worker, onRotateWorker, onRevokeWorker, onCopyText }: WorkerCardProps) {
  const { pending, busy, error, rotatedCredential, request, cancel, rotate, revoke } =
    useWorkerActions({ workerId: worker.workerId, onRotateWorker, onRevokeWorker });

  const host = worker.inventory?.host ?? null;

  return (
    <article className="worker-card" data-worker-id={worker.workerId}>
      <header className="worker-card-header">
        <WorkerStatusBadge online={worker.online} revoked={worker.revoked} />
        <span className="worker-id mono">{worker.workerId}</span>
        {worker.revoked ? null : (
          <span className="worker-actions">
            {pending === "rotate" ? (
              <>
                <button
                  type="button"
                  className="button button-sm"
                  disabled={busy}
                  onClick={cancel}
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
                onClick={() => request("rotate")}
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
                  onClick={cancel}
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
            This cannot be undone; enroll the host again to reconnect.
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

      <div className="worker-meta">
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

      <SlotTable worker={worker} />
    </article>
  );
}
