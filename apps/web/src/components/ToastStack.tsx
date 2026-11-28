/**
 * Notification Toast stack for mutation results (copy, rotate, revoke).
 *
 * The queue itself lives in `useToasts`; this surface only announces what the
 * workspace just did and lets the operator dismiss it early.
 */

export interface ToastItem {
  readonly id: string;
  readonly message: string;
  readonly tone?: "neutral" | "error";
}

export interface ToastStackProps {
  readonly toasts: readonly ToastItem[];
  readonly onDismiss: (id: string) => void;
}

export function ToastStack({ toasts, onDismiss }: ToastStackProps) {
  if (toasts.length === 0) {
    return null;
  }

  return (
    <div className="toast-stack" role="region" aria-label="Notifications" aria-live="polite">
      {toasts.map((toast) => (
        <div
          className="toast"
          data-tone={toast.tone ?? "neutral"}
          role={toast.tone === "error" ? "alert" : "status"}
          key={toast.id}
        >
          <span className="toast-message">{toast.message}</span>
          <button
            type="button"
            className="toast-dismiss"
            aria-label={`Dismiss notification: ${toast.message}`}
            onClick={() => onDismiss(toast.id)}
          >
            <span aria-hidden="true">✕</span>
          </button>
        </div>
      ))}
    </div>
  );
}
