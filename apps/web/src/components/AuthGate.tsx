/**
 * Sign-in surface for a same-origin (or explicit) Myrlet Hub.
 *
 * The Admin Bearer Token is held in component state for the lifetime of the
 * page only: it is never written to `localStorage`, URLs, or logs, and the card
 * says so explicitly. The Hub origin field gets a debounced, live
 * `probeHubHealth` reachability read so an operator can tell "wrong origin"
 * from "wrong token" before submitting.
 */

import { useEffect, useState } from "react";
import type { FormEvent } from "react";
import { AlertCircle, Eye, EyeOff, ShieldCheck } from "lucide-react";

import { THEMES, probeHubHealth } from "../state/useHub";
import type { Theme } from "../state/useHub";
import { BrandMark } from "./BrandMark";

export interface AuthGateProps {
  defaultBaseUrl: string;
  connecting: boolean;
  error: string | null;
  theme: Theme;
  onChangeTheme: (theme: Theme) => void;
  onConnect: (baseUrl: string, token: string) => Promise<boolean>;
}

const PROBE_DEBOUNCE_MS = 450;

type ProbeState =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "reachable"; version: string | null }
  | { kind: "unreachable" };

function probeLabel(probe: ProbeState): string {
  switch (probe.kind) {
    case "idle":
      return "Awaiting origin";
    case "checking":
      return "Checking…";
    case "reachable":
      return probe.version ? `Reachable · v${probe.version}` : "Reachable";
    case "unreachable":
      return "Unreachable";
  }
}

function probeTitle(probe: ProbeState): string {
  switch (probe.kind) {
    case "idle":
      return "Enter a Hub origin to check reachability.";
    case "checking":
      return "Probing this Hub origin for process liveness…";
    case "reachable":
      return "The Hub answered its health probe. This says nothing about the token.";
    case "unreachable":
      return "No Hub answered at this origin. Check the URL and that the Hub is running.";
  }
}

export function AuthGate(props: AuthGateProps) {
  const { defaultBaseUrl, connecting, error, theme, onChangeTheme, onConnect } = props;

  const [baseUrl, setBaseUrl] = useState(defaultBaseUrl);
  const [token, setToken] = useState("");
  const [showToken, setShowToken] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [probe, setProbe] = useState<ProbeState>({ kind: "idle" });

  const busy = connecting || submitting;

  // Debounced live reachability probe. Every edit cancels the in-flight result
  // so a stale origin can never be shown as reachable.
  useEffect(() => {
    const target = baseUrl.trim();
    if (target === "") {
      setProbe({ kind: "idle" });
      return;
    }

    let cancelled = false;
    setProbe({ kind: "checking" });
    const timer = window.setTimeout(() => {
      void probeHubHealth(target)
        .then((result) => {
          if (cancelled) {
            return;
          }
          setProbe(
            result.ok
              ? { kind: "reachable", version: result.version ?? null }
              : { kind: "unreachable" },
          );
        })
        .catch(() => {
          if (!cancelled) {
            setProbe({ kind: "unreachable" });
          }
        });
    }, PROBE_DEBOUNCE_MS);

    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [baseUrl]);

  const handleSubmit = async (event: FormEvent<HTMLFormElement>): Promise<void> => {
    event.preventDefault();
    if (busy || token.trim() === "") {
      return;
    }
    setSubmitting(true);
    try {
      // The token stays in state when connecting fails so it can be corrected
      // (for example an expired credential) without re-pasting.
      await onConnect(baseUrl, token);
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div className="auth-shell">
      <header className="auth-topbar">
        <span className="auth-brand">
          <BrandMark size={22} />
          <span className="brand-name">Myrlet</span>
          <span className="auth-topbar-tagline">Task &amp; Worker Coordination</span>
        </span>
        <span className="header-spacer" />
        <span className="connection-pill" data-state="disconnected">
          <span className="connection-dot" aria-hidden="true" />
          <span>Not connected</span>
        </span>
        <select
          className="select filter-select"
          aria-label="Theme"
          value={theme}
          onChange={(event) => onChangeTheme(event.target.value as Theme)}
        >
          {THEMES.map((option) => (
            <option key={option.id} value={option.id}>
              {option.label}
            </option>
          ))}
        </select>
      </header>

      <main className="auth-canvas">
        <div className="auth-card" style={{ maxWidth: 420 }}>
          <div className="auth-heading">
            <h1 className="auth-title">Connect to a Hub</h1>
            <p className="auth-subtitle">
              Sign in with the Admin Bearer Token issued by your Hub operator.
            </p>
          </div>

          <form className="auth-form" onSubmit={handleSubmit}>
            <div className="field">
              <div className="field-label-row">
                <label className="field-label" htmlFor="auth-hub-origin">
                  Hub Origin
                </label>
                <span className="header-spacer" />
                <span
                  className="reachability-pill"
                  data-state={probe.kind}
                  title={probeTitle(probe)}
                  aria-live="polite"
                >
                  <span className="connection-dot" aria-hidden="true" />
                  <span>{probeLabel(probe)}</span>
                </span>
              </div>
              <input
                id="auth-hub-origin"
                className="input input-mono"
                type="text"
                inputMode="url"
                autoComplete="url"
                autoCapitalize="none"
                spellCheck={false}
                placeholder="https://hub.example:4140"
                value={baseUrl}
                onChange={(event) => setBaseUrl(event.target.value)}
                disabled={busy}
              />
              <span className="field-hint">
                Same-origin by default. Leave unchanged to use this page&apos;s origin.
              </span>
            </div>

            <div className="field">
              <label className="field-label" htmlFor="auth-admin-token">
                Admin Bearer Token
              </label>
              <div className="input-affix">
                <input
                  id="auth-admin-token"
                  className="input input-mono"
                  type={showToken ? "text" : "password"}
                  name="adminBearerToken"
                  autoComplete="off"
                  autoCapitalize="none"
                  autoCorrect="off"
                  spellCheck={false}
                  required
                  autoFocus
                  aria-describedby="auth-token-note"
                  value={token}
                  onChange={(event) => setToken(event.target.value)}
                  disabled={busy}
                />
                <button
                  type="button"
                  className="icon-button"
                  onClick={() => setShowToken((current) => !current)}
                  aria-label={showToken ? "Hide Admin Bearer Token" : "Show Admin Bearer Token"}
                  aria-pressed={showToken}
                  disabled={busy}
                >
                  {showToken ? (
                    <EyeOff size={14} aria-hidden="true" />
                  ) : (
                    <Eye size={14} aria-hidden="true" />
                  )}
                </button>
              </div>
            </div>

            <p className="auth-note" id="auth-token-note">
              <ShieldCheck size={13} aria-hidden="true" />
              <span>Token is kept in browser memory only and cleared on page reload.</span>
            </p>

            {error ? (
              <div className="alert alert-error" role="alert">
                <AlertCircle size={14} aria-hidden="true" />
                <span>{error}</span>
              </div>
            ) : null}

            <button
              type="submit"
              className="button button-primary button-block"
              disabled={busy}
              aria-busy={busy}
            >
              {busy ? "Connecting…" : "Connect to Hub"}
            </button>
          </form>

          <div className="auth-footer">
            <span>Hub-served workspace</span>
            <span className="mono">API v1</span>
          </div>
        </div>
      </main>
    </div>
  );
}
