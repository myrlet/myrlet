/**
 * Workers drawer keyboard and focus contract, exercised on real boundary
 * behavior: it is a modal sibling over a still-mounted workspace, Tab wraps at
 * both ends, Escape runs after the drawer's own handlers (so a nested cancel
 * arm still wins), and closing returns focus to the real pill that opened it.
 */

import "@testing-library/jest-dom/vitest";

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useEffect, useRef, useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type {
  CreateEnrollmentCodeRequest,
  RotateWorkerResponse,
  WorkerRecord,
} from "../../api/types";
import { WorkersDrawer } from "../../components/WorkersDrawer";

function worker(overrides: Partial<WorkerRecord> = {}): WorkerRecord {
  return {
    workerId: "wkr_00000000000a",
    online: true,
    revoked: false,
    connectionEpoch: 4,
    lastSeenAtMs: 1_700_000_000_000,
    activeDeliveries: 1,
    inventory: {
      revision: 2,
      observedAtMs: 1_700_000_000_000,
      host: { os: "darwin", arch: "arm64", deploymentMode: "native" },
      slots: [
        {
          slotId: "mac-default",
          enabled: true,
          advisoryCapacity: null,
          roles: ["coding"],
          capabilities: [{ name: "rust", state: "available" }],
          executors: [{ id: "claude-code", kind: "claude" }],
        },
      ],
    },
    ...overrides,
  };
}

interface HarnessProps {
  readonly workers: readonly WorkerRecord[];
  readonly onClose?: () => void;
  readonly onCreateEnrollmentCode?: (
    request: CreateEnrollmentCodeRequest,
  ) => Promise<void>;
  readonly onRotateWorker?: (workerId: string) => Promise<RotateWorkerResponse>;
  readonly onRevokeWorker?: (workerId: string) => Promise<void>;
}

/** The App Bar pill plus the drawer it opens, as the workspace composes them. */
function Harness({
  workers,
  onClose,
  onCreateEnrollmentCode,
  onRotateWorker,
  onRevokeWorker,
}: HarnessProps) {
  const [open, setOpen] = useState(false);
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  // Mirrors the workspace Escape priority: the topmost overlay closes first and
  // only when no nested surface consumed the key.
  useEffect(() => {
    if (!open) {
      return;
    }
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.key === "Escape" && !event.defaultPrevented) {
        event.preventDefault();
        onClose?.();
        setOpen(false);
      }
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [onClose, open]);
  return (
    <>
      <button
        ref={triggerRef}
        type="button"
        aria-label={`Workers ${workers.length}/${workers.length}`}
        aria-expanded={open}
        onClick={() => setOpen((current) => !current)}
      >
        Workers
      </button>
      {open ? (
        <WorkersDrawer
          returnFocusRef={triggerRef}
          onlineWorkerCount={workers.filter((record) => record.online).length}
          baseUrl=""
          workers={workers}
          issued={null}
          busy={false}
          onRefresh={() => undefined}
          onClose={() => {
            onClose?.();
            setOpen(false);
          }}
          onCreateEnrollmentCode={
            onCreateEnrollmentCode ?? (async () => undefined)
          }
          onRotateWorker={
            onRotateWorker ??
            (async (workerId: string) => ({
              hubId: "hub_test",
              workerId,
              credential: "rotated",
            }))
          }
          onRevokeWorker={onRevokeWorker ?? (async () => undefined)}
          onCopyText={() => undefined}
        />
      ) : null}
    </>
  );
}

function openDrawer(): HTMLElement {
  const trigger = screen.getByRole("button", { name: /workers/i });
  trigger.focus();
  fireEvent.click(trigger);
  return trigger;
}

function drawer(): HTMLElement {
  return screen.getByRole("dialog", { name: /workers and pairing/i });
}

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("Workers drawer focus", () => {
  it("moves focus into the panel on open and returns it to the real opening pill", async () => {
    render(<Harness workers={[worker()]} />);
    const trigger = openDrawer();

    const panel = drawer();
    await waitFor(() => expect(document.activeElement).toBe(panel));
    expect(panel).toHaveAttribute("tabindex", "-1");

    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: /workers and pairing/i })).toBeNull();
    await waitFor(() => expect(document.activeElement).toBe(trigger));
    expect(trigger).toHaveAttribute("aria-expanded", "false");
  });

  it("wraps Tab forward off the last control and backward off the first", async () => {
    render(<Harness workers={[worker()]} />);
    openDrawer();
    const panel = drawer();
    await waitFor(() => expect(document.activeElement).toBe(panel));

    const focusable = Array.from(
      panel.querySelectorAll<HTMLElement>("button:not([disabled]), input:not([disabled])"),
    );
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    expect(first).toBeDefined();
    expect(last).toBeDefined();
    expect(first).not.toBe(last);

    last.focus();
    expect(fireEvent.keyDown(last, { key: "Tab" })).toBe(false);
    expect(document.activeElement).toBe(first);

    expect(fireEvent.keyDown(first, { key: "Tab", shiftKey: true })).toBe(false);
    expect(document.activeElement).toBe(last);
  });

  it("does not steal Tab from a control in the middle of the panel", async () => {
    render(<Harness workers={[worker()]} />);
    openDrawer();
    const panel = drawer();
    const middle = panel.querySelector<HTMLElement>(".view-search") as HTMLElement;
    middle.focus();
    expect(fireEvent.keyDown(middle, { key: "Tab" })).toBe(true);
    expect(document.activeElement).toBe(middle);
  });

  it("lets a nested confirmation cancel keep the drawer open on Escape", async () => {
    const onClose = vi.fn();
    render(<Harness workers={[worker()]} onClose={onClose} />);
    openDrawer();
    const panel = drawer();

    fireEvent.click(screen.getByRole("button", { name: /^revoke$/i }));
    expect(screen.getByRole("button", { name: /confirm revoke/i })).toBeInTheDocument();
    expect(panel).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /^cancel$/i }));
    expect(screen.queryByRole("button", { name: /confirm revoke/i })).toBeNull();
    expect(screen.getByRole("dialog", { name: /workers and pairing/i })).toBeInTheDocument();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("closes on a backdrop press but not on a press inside the panel", async () => {
    const onClose = vi.fn();
    render(<Harness workers={[worker()]} onClose={onClose} />);
    openDrawer();
    const panel = drawer();

    fireEvent.mouseDown(panel);
    expect(onClose).not.toHaveBeenCalled();

    fireEvent.mouseDown(panel.parentElement as HTMLElement);
    expect(onClose).toHaveBeenCalledTimes(1);
    await waitFor(() =>
      expect(screen.queryByRole("dialog", { name: /workers and pairing/i })).toBeNull(),
    );
  });
});
