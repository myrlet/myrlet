/**
 * Centred sign-in card for a same-origin (or explicit) Myrlet Hub.
 *
 * The Admin Bearer Token is held in component state for the lifetime of the
 * page only: it is never written to `localStorage`, URLs, or logs, and the card
 * says so explicitly. The Hub URL defaults to this page's origin.
 */

import { useState } from "react";
import type { FormEvent } from "react";
import { AlertCircle, Eye, EyeOff, Moon, ShieldCheck, Sun } from "lucide-react";

import type { Theme } from "../state/useHub";

export interface AuthGateProps {
  defaultBaseUrl: string;
  connecting: boolean;
  error: string | null;
  theme: Theme;
  onToggleTheme: () => void;
  onConnect: (baseUrl: string, token: string) => Promise<boolean>;
}

function BrandMark() {
  return (
    <span className="brand-mark" aria-hidden="true">
      <svg
        viewBox="0 0 20 20"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.4"
        strokeLinecap="round"
        strokeLinejoin="round"
      >
        <rect x="2.5" y="2.5" width="15" height="15" rx="4" />
        <path d="M6.6 13V7.1l3.4 3.1 3.4-3.1V13" />
      </svg>
    </span>
  );
}

export function AuthGate(props: AuthGateProps) {
  const { defaultBaseUrl, connecting, error, theme, onToggleTheme, onConnect } = props;

  const [baseUrl, setBaseUrl] = useState(defaultBaseUrl);
  const [token, setToken] = useState("");
  const [showToken, setShowToken] = useState(false);
  const [submitting, setSubmitting] = useState(false);

  const busy = connecting || submitting;
  const nextTheme: Theme = theme === "porcelain" ? "graphite" : "porcelain";
  const ThemeIcon = nextTheme === "graphite" ? Moon : Sun;

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
    <div className="auth-gate">
      <div className="auth-card">
        <div className="auth-brand">
          <BrandMark />
          <span className="brand-name">Myrlet</span>
          <span className="header-spacer" />
          <button
            type="button"
            className="button button-sm button-ghost"
            onClick={onToggleTheme}
            aria-label={`Switch to ${nextTheme === "graphite" ? "Graphite" : "Porcelain"} theme`}
            title={`Switch to ${nextTheme === "graphite" ? "Graphite" : "Porcelain"} theme`}
          >
            <ThemeIcon size={13} aria-hidden="true" />
            <span>{nextTheme === "graphite" ? "Graphite" : "Porcelain"}</span>
          </button>
        </div>

        <div className="auth-heading">
          <h1 className="auth-title">Connect to a Hub</h1>
          <p className="auth-subtitle">
            Sign in with the Admin Bearer Token issued by your Hub operator.
          </p>
        </div>

        <form className="auth-form" onSubmit={handleSubmit}>
          <div className="field">
            <label className="field-label" htmlFor="auth-hub-url">
              Hub URL
            </label>
            <input
              id="auth-hub-url"
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
            {busy ? "Connecting…" : "Connect"}
          </button>
        </form>

        <div className="auth-footer">
          <span>Hub-served workspace</span>
          <span className="mono">API v1</span>
        </div>
      </div>
    </div>
  );
}
