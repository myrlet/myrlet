/**
 * Overlay interaction contract at the workspace seam: the palette opening the
 * Workers drawer, the drawer closing before Task selection, and the Inspector
 * surviving underneath the drawer. These are the nesting paths the per-surface
 * tests cannot see.
 */

import "@testing-library/jest-dom/vitest";

import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import App from "../../App";
import type { GlobalSnapshot, SnapshotTaskSummary, TaskRecord, WorkerRecord } from "../../api/types";

type FetchHandler = (url: string, init: RequestInit) => Response | Promise<Response>;

function readUrl(input: RequestInfo | URL): string {
  if (typeof input === "string") {
    return input;
  }
  if (input instanceof URL) {
    return input.toString();
  }
  return input.url;
}

function stubFetch(handler: FetchHandler): void {
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => handler(readUrl(input), init ?? {})),
  );
}

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

/** An event stream that stays open: a live connection with no news. */
function idleEventStream(): Response {
  return new Response(
    new ReadableStream<Uint8Array>({
      start() {
        // Never enqueues or closes.
      },
    }),
    { status: 200, headers: { "Content-Type": "text/event-stream" } },
  );
}

function taskSummary(overrides: Partial<SnapshotTaskSummary> & { id: string }): SnapshotTaskSummary {
  return {
    parentTaskId: null,
    rootTaskId: overrides.id,
    depth: 1,
    sourceTaskId: null,
    dependencies: [],
    blockedBy: [],
    category: "general",
    title: overrides.id,
    status: "queued",
    version: 1,
    createdAtMs: 1_700_000_000_000,
    updatedAtMs: 1_700_000_000_000,
    finishedAtMs: null,
    currentDelivery: null,
    subtree: { total: 1, held: 0, queued: 1, dispatched: 0, finished: 0, cancelled: 0 },
    ...overrides,
  };
}

function workerRecord(): WorkerRecord {
  return {
    workerId: "wkr_00000000000a",
    online: true,
    revoked: false,
    approval: "approved",
    permissionTier: "worker",
    connectionEpoch: 2,
    lastSeenAtMs: Date.now(),
    activeDeliveries: 0,
    host: null,
    reportedRevision: null,
    appliedRevision: null,
    appliedConfig: null,
  };
}

function snapshot(): GlobalSnapshot {
  const tasks = [taskSummary({ id: "t1", title: "Ship release" })];
  return {
    schemaVersion: "myrlet.snapshot/v1",
    generatedAtMs: 1_700_000_000_500,
    eventCursor: 12,
    tasksTruncated: false,
    totalTasks: tasks.length,
    workers: [workerRecord()],
    tasks,
  };
}

function taskRecord(summary: SnapshotTaskSummary): TaskRecord {
  return {
    id: summary.id,
    rootTaskId: summary.rootTaskId,
    depth: summary.depth,
    status: summary.status,
    version: summary.version,
    createdAtMs: summary.createdAtMs,
    updatedAtMs: summary.updatedAtMs,
    finishedAtMs: summary.finishedAtMs,
    currentDelivery: summary.currentDelivery,
    blockedBy: summary.blockedBy,
    resultMarkdown: null,
    resultRecordedAtMs: null,
    subtree: summary.subtree,
    task: {
      clientRef: null,
      parentTaskId: summary.parentTaskId,
      sourceTaskId: summary.sourceTaskId,
      dependencies: summary.dependencies,
      category: summary.category,
      tags: [],
      title: summary.title,
      prompt: `Work on ${summary.title}.`,
      input: null,
      expectedOutput: null,
      externalReferences: [],
      routing: {},
      execution: {},
      metadata: {},
    },
  };
}

/** A controllable `matchMedia`; the panel stays wide unless a test asks. */
function stubMedia(narrow: boolean): void {
  const media = (matches: boolean, query: string): MediaQueryList =>
    ({
      get matches() {
        return matches;
      },
      media: query,
      onchange: null,
      addEventListener: () => undefined,
      removeEventListener: () => undefined,
      addListener: () => undefined,
      removeListener: () => undefined,
      dispatchEvent: () => true,
    }) as MediaQueryList;
  vi.stubGlobal("matchMedia", (query: string) => {
    const isNarrowQuery = query.includes("max-width");
    return media(isNarrowQuery ? narrow : false, query);
  });
}

async function connect(): Promise<void> {
  const input = document.querySelector<HTMLInputElement>('input[type="password"]');
  if (!input) {
    throw new Error("The Auth gate did not render its token field.");
  }
  fireEvent.change(input, { target: { value: "admin-token-value" } });
  const form = document.querySelector("form") as HTMLFormElement;
  fireEvent.submit(form);
  await waitFor(() => expect(document.querySelector(".app-bar")).not.toBeNull());
  // The App Bar paints before the workspace's passive effects register the
  // document `keydown` listener these tests press against.
  await act(async () => {});
}

function openList(): void {
  const group = screen.getByRole("group", { name: /workspace mode/i });
  fireEvent.click(within(group).getByRole("button", { name: /^list$/i }));
}

beforeEach(() => {
  stubMedia(false);
  stubFetch((url) => {
    const path = url.replace(/^https?:\/\/[^/]+/, "");
    if (path.startsWith("/v1/events")) {
      return idleEventStream();
    }
    if (path.startsWith("/v1/snapshot")) {
      return jsonResponse(snapshot());
    }
    if (path.startsWith("/v1/settings/security-trust")) {
      return jsonResponse({ requireWorkerApproval: true, requireConfigurationApply: true });
    }
    if (path.startsWith("/v1/workers")) {
      return jsonResponse({ workers: [workerRecord()] });
    }
    const [pathname] = path.split("?");
    const match = /^\/v1\/tasks\/([^/]+)(\/tree)?$/.exec(pathname);
    const id = match === null ? "t1" : decodeURIComponent(match[1]);
    const summary = snapshot().tasks.find((task) => task.id === id);
    if (summary === undefined) {
      return jsonResponse({ error: "NotFound", message: "Not found." }, 404);
    }
    if (match !== null && match[2] !== undefined) {
      return jsonResponse({ rootTaskId: summary.rootTaskId, tasks: [taskRecord(summary)], related: [], truncated: false });
    }
    return jsonResponse({ task: taskRecord(summary), summary: null, related: [] });
  });
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  window.localStorage.clear();
  delete document.documentElement.dataset.theme;
});

describe("palette and drawer nesting", () => {
  it("opens Workers from ⌘K, closes the palette, and returns focus to the real Workers pill", async () => {
    render(<App />);
    await connect();

    const workersPill = screen.getByRole("button", { name: /workers 1\/1/i });
    fireEvent.keyDown(document, { key: "k", ctrlKey: true });
    const palette = await screen.findByRole("dialog", { name: /command palette/i });
    fireEvent.click(within(palette).getByRole("option", { name: /workers & pairing/i }));

    const drawer = await screen.findByRole("dialog", { name: /workers and pairing/i });
    expect(screen.queryByRole("dialog", { name: /command palette/i })).toBeNull();
    // Focus enters the drawer panel, not the vanished palette option.
    await waitFor(() => expect(document.activeElement).toBe(drawer));

    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("dialog", { name: /workers and pairing/i })).toBeNull());
    // The App Bar pill is where focus belongs when the only other opener (the
    // palette option) is gone.
    await waitFor(() => expect(document.activeElement).toBe(workersPill));
    expect(document.activeElement).not.toBe(document.body);
  });

  it("returns focus to the Inspector's Manage shortcut, not the pill, when that is the opener", async () => {
    render(<App />);
    await connect();

    fireEvent.keyDown(document, { key: "i" });
    await waitFor(() => expect(document.querySelector(".inspector-pane")).not.toBeNull());
    const manage = screen.getByRole("button", { name: /manage workers/i });
    manage.focus();
    fireEvent.click(manage);

    const drawer = await screen.findByRole("dialog", { name: /workers and pairing/i });
    await waitFor(() => expect(document.activeElement).toBe(drawer));

    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("dialog", { name: /workers and pairing/i })).toBeNull());
    // The real opener is still mounted, so it wins over the App Bar fallback.
    await waitFor(() => expect(document.activeElement).toBe(manage));
  });

  it("closes the drawer on Escape before clearing the List selection or the Inspector", async () => {
    render(<App />);
    await connect();
    openList();

    fireEvent.click(document.querySelector('.list-row[data-task-id="t1"]') as HTMLElement);
    await waitFor(() => expect(document.querySelector(".detail-pane")).not.toBeNull());
    expect(document.querySelector(".inspector-slot")).not.toBeNull();

    const workersPill = screen.getByRole("button", { name: /workers 1\/1/i });
    fireEvent.click(workersPill);
    const drawer = await screen.findByRole("dialog", { name: /workers and pairing/i });
    await waitFor(() => expect(document.activeElement).toBe(drawer));

    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("dialog", { name: /workers and pairing/i })).toBeNull());
    // Selection and Inspector survive that first Escape.
    expect(document.querySelector(".detail-pane")).not.toBeNull();
    expect(document.querySelector(".inspector-slot")).not.toBeNull();

    // A second Escape now belongs to the selection.
    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() => expect(document.querySelector(".inspector-slot")).toBeNull());
  });

  it("opens the palette over the Workers drawer and hands focus back to the drawer on dismissal", async () => {
    render(<App />);
    await connect();

    fireEvent.click(screen.getByRole("button", { name: /workers 1\/1/i }));
    const drawer = await screen.findByRole("dialog", { name: /workers and pairing/i });
    await waitFor(() => expect(document.activeElement).toBe(drawer));

    fireEvent.keyDown(document, { key: "k", ctrlKey: true });
    const palette = await screen.findByRole("dialog", { name: /command palette/i });
    const paletteInput = within(palette).getByRole("textbox");
    await waitFor(() => expect(document.activeElement).toBe(paletteInput));
    // Both surfaces own their layers: the drawer stays mounted underneath.
    expect(screen.getByRole("dialog", { name: /workers and pairing/i })).toBeInTheDocument();

    // Escape closes the palette first, leaving the drawer open.
    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() =>
      expect(screen.queryByRole("dialog", { name: /command palette/i })).toBeNull(),
    );
    expect(screen.getByRole("dialog", { name: /workers and pairing/i })).toBeInTheDocument();
    // Focus returns to the drawer that owned it before the palette opened.
    await waitFor(() => expect(document.activeElement).toBe(drawer));

    // The drawer still catches its own Tab cycle, then its own Escape.
    const closeButton = screen.getByRole("button", { name: /close workers drawer/i });
    closeButton.focus();
    fireEvent.keyDown(closeButton, { key: "Tab" });
    expect(drawer.contains(document.activeElement)).toBe(true);

    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() =>
      expect(screen.queryByRole("dialog", { name: /workers and pairing/i })).toBeNull(),
    );
  });
});
