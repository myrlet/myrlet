/**
 * Frontend fact-honesty, transport, and end-to-end workspace tests.
 *
 * These tests assert the truthful wording and provenance rules the design
 * system depends on (never "Completed"/"Succeeded", unknown capacity is `—`,
 * tokens never reach storage) rather than incidental markup.
 */

import "@testing-library/jest-dom/vitest";

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import App from "../App";
import { HubClient } from "../api/client";
import type {
  GlobalSnapshot,
  HubEventEnvelope,
  SnapshotTaskSummary,
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
  return {
    schemaVersion: "myrlet.snapshot/v1",
    generatedAtMs: 1_700_000_000_500,
    eventCursor: 12,
    tasksTruncated: false,
    totalTasks: 1,
    workers: [workerRecord()],
    tasks: [
      taskSummary({
        id: "t1",
        title: "Ship release",
        status: "finished",
        version: 3,
        subtree: { total: 1, held: 0, queued: 0, dispatched: 0, finished: 1, cancelled: 0 },
      }),
    ],
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

function clickButtonMatching(pattern: RegExp): void {
  const buttons = screen.getAllByRole("button");
  for (const button of buttons) {
    const label = `${button.textContent ?? ""} ${button.getAttribute("aria-label") ?? ""} ${
      button.getAttribute("title") ?? ""
    }`;
    if (pattern.test(label)) {
      fireEvent.click(button);
      return;
    }
  }
  throw new Error(`No button matched ${String(pattern)}.`);
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
      return jsonResponse({ error: "NotFound", message: "Not found." }, 404);
    });
  });

  it("gates on auth, keeps the token out of storage, and shows honest facts", async () => {
    render(<App />);

    // 1. Auth gate first: no workspace and no fabricated Task facts yet.
    expect(screen.queryByText("Handling ended")).toBeNull();
    const tokenField = findTokenInput();
    fireEvent.change(tokenField, { target: { value: "admin-token-value" } });
    submitAuthGate();

    // 2. Connected workspace renders the Task tree with truthful wording.
    expect(await screen.findByText("Ship release")).toBeInTheDocument();
    const taskTable = screen.getByRole("table");
    expect(taskTable).toHaveTextContent("Handling ended");

    // 3. The bearer token is memory-only; only the theme may be persisted.
    const tokenKeys = Array.from({ length: window.localStorage.length }, (_value, index) =>
      window.localStorage.key(index),
    ).filter((key): key is string => key !== null);
    expect(tokenKeys.filter((key) => /token|bearer|auth/i.test(key))).toEqual([]);
    for (const value of tokenKeys.map((key) => window.localStorage.getItem(key) ?? "")) {
      expect(value).not.toContain("admin-token-value");
    }

    // 4. Toggling the theme persists the preference and nothing else.
    clickButtonMatching(/theme|porcelain|graphite|dark|light/i);
    await waitFor(() => {
      expect(window.localStorage.getItem("myrlet.theme")).toMatch(/porcelain|graphite/);
    });
    const storedKeys = Array.from({ length: window.localStorage.length }, (_value, index) =>
      window.localStorage.key(index),
    ).filter((key): key is string => key !== null);
    expect(storedKeys).toEqual(["myrlet.theme"]);
    expect(window.localStorage.getItem("myrlet.token")).toBeNull();

    // 5. Workers drawer: unknown advisory capacity renders as `—`, never `0`.
    clickButtonMatching(/workers/i);
    const workersDialog = await screen.findByRole("dialog", { name: /workers/i });
    expect(workersDialog).toBeInTheDocument();
    expect(screen.getAllByText("wkr_00000000000a").length).toBeGreaterThan(0);

    const capacityLabels = screen
      .getAllByText(/capacity/i)
      .map((element) => element.textContent ?? "")
      .join(" | ");
    expect(capacityLabels).toContain("—");
    expect(capacityLabels).not.toMatch(/capacity\s*0\b/);

    // 6. Escape closes the drawer without touching the session.
    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() => {
      expect(screen.queryByRole("dialog", { name: /workers/i })).toBeNull();
    });
    expect(screen.getByText("Ship release")).toBeInTheDocument();
  });
});
