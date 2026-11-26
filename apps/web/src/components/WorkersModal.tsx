/**
 * Workers & Pairing drawer: one-time enrollment codes on top, enrolled
 * Worker/Slot inventory with rotate and revoke below.
 *
 * Inventory is reported by Workers and shown verbatim: advisory capacity of
 * `null` is unknown and renders as `—`, never `0`.
 */

import { useCallback, useEffect, useRef, useState } from "react";

import { HubApiError } from "../api/client";
import type {
  CreateEnrollmentCodeRequest,
  CreateEnrollmentCodeResponse,
  RotateWorkerResponse,
  WorkerRecord,
} from "../api/types";
import {
  CAPABILITY_STATE_LABELS,
  buildPairCommand,
  formatCapacity,
  formatRelativeTime,
  formatTimestamp,
} from "../format";
import { WorkerStatusBadge } from "./StatusBadge";

function describeError(error: unknown): string {
  if (error instanceof HubApiError) {
    return error.message;
  }
  if (error instanceof Error) {
    return error.message;
  }
  return "The Hub command could not be completed.";
}

const DEFAULT_TTL_SECONDS = 600;

/** Copy text, preferring the async Clipboard API and degrading to a no-op. */
async function copyText(text: string): Promise<boolean> {
  const clipboard: Clipboard | undefined =
    typeof navigator === "undefined" ? undefined : navigator.clipboard;
  if (!clipboard || typeof clipboard.writeText !== "function") {
    return false;
  }
  try {
    await clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}

interface CopyButtonProps {
  readonly text: string;
  readonly label?: string;
}

function CopyButton({ text, label = "Copy" }: CopyButtonProps) {
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (!copied) {
      return;
    }
    const timer = window.setTimeout(() => setCopied(false), 2_000);
    return () => window.clearTimeout(timer);
  }, [copied]);

  return (
    <button
      type="button"
      className="button button-sm"
      onClick={() => {
        void copyText(text).then((ok) => {
          if (ok) {
            setCopied(true);
          }
        });
      }}
    >
      {copied ? "Copied" : label}
    </button>
  );
}

/* --------------------------------------------------------------------- */
/* Enrollment                                                             */
/* --------------------------------------------------------------------- */

interface EnrollSectionProps {
  readonly baseUrl: string;
  readonly onCreateEnrollmentCode: (
    request: CreateEnrollmentCodeRequest,
  ) => Promise<CreateEnrollmentCodeResponse>;
}

function EnrollSection({ baseUrl, onCreateEnrollmentCode }: EnrollSectionProps) {
  const [ttlSeconds, setTtlSeconds] = useState(String(DEFAULT_TTL_SECONDS));
  const [suggestedWorkerId, setSuggestedWorkerId] = useState("");
  const [issued, setIssued] = useState<CreateEnrollmentCodeResponse | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = useCallback(async (): Promise<void> => {
    const parsedTtl = Number.parseInt(ttlSeconds, 10);
    if (!Number.isFinite(parsedTtl) || parsedTtl <= 0) {
      setError("The code lifetime must be a positive number of seconds.");
      return;
    }
    const suggested = suggestedWorkerId.trim();
    const request: CreateEnrollmentCodeRequest = {
      ttlSeconds: parsedTtl,
      suggestedWorkerId: suggested === "" ? null : suggested,
    };
    setBusy(true);
    setError(null);
    setIssued(null);
    try {
      const response = await onCreateEnrollmentCode(request);
      setIssued(response);
    } catch (issueError) {
      setError(describeError(issueError));
    } finally {
      setBusy(false);
    }
  }, [onCreateEnrollmentCode, suggestedWorkerId, ttlSeconds]);

  const pairCommand = issued ? buildPairCommand(baseUrl, issued.enrollmentCode) : "";

  return (
    <section className="detail-section">
      <h3 className="detail-section-title">Enroll New Worker</h3>
      <div className="form-grid">
        <div className="field">
          <label className="field-label" htmlFor="enroll-ttl">
            Code lifetime (seconds)
          </label>
          <input
            id="enroll-ttl"
            className="input input-mono"
            inputMode="numeric"
            value={ttlSeconds}
            disabled={busy}
            onChange={(event) => setTtlSeconds(event.target.value)}
          />
        </div>
        <div className="field">
          <label className="field-label" htmlFor="enroll-worker-id">
            Suggested Worker ID (optional)
          </label>
          <input
            id="enroll-worker-id"
            className="input input-mono"
            placeholder="wkr_…"
            value={suggestedWorkerId}
            disabled={busy}
            onChange={(event) => setSuggestedWorkerId(event.target.value)}
          />
        </div>
      </div>

      {error ? (
        <div className="alert alert-error" role="alert">
          <span>{error}</span>
        </div>
      ) : null}

      <div className="inline">
        <button
          type="button"
          className="button button-primary"
          aria-busy={busy ? "true" : undefined}
          disabled={busy}
          onClick={() => {
            void submit();
          }}
        >
          {busy ? "Generating…" : "Generate Pairing Code"}
        </button>
        <span className="field-hint">
          Single use. The code is shown once and expires automatically.
        </span>
      </div>

      {issued ? (
        <div className="progress-block">
          <span className="field-label">Pairing code</span>
          <span className="pair-code">{issued.enrollmentCode}</span>
          <span className="field-hint">
            Expires {formatTimestamp(issued.expiresAtMs)}
            {issued.suggestedWorkerId ? ` · suggested Worker ${issued.suggestedWorkerId}` : ""}
          </span>
          <span className="field-label">Pair command</span>
          <div className="copy-row">
            <code className="code-block">{pairCommand}</code>
            <CopyButton text={pairCommand} />
          </div>
          <span className="field-hint">
            Run this once on the new host. The Worker stores its credential locally.
          </span>
        </div>
      ) : null}
    </section>
  );
}

/* --------------------------------------------------------------------- */
/* Worker row                                                             */
/* --------------------------------------------------------------------- */

interface WorkerItemProps {
  readonly worker: WorkerRecord;
  readonly onRotateWorker: (workerId: string) => Promise<RotateWorkerResponse>;
  readonly onRevokeWorker: (workerId: string) => Promise<void>;
}

type PendingAction = "rotate" | "revoke" | null;

function WorkerItem({ worker, onRotateWorker, onRevokeWorker }: WorkerItemProps) {
  const [pending, setPending] = useState<PendingAction>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [rotated, setRotated] = useState<RotateWorkerResponse | null>(null);
  const mountedRef = useRef(true);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  const inventory = worker.inventory;
  const host = inventory?.host ?? null;

  const rotate = useCallback(async (): Promise<void> => {
    setBusy(true);
    setError(null);
    try {
      const response = await onRotateWorker(worker.workerId);
      if (mountedRef.current) {
        setRotated(response);
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
        setRotated(null);
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

  return (
    <div className="worker-item" data-worker-id={worker.workerId}>
      <div className="worker-item-header">
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
      </div>

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

      {rotated ? (
        <div className="progress-block">
          <span className="field-label">New credential</span>
          <div className="copy-row">
            <code className="code-block">{rotated.credential}</code>
            <CopyButton text={rotated.credential} />
          </div>
          <span className="field-hint">
            Shown once; import into the Worker with worker credential-import.
          </span>
        </div>
      ) : null}

      <div className="worker-meta">
        <span>
          epoch <strong>{worker.connectionEpoch}</strong>
        </span>
        <span>
          active Deliveries <strong>{worker.activeDeliveries}</strong>
        </span>
        <span>
          last seen <strong>{formatRelativeTime(worker.lastSeenAtMs)}</strong>
        </span>
        {host ? (
          <span>
            <strong>{host.os}</strong> / <strong>{host.arch}</strong> /{" "}
            <strong>{host.deploymentMode}</strong>
          </span>
        ) : (
          <span>no inventory reported</span>
        )}
      </div>

      {inventory && inventory.slots.length > 0 ? (
        <div className="slot-list">
          {inventory.slots.map((slot) => (
            <div className="slot-item" key={slot.slotId}>
              <span className={slot.enabled ? "slot-name" : "slot-name slot-disabled"}>
                {slot.slotId}
              </span>
              <span>{slot.enabled ? "enabled" : "disabled"}</span>
              <span>
                capacity <span className="tabular">{formatCapacity(slot.advisoryCapacity)}</span>
              </span>
              {slot.roles.length > 0 ? (
                <span>roles {slot.roles.join(", ")}</span>
              ) : (
                <span>no roles</span>
              )}
              {slot.capabilities.length > 0 ? (
                <span className="inline">
                  {slot.capabilities.map((capability) => (
                    <span
                      className="tag"
                      data-state={capability.state}
                      key={capability.name}
                      title={CAPABILITY_STATE_LABELS[capability.state]}
                    >
                      {capability.name} · {CAPABILITY_STATE_LABELS[capability.state]}
                    </span>
                  ))}
                </span>
              ) : (
                <span>no capabilities</span>
              )}
              {slot.executors.length > 0 ? (
                <span>
                  executors{" "}
                  {slot.executors.map((executor) => `${executor.id} (${executor.kind})`).join(", ")}
                </span>
              ) : (
                <span>no executors</span>
              )}
            </div>
          ))}
        </div>
      ) : (
        <span className="detail-empty">No Slots reported yet.</span>
      )}
    </div>
  );
}

/* --------------------------------------------------------------------- */
/* Drawer                                                                 */
/* --------------------------------------------------------------------- */

export interface WorkersModalProps {
  readonly open: boolean;
  readonly baseUrl: string;
  readonly workers: readonly WorkerRecord[];
  readonly onClose: () => void;
  readonly onCreateEnrollmentCode: (
    request: CreateEnrollmentCodeRequest,
  ) => Promise<CreateEnrollmentCodeResponse>;
  readonly onRotateWorker: (workerId: string) => Promise<RotateWorkerResponse>;
  readonly onRevokeWorker: (workerId: string) => Promise<void>;
}

export function WorkersModal({
  open,
  baseUrl,
  workers,
  onClose,
  onCreateEnrollmentCode,
  onRotateWorker,
  onRevokeWorker,
}: WorkersModalProps) {
  const bodyRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!open) {
      return;
    }
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.key === "Escape") {
        onClose();
      }
    };
    document.addEventListener("keydown", onKeyDown);
    const frame = window.setTimeout(() => {
      bodyRef.current?.focus();
    }, 0);
    return () => {
      document.removeEventListener("keydown", onKeyDown);
      window.clearTimeout(frame);
    };
  }, [open, onClose]);

  if (!open) {
    return null;
  }

  const removed = workers.filter((worker) => worker.revoked);

  return (
    <div
      className="drawer-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) {
          onClose();
        }
      }}
    >
      <div
        className="drawer-panel"
        role="dialog"
        aria-modal="true"
        aria-label="Workers and pairing"
      >
        <div className="drawer-header">
          <div className="dialog-heading">
            <h2 className="dialog-title">Workers &amp; Pairing</h2>
            <p className="dialog-description">
              {workers.length === 0
                ? "No Workers enrolled yet."
                : `${workers.length} enrolled · ${removed.length} revoked`}
            </p>
          </div>
          <button type="button" className="icon-button" aria-label="Close workers" onClick={onClose}>
            <span aria-hidden="true">✕</span>
          </button>
        </div>

        <div className="drawer-body" ref={bodyRef} tabIndex={-1}>
          <EnrollSection baseUrl={baseUrl} onCreateEnrollmentCode={onCreateEnrollmentCode} />

          <section className="detail-section">
            <h3 className="detail-section-title">Enrolled Workers</h3>
            {workers.length === 0 ? (
              <p className="detail-empty">
                Pair a host with the code above, then start its Worker daemon.
              </p>
            ) : (
              <div className="worker-list">
                {workers.map((worker) => (
                  <WorkerItem
                    key={worker.workerId}
                    worker={worker}
                    onRotateWorker={onRotateWorker}
                    onRevokeWorker={onRevokeWorker}
                  />
                ))}
              </div>
            )}
          </section>
        </div>

        <div className="drawer-footer">
          <button type="button" className="button" onClick={onClose}>
            Close
          </button>
        </div>
      </div>
    </div>
  );
}
