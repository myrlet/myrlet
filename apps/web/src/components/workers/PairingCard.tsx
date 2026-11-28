/**
 * One-step pairing card: issues a single-use enrollment code and shows the
 * copy-ready `worker pair` command once.
 *
 * The code lifetime and optional suggested Worker ID are form-local; the issued
 * code itself is session state owned above, so the card locks after issuing.
 * Validation stays here because it guards this form's own inputs.
 */

import { useCallback, useState } from "react";

import type {
  CreateEnrollmentCodeRequest,
  CreateEnrollmentCodeResponse,
} from "../../api/types";
import { buildPairCommand, formatTimestamp } from "../../format";
import { describeIssueError } from "./workerErrors";

const DEFAULT_TTL_SECONDS = 600;

export interface PairingCardProps {
  readonly baseUrl: string;
  readonly issued: CreateEnrollmentCodeResponse | null;
  readonly busy: boolean;
  readonly onCreateEnrollmentCode: (
    request: CreateEnrollmentCodeRequest,
  ) => Promise<void>;
  readonly onCopyText: (label: string, text: string) => void;
}

export function PairingCard({
  baseUrl,
  issued,
  busy,
  onCreateEnrollmentCode,
  onCopyText,
}: PairingCardProps) {
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
      setError(describeIssueError(issueError));
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
