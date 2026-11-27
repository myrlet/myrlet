/**
 * Frontend fact-honesty, transport, and end-to-end Work Atlas tests.
 *
 * These tests assert the truthful wording and provenance rules the design
 * system depends on (never "Completed"/"Succeeded", unknown capacity is `—`,
 * tokens never reach storage) plus the single-workspace chrome: 52px App Bar
 * with a centered `Board | List` switch, a 42px workspace toolbar, a
 * full-bleed Board by default (Choice A — Inspector collapsed), the Workers
 * drawer, and no Events tab or left sidebar rail.
 */

import "@testing-library/jest-dom/vitest";

import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import App from "../App";
import { HubClient } from "../api/client";
import type {
  GlobalSnapshot,
  HubEventEnvelope,
  SnapshotTaskSummary,
  TaskRecord,
  WorkerRecord,
} from "../api/types";
import { TASK_STATUS_META, buildTaskTreeRows, formatCapacity } from "../format";

/* --------------------------------------------------------------------- */
/* Fixtures and fetch plumbing                                           */
/* --------------------------------------------------------------------- */

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
  const mock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) =>
    handler(readUrl(input), init ?? {}),
  );
  vi.stubGlobal("fetch", mock);
}

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

function streamResponse(chunks: readonly string[]): Response {
  const encoder = new TextEncoder();
  const stream = new ReadableStream<Uint8Array>({
    start(controller) {
      for (const chunk of chunks) {
        controller.enqueue(encoder.encode(chunk));
      }
      controller.close();
    },
  });
  return new Response(stream, {
    status: 200,
    headers: { "Content-Type": "text/event-stream" },
  });
}

/** A Hub event stream that stays open, like a live connection with no news. */
function idleEventStream(): Response {
  const stream = new ReadableStream<Uint8Array>({
    start() {
      // Intentionally never enqueues or closes: the SSE read stays pending.
    },
  });
  return new Response(stream, {
    status: 200,
    headers: { "Content-Type": "text/event-stream" },
  });
}

function taskSummary(overrides: Partial<SnapshotTaskSummary> & { id: string }): SnapshotTaskSummary {
  return {
    parentTaskId: null,
    rootTaskId: overrides.id,
    depth: 1,
    sourceTaskId: null,
    dependencies: [],
    category: "general",
    title: overrides.id,
    status: "held",
    version: 1,
    createdAtMs: 1_700_000_000_000,
    updatedAtMs: 1_700_000_000_000,
    finishedAtMs: null,
    currentDelivery: null,
    subtree: { total: 1, held: 1, queued: 0, dispatched: 0, finished: 0, cancelled: 0 },
    ...overrides,
  };
}

function workerRecord(overrides: Partial<WorkerRecord> = {}): WorkerRecord {
  return {
    workerId: "wkr_00000000000a",
    online: true,
    revoked: false,
    connectionEpoch: 4,
    lastSeenAtMs: Date.now(),
    activeDeliveries: 1,
    inventory: {
      revision: 2,
      observedAtMs: 1_700_000_000_000,
      host: { os: "darwin", arch: "arm64", deploymentMode: "native" },
      slots: [
        {
          slotId: "mac-default",
          enabled: true,
          // Unknown capacity: must render `—`, never `0`.
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

/** A full Task record, so `GET /v1/tasks/{id}` can satisfy a selection. */
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

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  window.localStorage.clear();
});

/* --------------------------------------------------------------------- */
/* 1. Fact honesty                                                        */
/* --------------------------------------------------------------------- */

describe("format.ts fact honesty", () => {
  it("labels a finished Task as Handling ended, never as a verdict", () => {
    const finished = TASK_STATUS_META.finished;
    expect(finished.label).toBe("Handling ended");
    expect(finished.label).not.toBe("Completed");
    expect(finished.label).not.toBe("Succeeded");
    expect(finished.terminal).toBe(true);
    expect(finished.description.toLowerCase()).toContain("not a success verdict");
  });

  it("renders unknown advisory capacity as an em dash, never zero", () => {
    expect(formatCapacity(null)).toBe("—");
    expect(formatCapacity(undefined)).toBe("—");
    expect(formatCapacity(2)).toBe("2");
  });

  it("keeps the ancestor chain visible when filtering on a descendant title", () => {
    const root = taskSummary({ id: "t1", title: "Prepare release" });
    const child = taskSummary({ id: "t2", title: "Run unit tests", parentTaskId: "t1", depth: 2 });
    const grandchild = taskSummary({
      id: "t3",
      title: "Run wasm tests",
      parentTaskId: "t2",
      depth: 3,
    });
    const unrelated = taskSummary({ id: "t4", title: "Unrelated root" });

    const unfiltered = buildTaskTreeRows([root, child, grandchild, unrelated], new Set());
    expect(unfiltered.map((row) => row.id)).toEqual(["t1", "t4"]);

    const filtered = buildTaskTreeRows(
      [root, child, grandchild, unrelated],
      new Set(),
      { query: "wasm" },
    );

    expect(filtered.map((row) => row.id)).toEqual(["t1", "t2", "t3"]);
    expect(filtered.map((row) => row.depth)).toEqual([0, 1, 2]);
    expect(filtered.map((row) => row.expanded)).toEqual([true, true, false]);
    expect(filtered[2]?.matchesFilter).toBe(true);
    expect(filtered[0]?.matchesFilter).toBe(false);
  });
});

/* --------------------------------------------------------------------- */
/* 2. Fetch-SSE transport                                                 */
/* --------------------------------------------------------------------- */

describe("HubClient.openEvents", () => {
  const noopHandlers = { onEvent: () => undefined };

  it("sends bearer and Last-Event-ID headers and parses streamed frames", async () => {
    let seenUrl = "";
    let seenInit: RequestInit = {};
    stubFetch((url, init) => {
      seenUrl = url;
      seenInit = init;
      return streamResponse([
        ": keepalive\n\n",
        'id: 7\ndata: {"seq":7,"kind":"task.created","createdAtMs":1,"payload":{"taskId":"t1"}}\n\n',
        'id: 8\ndata: {"seq":8,"kind":"task.updated","createdAtMs":2,"payload":null}\n\n',
      ]);
    });

    const client = new HubClient({ baseUrl: "https://hub.test", token: "test-token" });
    const events: HubEventEnvelope[] = [];
    const onOpen = vi.fn();

    const outcome = await client.openEvents(
      42,
      new AbortController().signal,
      { ...noopHandlers, onOpen, onEvent: (event) => events.push(event) },
    );

    expect(outcome).toBe("ended");
    expect(onOpen).toHaveBeenCalledTimes(1);
    expect(seenUrl).toBe("https://hub.test/v1/events");

    const headers = seenInit.headers as Record<string, string>;
    expect(headers.Authorization).toBe("Bearer test-token");
    expect(headers["Last-Event-ID"]).toBe("42");
    expect(headers.Accept).toBe("text/event-stream");

    expect(events.map((event) => event.seq)).toEqual([7, 8]);
    expect(events[0]?.kind).toBe("task.created");
  });

  it("reports an expired cursor on HTTP 410", async () => {
    stubFetch(() =>
      jsonResponse({ error: "CursorExpired", message: "Cursor fell outside retention." }, 410),
    );

    const onCursorExpired = vi.fn();
    const onUnauthorized = vi.fn();
    const client = new HubClient({ baseUrl: "", token: "test-token" });

    const outcome = await client.openEvents(null, new AbortController().signal, {
      ...noopHandlers,
      onCursorExpired,
      onUnauthorized,
    });

    expect(outcome).toBe("cursor_expired");
    expect(onCursorExpired).toHaveBeenCalledTimes(1);
    expect(onUnauthorized).not.toHaveBeenCalled();
  });

  it("reports an unauthorized session on HTTP 401", async () => {
    stubFetch(() => jsonResponse({ error: "Unauthorized", message: "Invalid token." }, 401));

    const onCursorExpired = vi.fn();
    const onUnauthorized = vi.fn();
    const client = new HubClient({ baseUrl: "", token: "bad-token" });

    const outcome = await client.openEvents(null, new AbortController().signal, {
      ...noopHandlers,
      onCursorExpired,
      onUnauthorized,
    });

    expect(outcome).toBe("unauthorized");
    expect(onUnauthorized).toHaveBeenCalledTimes(1);
    expect(onCursorExpired).not.toHaveBeenCalled();
  });
});

/* --------------------------------------------------------------------- */
/* 3. App end-to-end flow                                                 */
/* --------------------------------------------------------------------- */

function buildSnapshot(): GlobalSnapshot {
  const tasks: SnapshotTaskSummary[] = [
    taskSummary({
      id: "t1",
      title: "Ship release",
      status: "finished",
      version: 3,
      subtree: { total: 1, held: 0, queued: 0, dispatched: 0, finished: 1, cancelled: 0 },
    }),
    taskSummary({ id: "t2", title: "Draft notes", status: "held" }),
    taskSummary({
      id: "t3",
      title: "Run tests",
      status: "queued",
      dependencies: ["t2"],
      subtree: { total: 1, held: 0, queued: 1, dispatched: 0, finished: 0, cancelled: 0 },
    }),
    taskSummary({
      id: "t4",
      title: "Publish",
      status: "dispatched",
      subtree: { total: 1, held: 0, queued: 0, dispatched: 1, finished: 0, cancelled: 0 },
    }),
    taskSummary({
      id: "t5",
      title: "Old plan",
      status: "cancelled",
      subtree: { total: 1, held: 0, queued: 0, dispatched: 0, finished: 0, cancelled: 1 },
    }),
  ];

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

/** Find the Auth gate's token field without depending on one exact label. */
function findTokenInput(): HTMLInputElement {
  const labelled = [
    /admin bearer token/i,
    /bearer token/i,
    /admin token/i,
    /^token$/i,
    /token/i,
  ];
  for (const pattern of labelled) {
    const field = screen.queryByLabelText(pattern, { selector: "input" });
    if (field instanceof HTMLInputElement) {
      return field;
    }
  }
  const byPlaceholder = screen.queryAllByPlaceholderText(/token/i)[0];
  if (byPlaceholder instanceof HTMLInputElement) {
    return byPlaceholder;
  }
  const byType = document.querySelector<HTMLInputElement>('input[type="password"]');
  if (byType) {
    return byType;
  }
  const byAttribute = Array.from(document.querySelectorAll("input")).find((input) =>
    /token|secret|bearer/i.test(
      `${input.name} ${input.id} ${input.getAttribute("aria-label") ?? ""} ${input.autocomplete}`,
    ),
  );
  if (byAttribute) {
    return byAttribute;
  }
  throw new Error("The Auth gate did not render a token field.");
}

function submitAuthGate(): void {
  const byName = screen.queryByRole("button", { name: /connect|sign in|open|continue/i });
  if (byName) {
    fireEvent.click(byName);
    return;
  }
  const form = document.querySelector("form");
  if (!form) {
    throw new Error("The Auth gate did not render a form.");
  }
  fireEvent.submit(form);
}

/** The App Bar's centered Board | List switch — the only view navigation. */
function modeSwitch(): HTMLElement {
  return screen.getByRole("group", { name: /workspace mode/i });
}

/** Render the app, complete the Auth gate, and wait for the first Snapshot. */
async function connectWorkspace(): Promise<void> {
  render(<App />);
  expect(screen.queryByText("Handling ended")).toBeNull();
  const tokenField = findTokenInput();
  fireEvent.change(tokenField, { target: { value: "admin-token-value" } });
  submitAuthGate();
  await waitFor(() => {
    expect(document.querySelector(".app-bar")).not.toBeNull();
  });
}

function localStorageKeys(): string[] {
  return Array.from({ length: window.localStorage.length }, (_value, index) =>
    window.localStorage.key(index),
  ).filter((key): key is string => key !== null);
}

describe("App workspace flow", () => {
  beforeEach(() => {
    window.localStorage.clear();

    stubFetch((url) => {
      const path = url.replace(/^https?:\/\/[^/]+/, "");
      if (path.startsWith("/v1/events")) {
        return idleEventStream();
      }
      if (path.startsWith("/v1/snapshot")) {
        return jsonResponse(buildSnapshot());
      }
      if (path.startsWith("/v1/workers")) {
        return jsonResponse({ workers: [workerRecord()] });
      }
      if (path.startsWith("/v1/tasks/")) {
        const [pathname] = path.split("?");
        const match = /^\/v1\/tasks\/([^/]+)(\/tree)?$/.exec(pathname);
        const id = match === null ? "t1" : decodeURIComponent(match[1]);
        const isTree = match !== null && match[2] !== undefined;
        const summary = buildSnapshot().tasks.find((task) => task.id === id);
        if (summary === undefined) {
          return jsonResponse({ error: "NotFound", message: "Not found." }, 404);
        }
        if (isTree) {
          return jsonResponse({
            rootTaskId: summary.rootTaskId,
            tasks: [taskRecord(summary)],
            truncated: false,
          });
        }
        return jsonResponse(taskRecord(summary));
      }
      if (path.startsWith("/v1/tasks")) {
        return jsonResponse(taskRecord(buildSnapshot().tasks[0]));
      }
      return jsonResponse({ error: "NotFound", message: "Not found." }, 404);
    });
  });

  it("gates on auth first and keeps the bearer token out of storage", async () => {
    render(<App />);

    // 1. Auth gate first: no workspace and no fabricated Task facts yet.
    expect(screen.queryByText("Handling ended")).toBeNull();
    expect(document.querySelector(".app-frame")).toBeNull();

    const tokenField = findTokenInput();
    fireEvent.change(tokenField, { target: { value: "admin-token-value" } });
    submitAuthGate();

    // 2. Connected workspace renders the Work Atlas chrome.
    await waitFor(() => {
      expect(document.querySelector(".app-bar")).not.toBeNull();
    });
    expect(document.querySelector(".app-frame")).not.toBeNull();

    // 3. The bearer token is memory-only; only the theme may be persisted.
    const tokenKeys = localStorageKeys().filter((key) => /token|bearer|auth/i.test(key));
    expect(tokenKeys).toEqual([]);
    for (const value of localStorageKeys().map((key) => window.localStorage.getItem(key) ?? "")) {
      expect(value).not.toContain("admin-token-value");
    }
  });

  it("opens full-bleed on the Board with centered Board | List, no sidebar, no Events, Inspector collapsed", async () => {
    await connectWorkspace();

    // No left rail, no page tabs, no Events anywhere in the chrome.
    expect(document.querySelector(".sidebar")).toBeNull();
    expect(document.querySelector(".sidebar-rail")).toBeNull();
    expect(screen.queryByRole("navigation", { name: /workspace views/i })).toBeNull();
    expect(screen.queryByRole("button", { name: /^events/i })).toBeNull();
    expect(screen.queryByText(/event stream/i)).toBeNull();

    // The Board | List switch is centered in the App Bar and defaults to Board.
    const appBar = document.querySelector(".app-bar") as HTMLElement;
    const center = appBar.querySelector(".app-bar-center");
    expect(center).not.toBeNull();
    expect(center?.contains(modeSwitch())).toBe(true);
    expect(within(modeSwitch()).getByRole("button", { name: /^board$/i })).toHaveAttribute(
      "data-active",
      "true",
    );

    // Board is the home view, rendered full-bleed with a 42px toolbar.
    expect(document.querySelector('.view-body[data-view="board"]')).not.toBeNull();
    expect(document.querySelector(".board-toolbar")).not.toBeNull();

    // Choice A: the Inspector starts collapsed so the canvas gets 100% width.
    expect(document.querySelector(".inspector-slot")).toBeNull();
    const inspectorToggle = within(
      screen.getByRole("toolbar", { name: /board toolbar/i }),
    ).getByRole("button", { name: /inspector/i });
    expect(inspectorToggle).toHaveAttribute("aria-pressed", "false");

    // The status bar carries counters, totals, and hints — but no duplicate
    // live-stream or Workers readouts (those live in the App Bar now).
    const statusBar = document.querySelector(".status-bar") as HTMLElement;
    expect(statusBar.textContent ?? "").toContain("relations");
    expect(statusBar.textContent ?? "").not.toMatch(/live · event stream/i);
    expect(within(statusBar).queryByRole("button", { name: /^workers/i })).toBeNull();
    expect(statusBar.textContent ?? "").toContain("⌘K · I · ?");

    // Workers live in the App Bar as a drawer trigger, not a page.
    expect(
      within(appBar).getByRole("button", { name: /workers/i }),
    ).toHaveTextContent("1/1");
  });

  it("opens Task Detail on click and toggles the Workspace Inspector when nothing is selected", async () => {
    await connectWorkspace();

    const node = document.querySelector<HTMLElement>('.atlas-node[data-task-id="t3"]');
    expect(node).not.toBeNull();
    fireEvent.click(node as HTMLElement);

    await waitFor(() => {
      expect(document.querySelector(".detail-pane")).not.toBeNull();
    });
    expect(document.querySelector(".inspector-slot")).not.toBeNull();

    // Escape clears the selection and retracts the Inspector (full-bleed again).
    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() => {
      expect(document.querySelector(".inspector-slot")).toBeNull();
    });

    // Toggling Inspector with no selection shows the Workspace overview.
    const toolbar = screen.getByRole("toolbar", { name: /board toolbar/i });
    fireEvent.click(within(toolbar).getByRole("button", { name: /inspector/i }));
    await waitFor(() => {
      expect(document.querySelector(".inspector-pane")).not.toBeNull();
    });
    expect(document.querySelector(".detail-pane")).toBeNull();
  });

  it("switches Board and List without losing filters or selection, and never claims a verdict", async () => {
    await connectWorkspace();

    // A filter applied in Board mode must survive the mode switch.
    fireEvent.change(screen.getByLabelText("Filter by status"), {
      target: { value: "finished" },
    });
    const node = document.querySelector<HTMLElement>('.atlas-node[data-task-id="t1"]');
    expect(node).not.toBeNull();
    fireEvent.click(node as HTMLElement);
    await waitFor(() => {
      expect(document.querySelector(".detail-pane")).not.toBeNull();
    });

    fireEvent.click(within(modeSwitch()).getByRole("button", { name: /list/i }));
    await waitFor(() => {
      expect(document.querySelector('.view-body[data-view="list"]')).not.toBeNull();
    });
    expect(within(modeSwitch()).getByRole("button", { name: /^list$/i })).toHaveAttribute(
      "data-active",
      "true",
    );

    // Filters and selection are preserved; the Inspector is still open.
    expect((screen.getByLabelText("Filter by status") as HTMLSelectElement).value).toBe("finished");
    expect(document.querySelector(".inspector-slot")).not.toBeNull();
    expect(
      document.querySelector('.list-row[data-selected="true"]'),
    ).not.toBeNull();

    // Only the filtered (finished) Task is listed, labelled honestly.
    const listPane = document.querySelector(".task-pane") as HTMLElement;
    expect(listPane).toHaveTextContent("Handling ended");
    expect(listPane).not.toHaveTextContent("Completed");
    expect(listPane).not.toHaveTextContent("Succeeded");

    fireEvent.click(within(modeSwitch()).getByRole("button", { name: /^board$/i }));
    await waitFor(() => {
      expect(document.querySelector('.view-body[data-view="board"]')).not.toBeNull();
    });
    expect((screen.getByLabelText("Filter by status") as HTMLSelectElement).value).toBe("finished");
    expect(document.querySelector(".detail-pane")).not.toBeNull();
  });

  it("opens the Workers drawer over the canvas with unknown capacity as an em dash", async () => {
    await connectWorkspace();

    fireEvent.click(
      within(document.querySelector(".app-bar") as HTMLElement).getByRole("button", {
        name: /workers/i,
      }),
    );

    const drawer = await screen.findByRole("dialog", { name: /workers and pairing/i });
    expect(drawer).toHaveClass("workers-drawer-panel");
    expect(within(drawer).getByText(/1 of 1 online/i)).toBeInTheDocument();

    // The canvas stays mounted underneath: this is a drawer, not a page.
    expect(document.querySelector('.view-body[data-view="board"]')).not.toBeNull();

    const card = drawer.querySelector<HTMLElement>('[data-worker-id="wkr_00000000000a"]');
    expect(card).not.toBeNull();
    const slotRow = within(card as HTMLElement).getByText("mac-default").closest("tr");
    expect(slotRow).not.toBeNull();
    const slotRowElement = slotRow as HTMLElement;
    expect(within(slotRowElement).getByText("—")).toBeInTheDocument();
    expect(slotRowElement.textContent ?? "").not.toMatch(/capacity\s*0/i);
    expect(slotRowElement.textContent ?? "").not.toContain("0");

    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() => {
      expect(screen.queryByRole("dialog", { name: /workers and pairing/i })).toBeNull();
    });
  });

  it("persists only myrlet.theme when the Settings popover changes theme", async () => {
    await connectWorkspace();

    const settingsButton = screen.getByRole("button", { name: /settings and theme/i });
    expect(settingsButton).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(settingsButton);

    const menu = await screen.findByRole("menu", { name: /settings/i });
    expect(within(menu).getByText("THEME")).toBeInTheDocument();
    const graphite = within(menu).getByRole("menuitemradio", { name: /graphite/i });
    expect(graphite).toHaveAttribute("aria-checked", "false");
    fireEvent.click(graphite);

    await waitFor(() => {
      expect(window.localStorage.getItem("myrlet.theme")).toBe("graphite");
    });
    expect(document.documentElement.dataset.theme).toBe("graphite");

    expect(localStorageKeys()).toEqual(["myrlet.theme"]);
    expect(window.localStorage.getItem("myrlet.token")).toBeNull();
  });

  it("opens the Command Palette with Ctrl+K, lists Board/List, Workers & Pairing, and board actions, closing on Escape", async () => {
    await connectWorkspace();

    fireEvent.keyDown(document, { key: "k", ctrlKey: true });
    const palette = await screen.findByRole("dialog", { name: /command palette/i });
    expect(palette).toBeInTheDocument();
    expect(
      within(palette).getByPlaceholderText(/search tasks, views, actions, themes/i),
    ).toBeInTheDocument();

    expect(within(palette).getByRole("option", { name: /^board/i })).toBeInTheDocument();
    expect(within(palette).getByRole("option", { name: /^list/i })).toBeInTheDocument();
    expect(
      within(palette).getByRole("option", { name: /^workers & pairing/i }),
    ).toBeInTheDocument();
    expect(within(palette).getByRole("option", { name: /fit board to view/i })).toBeInTheDocument();
    expect(within(palette).getByRole("option", { name: /arrange board layout/i })).toBeInTheDocument();
    expect(within(palette).queryByRole("option", { name: /^events/i })).toBeNull();

    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() => {
      expect(screen.queryByRole("dialog", { name: /command palette/i })).toBeNull();
    });
  });

  it("opens the Workers drawer from the Command Palette action", async () => {
    await connectWorkspace();

    fireEvent.keyDown(document, { key: "k", ctrlKey: true });
    const palette = await screen.findByRole("dialog", { name: /command palette/i });
    fireEvent.click(within(palette).getByRole("option", { name: /^workers & pairing/i }));

    await waitFor(() => {
      expect(screen.getByRole("dialog", { name: /workers and pairing/i })).toBeInTheDocument();
    });
    expect(screen.queryByRole("dialog", { name: /command palette/i })).toBeNull();
  });

  it("filters the Board from the status bar counters", async () => {
    await connectWorkspace();

    const statusBar = document.querySelector(".status-bar") as HTMLElement;
    fireEvent.click(within(statusBar).getByRole("button", { name: /^queued\b/i }));

    await waitFor(() => {
      const statusSelect = screen.getByLabelText("Filter by status") as HTMLSelectElement;
      expect(statusSelect.value).toBe("queued");
    });
  });
});
