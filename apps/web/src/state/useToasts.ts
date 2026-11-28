import { useCallback, useEffect, useRef, useState } from "react";

import type { ToastItem } from "../components/ToastStack";

const TOAST_TIMEOUT_MS = 3_800;

export function useToasts() {
  const [toasts, setToasts] = useState<readonly ToastItem[]>([]);
  const toastIdRef = useRef(0);
  const toastTimersRef = useRef<number[]>([]);

  const dismissToast = useCallback((id: string) => {
    setToasts((current) => current.filter((toast) => toast.id !== id));
  }, []);

  const notify = useCallback((message: string, tone: "neutral" | "error" = "neutral") => {
    toastIdRef.current += 1;
    const id = `toast-${toastIdRef.current}`;
    setToasts((current) => [...current, { id, message, tone }]);
    const timer = window.setTimeout(() => {
      setToasts((current) => current.filter((toast) => toast.id !== id));
      toastTimersRef.current = toastTimersRef.current.filter((handle) => handle !== timer);
    }, TOAST_TIMEOUT_MS);
    toastTimersRef.current.push(timer);
  }, []);

  useEffect(
    () => () => {
      for (const timer of toastTimersRef.current) {
        window.clearTimeout(timer);
      }
      toastTimersRef.current = [];
    },
    [],
  );

  return { toasts, dismissToast, notify };
}
