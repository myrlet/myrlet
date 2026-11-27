/**
 * Frontend fact-honesty, transport, and end-to-end Work Atlas tests.
 *
 * These tests assert the truthful wording and provenance rules the design
 * system depends on (never "Completed"/"Succeeded", unknown capacity is `—`,
 * tokens never reach storage) plus the final single-row chrome: one 52px App
 * Bar with a centered `Board | List` switch, System / Light / Dark appearance,
 * a relationship Board home view with a dense List alternative,
 * a strictly read-only Task surface, the Workers drawer, and no Events tab or
 * left sidebar rail.
 */

import "@testing-library/jest-dom/vitest";

import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
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

interface ColorSchemeController {
  setDark: (dark: boolean) => void;
}

/** A controllable `matchMedia`; color-scheme changes dispatch to real listeners. */
function stubColorScheme(dark: boolean, narrow = false): ColorSchemeController {
  let colorDark = dark;
  const listeners = new Set<(event: MediaQueryListEvent) => void>();
  const colorMedia = {
    get matches() {
      return colorDark;
    },
    media: "(prefers-color-scheme: dark)",
    onchange: null,
    addEventListener: (type: string, listener: EventListenerOrEventListenerObject | null) => {
      if (type === "change" && typeof listener === "function") {
        listeners.add(listener as (event: MediaQueryListEvent) => void);
      }
    },
    removeEventListener: (type: string, listener: EventListenerOrEventListenerObject | null) => {
      if (type === "change" && typeof listener === "function") {
        listeners.delete(listener as (event: MediaQueryListEvent) => void);
      }
    },
    addListener: () => undefined,
    removeListener: () => undefined,
    dispatchEvent: () => true,
  } as MediaQueryList;
  const narrowMedia = {
    matches: narrow,
    media: "(max-width: 1024px)",
    onchange: null,
    addEventListener: () => undefined,
    removeEventListener: () => undefined,
    addListener: () => undefined,
    removeListener: () => undefined,
    dispatchEvent: () => true,
  } as MediaQueryList;
  vi.stubGlobal(
    "matchMedia",
    vi.fn((query: string) =>
      query.includes("max-width") ? narrowMedia : colorMedia,
    ),
  );
  return {
    setDark(next) {
      colorDark = next;
      const event = {
        matches: next,
        media: colorMedia.media,
      } as MediaQueryListEvent;
      for (const listener of listeners) {
        listener(event);
      }
    },
  };
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

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  window.localStorage.clear();
  delete document.documentElement.dataset.theme;
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

function appBar(): HTMLElement {
  return document.querySelector(".app-bar") as HTMLElement;
}

function boardFloatingBar(): HTMLElement {
  return screen.getByRole("toolbar", { name: /board controls/i });
}

function openList(): void {
  fireEvent.click(within(modeSwitch()).getByRole("button", { name: /^list$/i }));
}

async function openBoard(): Promise<void> {
  fireEvent.click(within(modeSwitch()).getByRole("button", { name: /^board$/i }));
  await waitFor(() => {
    expect(document.querySelector('.view-body[data-view="board"]')).not.toBeNull();
  });
}

describe("App workspace flow", () => {
  beforeEach(() => {
    window.localStorage.clear();
    stubColorScheme(false);

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

    // 2. The gate offers System / Light / Dark as buttons — never a <select>.
    expect(document.querySelectorAll("select")).toHaveLength(0);
    const appearance = screen.getByRole("group", { name: /appearance/i });
    for (const label of ["System", "Light", "Dark"]) {
      expect(within(appearance).getByRole("button", { name: new RegExp(label, "i") })).toBeInTheDocument();
    }

    const tokenField = findTokenInput();
    fireEvent.change(tokenField, { target: { value: "admin-token-value" } });
    submitAuthGate();

    // 3. Connected workspace renders the Work Atlas chrome.
    await waitFor(() => {
      expect(document.querySelector(".app-bar")).not.toBeNull();
    });
    expect(document.querySelector(".app-frame")).not.toBeNull();

    // 4. The bearer token is memory-only; only the theme may be persisted, and
    //    an untouched System preference writes nothing at all.
    const tokenKeys = localStorageKeys().filter((key) => /token|bearer|auth/i.test(key));
    expect(tokenKeys).toEqual([]);
    for (const value of localStorageKeys().map((key) => window.localStorage.getItem(key) ?? "")) {
      expect(value).not.toContain("admin-token-value");
    }
    expect(window.localStorage.getItem("myrlet.theme")).toBeNull();
  });

  it("opens the Board by default with one 52px row, no toolbar, no sidebar, no Events", async () => {
    await connectWorkspace();

    // No left rail, no page tabs, no Events anywhere in the chrome.
    expect(document.querySelector(".sidebar")).toBeNull();
    expect(document.querySelector(".sidebar-rail")).toBeNull();
    expect(screen.queryByRole("navigation", { name: /workspace views/i })).toBeNull();
    expect(screen.queryByRole("button", { name: /^events/i })).toBeNull();
    expect(screen.queryByText(/event stream/i)).toBeNull();

    // Exactly one top row: the App Bar. The old 42px toolbar row is gone.
    expect(document.querySelector(".board-toolbar")).toBeNull();
    expect(screen.queryByRole("toolbar", { name: /board toolbar/i })).toBeNull();
    const frame = document.querySelector(".app-frame") as HTMLElement;
    expect(frame.firstElementChild).toHaveClass("app-bar");

    // No global search field survives in the chrome or the workspace views.
    expect(document.querySelector(".view-body [data-view-search]")).toBeNull();
    expect(within(appBar()).queryByRole("searchbox")).toBeNull();
    expect(document.querySelector(".toolbar-search")).toBeNull();

    // The Board | List switch is centered in the App Bar and defaults to Board.
    const center = appBar().querySelector(".app-bar-center");
    expect(center).not.toBeNull();
    expect(center?.contains(modeSwitch())).toBe(true);
    expect(within(modeSwitch()).getByRole("button", { name: /^board$/i })).toHaveAttribute(
      "data-active",
      "true",
    );

    // Board is the home view, rendered full-width with no Inspector.
    expect(document.querySelector('.view-body[data-view="board"]')).not.toBeNull();
    expect(document.querySelector(".atlas-board")).not.toBeNull();
    expect(document.querySelector(".inspector-slot")).toBeNull();

    openList();
    // Desktop rows: a sticky column header, title-first cells, status, progress,
    // and Worker aligned on one line, plus a copy-ID row action.
    const outline = document.querySelector(".list-outline") as HTMLElement;
    expect(outline).not.toBeNull();
    const labels = Array.from(outline.querySelectorAll(".list-columns .list-col-label")).map(
      (node) => (node.textContent ?? "").trim(),
    );
    expect(labels).toEqual(["Task", "Status", "Progress", "Worker", "Actions"]);
    expect(document.querySelectorAll(".list-row").length).toBe(5);
    expect(
      document.querySelector('.list-row[data-task-id="t1"] .list-action-btn'),
    ).not.toBeNull();
    expect(document.querySelector(".list-group-card")).toBeNull();

    // The status bar carries counters, totals, and hints — but no duplicate
    // live-stream or Workers readouts (those live in the App Bar now).
    const statusBar = document.querySelector(".status-bar") as HTMLElement;
    expect(statusBar.textContent ?? "").toContain("relations");
    expect(statusBar.textContent ?? "").not.toMatch(/live · event stream/i);
    expect(within(statusBar).queryByRole("button", { name: /^workers/i })).toBeNull();
    expect(statusBar.textContent ?? "").toContain("⌘K · I · ?");

    // Workers live in the App Bar as a drawer trigger, not a page.
    expect(
      within(appBar()).getByRole("button", { name: /workers/i }),
    ).toHaveTextContent("1/1");
  });

  it("resolves System and follows a same-mount prefers-color-scheme change", async () => {
    const colorScheme = stubColorScheme(true);
    render(<App />);

    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(window.localStorage.getItem("myrlet.theme")).toBeNull();

    act(() => colorScheme.setDark(false));
    await waitFor(() => {
      expect(document.documentElement.dataset.theme).toBe("light");
    });
    expect(window.localStorage.getItem("myrlet.theme")).toBeNull();
  });

  it("switches System | Light | Dark from Settings and persists only myrlet.theme", async () => {
    await connectWorkspace();

    // System is the default: no explicit choice has been persisted yet.
    expect(document.documentElement.dataset.theme).toBe("light");
    expect(window.localStorage.getItem("myrlet.theme")).toBeNull();

    const settingsButton = screen.getByRole("button", { name: /settings and theme/i });
    expect(settingsButton).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(settingsButton);

    const menu = await screen.findByRole("menu", { name: /settings/i });
    expect(within(menu).getByText("APPEARANCE")).toBeInTheDocument();
    const segment = within(menu).getByRole("group", { name: /appearance/i });
    const system = within(segment).getByRole("menuitemradio", { name: /system/i });
    const light = within(segment).getByRole("menuitemradio", { name: /light/i });
    const dark = within(segment).getByRole("menuitemradio", { name: /dark/i });
    expect(system).toHaveAttribute("aria-checked", "true");
    expect(light).toHaveAttribute("aria-checked", "false");
    expect(dark).toHaveAttribute("aria-checked", "false");

    fireEvent.click(dark);
    await waitFor(() => {
      expect(document.documentElement.dataset.theme).toBe("dark");
    });
    expect(window.localStorage.getItem("myrlet.theme")).toBe("dark");
    expect(dark).toHaveAttribute("aria-checked", "true");

    fireEvent.click(light);
    await waitFor(() => {
      expect(document.documentElement.dataset.theme).toBe("light");
    });
    expect(window.localStorage.getItem("myrlet.theme")).toBe("light");

    fireEvent.click(system);
    await waitFor(() => {
      expect(system).toHaveAttribute("aria-checked", "true");
    });
    expect(window.localStorage.getItem("myrlet.theme")).toBe("system");

    expect(localStorageKeys()).toEqual(["myrlet.theme"]);
    expect(window.localStorage.getItem("myrlet.token")).toBeNull();
  });

  it("offers no Task write entry points anywhere in the chrome, List, Board, or Detail", async () => {
    await connectWorkspace();
    openList();

    // Anchored so a Task whose status label reads "Dispatched" is not mistaken
    // for a Dispatch action.
    const writeLabels = [
      /^new task/i,
      /^import/i,
      /^dispatch\b/i,
      /^add subtask/i,
      /^subtask/i,
      /^cancel\b/i,
      /copy \/ retry/i,
    ];

    const assertNoWriteButton = (root: ParentNode, where: string): void => {
      for (const pattern of writeLabels) {
        expect(
          within(root as HTMLElement).queryByRole("button", { name: pattern }),
          `${where} must not offer ${pattern}`,
        ).toBeNull();
      }
    };

    const appBarElement = appBar();
    assertNoWriteButton(appBarElement, "the App Bar");
    expect(within(appBarElement).queryByText(/new task/i)).toBeNull();
    expect(within(appBarElement).queryByText(/import/i)).toBeNull();

    assertNoWriteButton(document.querySelector(".view-body") as HTMLElement, "the List");

    // Read-only Detail: selecting a Task never offers Dispatch / Cancel / Finish.
    fireEvent.click(document.querySelector('.list-row[data-task-id="t3"]') as HTMLElement);
    await waitFor(() => {
      expect(document.querySelector(".detail-pane")).not.toBeNull();
    });
    assertNoWriteButton(document.querySelector(".detail-pane") as HTMLElement, "Task Detail");
    expect(document.querySelector(".detail-pane")?.textContent ?? "").not.toMatch(
      /dispatch|cancel task|finish task/i,
    );

    await openBoard();
    assertNoWriteButton(document.querySelector(".view-body") as HTMLElement, "the Board");
    expect(
      document.querySelector('.view-body [aria-label="Dispatch Run tests"]'),
    ).toBeNull();
  });

  it("renders the immutable Markdown result safely in Task Detail", async () => {
    const result =
      "# Findings\n\n- one\n- two\n\n| Item | State |\n| --- | --- |\n| Build | ok |\n\n" +
      "```sh\ncargo test\n```\n\nSee [the PR](https://example.test/pr/1).\n\n" +
      '<script>alert("x")</script>\n\n![chart](https://example.test/chart.png)\n';
    // jsdom has no layout; give the preview a deterministic box model so this
    // long Result is measurably clipped and offers the full reader.
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (
      this: HTMLElement,
    ): DOMRect {
      if (this.classList.contains("result-markdown")) {
        return new DOMRect(0, 0, 480, this.children.length * 24);
      }
      if (this.parentElement?.classList.contains("result-markdown")) {
        const index = Array.from(this.parentElement.children).indexOf(this);
        return new DOMRect(0, index * 24, 480, 24);
      }
      return new DOMRect(0, 0, 0, 0);
    });
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
      const [pathname] = path.split("?");
      const match = /^\/v1\/tasks\/([^/]+)(\/tree)?$/.exec(pathname);
      const id = match === null ? "t1" : decodeURIComponent(match[1]);
      const summary = buildSnapshot().tasks.find((task) => task.id === id);
      if (summary === undefined) {
        return jsonResponse({ error: "NotFound", message: "Not found." }, 404);
      }
      if (match !== null && match[2] !== undefined) {
        return jsonResponse({
          rootTaskId: summary.rootTaskId,
          tasks: [taskRecord(summary)],
          truncated: false,
        });
      }
      return jsonResponse({
        ...taskRecord(summary),
        resultMarkdown: result,
        resultRecordedAtMs: 1_700_000_000_600,
      });
    });

    await connectWorkspace();
    fireEvent.click(document.querySelector('.atlas-node[data-task-id="t3"]') as HTMLElement);
    await waitFor(() => {
      expect(document.querySelector(".detail-pane .result-markdown")).not.toBeNull();
    });

    const pane = document.querySelector(".detail-pane") as HTMLElement;
    await waitFor(() => {
      expect(within(pane).getByRole("button", { name: /read full result/i })).toBeInTheDocument();
    });
    const markdown = pane.querySelector(".result-preview .result-markdown") as HTMLElement;
    expect(within(markdown).getByRole("heading", { name: "Findings" })).toBeInTheDocument();
    expect(markdown.querySelectorAll("li")).toHaveLength(2);
    expect(markdown.querySelector("table")).not.toBeNull();
    expect(pane.textContent ?? "").toMatch(/Synced|Recorded/);

    // The Overview clips this long result; the complete, sanitized document
    // lives in the reader behind `Read full result`.
    fireEvent.click(within(pane).getByRole("button", { name: /read full result/i }));
    const reader = await screen.findByRole("dialog");
    await waitFor(() => {
      expect(reader.querySelector(".result-markdown")).not.toBeNull();
    });
    const full = reader.querySelector(".result-markdown") as HTMLElement;
    expect(full.querySelector("pre code")?.textContent).toContain("cargo test");

    const link = within(full).getByRole("link", { name: /the pr/i });
    expect(link).toHaveAttribute("href", "https://example.test/pr/1");
    expect(link).toHaveAttribute("target", "_blank");
    expect(link.getAttribute("rel") ?? "").toMatch(/noopener/);

    // No raw HTML, no remotely loaded image: markdown shows the alt text instead.
    expect(full.querySelector("script")).toBeNull();
    expect(full.querySelector("img")).toBeNull();
    expect(full.textContent ?? "").not.toContain("alert(\"x\")");
    expect(full.textContent ?? "").toContain("[image: chart]");
  });

  it("never renders a native select in the gate, workspace, or Workers drawer", async () => {
    stubColorScheme(false);
    render(<App />);
    expect(document.querySelectorAll("select")).toHaveLength(0);

    const tokenField = findTokenInput();
    fireEvent.change(tokenField, { target: { value: "admin-token-value" } });
    submitAuthGate();
    await waitFor(() => {
      expect(document.querySelector(".app-bar")).not.toBeNull();
    });
    expect(document.querySelectorAll("select")).toHaveLength(0);

    fireEvent.click(
      within(appBar()).getByRole("button", { name: /workers/i }),
    );
    const drawer = await screen.findByRole("dialog", { name: /workers and pairing/i });
    expect(drawer.querySelectorAll("select")).toHaveLength(0);

    fireEvent.keyDown(document, { key: "Escape" });
    expect(document.querySelector('.view-body[data-view="board"]')).not.toBeNull();
    openList();
    expect(document.querySelector('.view-body[data-view="list"]')).not.toBeNull();
    expect(document.querySelectorAll("select")).toHaveLength(0);
  });

  it("docks Board controls in a real header row so they never cover group labels", async () => {
    await connectWorkspace();
    await openBoard();

    const dock = boardFloatingBar();
    expect(dock).toHaveClass("atlas-dock");
    expect(dock.closest(".atlas-stage")).toBeNull();
    expect(dock.closest(".atlas-board")).not.toBeNull();
    expect(within(dock).getByText(/relationships/i)).toBeInTheDocument();
    expect(within(dock).getByRole("button", { name: /fit/i })).toBeInTheDocument();
    expect(within(dock).getByRole("button", { name: /arrange/i })).toBeInTheDocument();
    expect(within(dock).getByRole("button", { name: /inspector/i })).toHaveAttribute(
      "aria-pressed",
      "false",
    );
    expect(within(dock).queryByRole("button", { name: /focus/i })).toBeNull();
  });

  it("reveals Board Focus only with a selection and keeps dimmed titles readable", async () => {
    await connectWorkspace();
    await openBoard();

    fireEvent.click(document.querySelector('.atlas-node[data-task-id="t3"]') as HTMLElement);
    await waitFor(() => {
      expect(document.querySelector(".detail-pane")).not.toBeNull();
    });
    const focused = boardFloatingBar();
    expect(within(focused).getByRole("button", { name: /focus/i })).toBeInTheDocument();
    expect(within(focused).getByRole("button", { name: /inspector/i })).toHaveAttribute(
      "aria-pressed",
      "true",
    );

    // Selection dims unrelated cards but their titles stay present and legible.
    const dimmed = document.querySelector('.atlas-node[data-dimmed="true"]');
    expect(dimmed).not.toBeNull();
    expect(dimmed?.querySelector(".atlas-node-title")?.textContent ?? "").not.toBe("");
  });

  it("opens Task Detail on click in the List and toggles the Workspace Inspector when nothing is selected", async () => {
    await connectWorkspace();
    openList();

    const row = document.querySelector<HTMLElement>('.list-row[data-task-id="t3"]');
    expect(row).not.toBeNull();
    fireEvent.click(row as HTMLElement);

    await waitFor(() => {
      expect(document.querySelector(".detail-pane")).not.toBeNull();
    });
    expect(document.querySelector(".inspector-slot")).not.toBeNull();

    // Escape clears the selection and retracts the Inspector (full-width again).
    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() => {
      expect(document.querySelector(".inspector-slot")).toBeNull();
    });

    // Toggling Inspector with no selection shows the Workspace overview.
    const listPane = document.querySelector(".task-pane") as HTMLElement;
    fireEvent.click(within(listPane).getByRole("button", { name: /inspector/i }));
    await waitFor(() => {
      expect(document.querySelector(".inspector-pane")).not.toBeNull();
    });
    expect(document.querySelector(".detail-pane")).toBeNull();
  });

  it("uses one sibling scrim, inert background, and focus return for both Inspector modes", async () => {
    stubColorScheme(false, true);
    await connectWorkspace();
    openList();

    const listPane = document.querySelector(".task-pane") as HTMLElement;
    const inspectorTrigger = within(listPane).getByRole("button", { name: /inspector/i });
    inspectorTrigger.focus();
    fireEvent.click(inspectorTrigger);
    await waitFor(() => {
      expect(document.querySelector(".inspector-pane")).not.toBeNull();
    });
    const viewBody = document.querySelector(".view-body") as HTMLElement;
    const slot = document.querySelector(".inspector-slot") as HTMLElement;
    const scrim = document.querySelector(".inspector-scrim") as HTMLElement;
    expect(scrim).not.toBeNull();
    // The scrim is a sibling behind the panel, never a wrapping child: the
    // panel's own close button must win the hit test.
    expect(scrim.parentElement).toBe(slot.parentElement);
    expect(scrim.contains(slot)).toBe(false);
    expect(viewBody).toHaveAttribute("inert");
    await waitFor(() => {
      expect(document.activeElement).toBe(slot);
    });

    // The panel's own close button is reachable and closes the Inspector.
    const closeButton = within(slot).getByRole("button", {
      name: /close workspace inspector/i,
    });
    closeButton.focus();
    expect(document.activeElement).toBe(closeButton);
    fireEvent.click(closeButton);
    await waitFor(() => {
      expect(document.querySelector(".inspector-slot")).toBeNull();
      expect(document.activeElement).toBe(inspectorTrigger);
    });
    expect(viewBody).not.toHaveAttribute("inert");

    const row = document.querySelector<HTMLElement>('.list-row[data-task-id="t3"]');
    expect(row).not.toBeNull();
    row?.focus();
    fireEvent.click(row as HTMLElement);
    await waitFor(() => {
      expect(document.querySelector(".detail-pane")).not.toBeNull();
    });
    expect(document.querySelector(".inspector-scrim")).not.toBeNull();
    expect(viewBody).toHaveAttribute("inert");
    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() => {
      expect(document.querySelector(".detail-pane")).toBeNull();
      expect(document.activeElement).toBe(row);
    });
  });

  it("switches List and Board without losing the status filter or selection, and never claims a verdict", async () => {
    await connectWorkspace();
    openList();

    // Filter from the status bar; the List narrows to the finished Task.
    const statusBar = document.querySelector(".status-bar") as HTMLElement;
    fireEvent.click(within(statusBar).getByRole("button", { name: /^handling ended\b/i }));

    await waitFor(() => {
      expect(document.querySelector('.list-row[data-task-id="t1"]')).not.toBeNull();
    });
    expect(document.querySelector('.list-row[data-task-id="t2"]')).toBeNull();
    const pill = document.querySelector(".active-filter-pill") as HTMLElement;
    expect(pill).not.toBeNull();
    expect(pill.textContent ?? "").toContain("Handling ended");

    fireEvent.click(document.querySelector('.list-row[data-task-id="t1"]') as HTMLElement);
    await waitFor(() => {
      expect(document.querySelector(".detail-pane")).not.toBeNull();
    });

    await openBoard();
    expect(within(modeSwitch()).getByRole("button", { name: /^board$/i })).toHaveAttribute(
      "data-active",
      "true",
    );

    // Filter and selection survive the switch; the Inspector stays open.
    expect(document.querySelector(".active-filter-pill")?.textContent ?? "").toContain(
      "Handling ended",
    );
    expect(document.querySelector(".inspector-slot")).not.toBeNull();
    expect(document.querySelector('.atlas-node[data-selected="true"]')).not.toBeNull();
    expect(document.querySelector('.atlas-node[data-task-id="t1"]')).not.toBeNull();
    expect(document.querySelector('.atlas-node[data-task-id="t2"]')).toBeNull();

    // The chip clears the filter without leaving the view.
    fireEvent.click(
      within(document.querySelector(".active-filter-pill") as HTMLElement).getByRole("button", {
        name: /clear status filter/i,
      }),
    );
    await waitFor(() => {
      expect(document.querySelector(".active-filter-pill")).toBeNull();
    });
    expect(document.querySelectorAll(".atlas-node").length).toBeGreaterThan(1);

    fireEvent.click(within(modeSwitch()).getByRole("button", { name: /^list$/i }));
    await waitFor(() => {
      expect(document.querySelector('.view-body[data-view="list"]')).not.toBeNull();
    });
    expect(document.querySelector(".detail-pane")).not.toBeNull();

    // The List stays truthful about the state it shows.
    const listPane = document.querySelector(".task-pane") as HTMLElement;
    expect(listPane).toHaveTextContent("Handling ended");
    expect(listPane).not.toHaveTextContent("Completed");
    expect(listPane).not.toHaveTextContent("Succeeded");
  });

  it("opens the Workers drawer over the List with unknown capacity as an em dash", async () => {
    await connectWorkspace();
    openList();

    fireEvent.click(
      within(appBar()).getByRole("button", {
        name: /workers/i,
      }),
    );

    const drawer = await screen.findByRole("dialog", { name: /workers and pairing/i });
    expect(drawer).toHaveClass("workers-drawer-panel");
    expect(within(drawer).getByText(/1 of 1 online/i)).toBeInTheDocument();

    // The workspace stays mounted underneath: this is a drawer, not a page.
    expect(document.querySelector('.view-body[data-view="list"]')).not.toBeNull();

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

  it("opens the Command Palette with Ctrl+K without Task write actions, and closes on Escape", async () => {
    await connectWorkspace();

    fireEvent.keyDown(document, { key: "k", ctrlKey: true });
    const palette = await screen.findByRole("dialog", { name: /command palette/i });
    expect(palette).toBeInTheDocument();
    expect(
      within(palette).getByPlaceholderText(/search tasks, views, actions, appearance/i),
    ).toBeInTheDocument();

    expect(within(palette).getByRole("option", { name: /^board/i })).toBeInTheDocument();
    expect(within(palette).getByRole("option", { name: /^list/i })).toBeInTheDocument();
    expect(
      within(palette).getByRole("option", { name: /^workers & pairing/i }),
    ).toBeInTheDocument();
    expect(
      within(palette).getByRole("option", { name: /toggle inspector/i }),
    ).toBeInTheDocument();
    expect(within(palette).getByRole("option", { name: /fit board to view/i })).toBeInTheDocument();
    expect(within(palette).getByRole("option", { name: /arrange board layout/i })).toBeInTheDocument();
    expect(within(palette).getByRole("option", { name: /^system/i })).toBeInTheDocument();
    expect(within(palette).getByRole("option", { name: /^light/i })).toBeInTheDocument();
    expect(within(palette).getByRole("option", { name: /^dark/i })).toBeInTheDocument();

    // The palette is strictly read-only: no create / import / dispatch.
    expect(within(palette).queryByRole("option", { name: /^new task/i })).toBeNull();
    expect(within(palette).queryByRole("option", { name: /^import/i })).toBeNull();
    expect(within(palette).queryByRole("option", { name: /^dispatch/i })).toBeNull();

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

  it("filters the List from the status bar counters with a dismissible pill", async () => {
    await connectWorkspace();
    openList();

    const statusBar = document.querySelector(".status-bar") as HTMLElement;
    const queued = within(statusBar).getByRole("button", { name: /^queued\b/i });
    fireEvent.click(queued);

    await waitFor(() => {
      expect(queued).toHaveAttribute("aria-pressed", "true");
    });
    const pill = document.querySelector(".active-filter-pill") as HTMLElement;
    expect(pill.textContent ?? "").toContain("Filtered: Queued");
    expect(document.querySelector('.list-row[data-task-id="t3"]')).not.toBeNull();
    expect(document.querySelector('.list-row[data-task-id="t2"]')).toBeNull();

    fireEvent.click(within(pill).getByRole("button", { name: /clear status filter/i }));
    await waitFor(() => {
      expect(document.querySelector(".active-filter-pill")).toBeNull();
    });
    expect(queued).toHaveAttribute("aria-pressed", "false");
    expect(document.querySelector('.list-row[data-task-id="t2"]')).not.toBeNull();
  });

  it("collapses the five status counters into one accessible filter menu on a phone", async () => {
    stubColorScheme(false, true);
    await connectWorkspace();
    openList();

    // No scattered counters and no native select: one trigger opens a menu.
    const statusBar = document.querySelector(".status-bar") as HTMLElement;
    expect(statusBar.querySelectorAll(".status-counter")).toHaveLength(0);
    expect(document.querySelectorAll("select")).toHaveLength(0);
    const trigger = within(statusBar).getByRole("button", {
      name: /filter by status/i,
    });
    expect(trigger).toHaveAttribute("aria-expanded", "false");
    expect(trigger).toHaveTextContent("All statuses");

    fireEvent.click(trigger);
    const menu = await screen.findByRole("menu", { name: /filter by status/i });
    expect(trigger).toHaveAttribute("aria-expanded", "true");
    expect(within(menu).getByRole("menuitemradio", { name: /all statuses/i })).toHaveAttribute(
      "aria-checked",
      "true",
    );
    const queuedOption = within(menu).getByRole("menuitemradio", { name: /queued/i });
    fireEvent.click(queuedOption);

    await waitFor(() => {
      expect(document.querySelector(".active-filter-pill")).not.toBeNull();
    });
    expect(screen.queryByRole("menu", { name: /filter by status/i })).toBeNull();
    expect(trigger).toHaveTextContent("Queued");
    expect(document.querySelector('.list-row[data-task-id="t3"]')).not.toBeNull();
    expect(document.querySelector('.list-row[data-task-id="t2"]')).toBeNull();

    // Escape peels the filter off the workspace without touching selection.
    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() => {
      expect(document.querySelector(".active-filter-pill")).toBeNull();
      expect(trigger).toHaveTextContent("All statuses");
    });
  });

  it("clears the active status filter before closing an open Inspector", async () => {
    await connectWorkspace();
    openList();

    const statusBar = document.querySelector(".status-bar") as HTMLElement;
    const queued = within(statusBar).getByRole("button", { name: /^queued\b/i });
    fireEvent.click(queued);
    await waitFor(() => {
      expect(queued).toHaveAttribute("aria-pressed", "true");
      expect(document.querySelector(".active-filter-pill")).not.toBeNull();
    });

    const listPane = document.querySelector(".task-pane") as HTMLElement;
    fireEvent.click(within(listPane).getByRole("button", { name: /inspector/i }));
    await waitFor(() => {
      expect(document.querySelector(".inspector-pane")).not.toBeNull();
    });

    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() => {
      expect(document.querySelector(".active-filter-pill")).toBeNull();
      expect(queued).toHaveAttribute("aria-pressed", "false");
    });
    expect(document.querySelector(".inspector-pane")).not.toBeNull();

    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() => {
      expect(document.querySelector(".inspector-slot")).toBeNull();
    });
  });

  it("keeps the snapshot Task name and status when the detail read fails, then retries or dismisses it", async () => {
    let detailAttempts = 0;
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
      const [pathname] = path.split("?");
      const match = /^\/v1\/tasks\/([^/]+)(\/tree)?$/.exec(pathname);
      const id = match === null ? "t3" : decodeURIComponent(match[1]);
      const summary = buildSnapshot().tasks.find((task) => task.id === id);
      if (summary === undefined) {
        return jsonResponse({ error: "NotFound", message: "Not found." }, 404);
      }
      if (match !== null && match[2] !== undefined) {
        return jsonResponse({
          rootTaskId: summary.rootTaskId,
          tasks: [taskRecord(summary)],
          truncated: false,
        });
      }
      detailAttempts += 1;
      if (detailAttempts < 3) {
        return jsonResponse(
          { error: "Internal", message: "The Hub returned status 500." },
          500,
        );
      }
      return jsonResponse(taskRecord(summary));
    });

    await connectWorkspace();
    openList();
    fireEvent.click(document.querySelector('.list-row[data-task-id="t3"]') as HTMLElement);

    const detailPane = (): HTMLElement =>
      document.querySelector(".detail-pane") as HTMLElement;

    // A failed detail read keeps the Snapshot name/status and states the failure
    // inside the panel instead of claiming that nothing is selected.
    await waitFor(() => {
      expect(detailPane().querySelector(".detail-error")).not.toBeNull();
    });
    expect(detailPane().querySelector(".detail-title")?.textContent).toBe("Run tests");
    expect(detailPane().textContent ?? "").toContain("Queued");
    expect(detailPane().textContent ?? "").not.toContain("No Task selected");
    // No fabricated detail from the Snapshot summary.
    expect(detailPane().textContent ?? "").not.toContain("Work on Run tests.");

    // Dismissing closes only the failure notice; the selection stays.
    fireEvent.click(
      within(detailPane()).getByRole("button", { name: /dismiss load failure/i }),
    );
    await waitFor(() => {
      expect(detailPane().querySelector(".detail-error")).toBeNull();
    });
    expect(detailPane().querySelector(".detail-title")?.textContent).toBe("Run tests");

    // Selecting the same Task again fails once more, then Retry reloads it.
    fireEvent.click(document.querySelector('.list-row[data-task-id="t3"]') as HTMLElement);
    await waitFor(() => {
      expect(detailPane().querySelector(".detail-error")).not.toBeNull();
    });
    fireEvent.click(within(detailPane()).getByRole("button", { name: /retry/i }));
    await waitFor(() => {
      expect(detailPane().querySelector(".detail-error")).toBeNull();
      expect(detailPane().textContent ?? "").toContain("Work on Run tests.");
    });
    expect(detailAttempts).toBe(3);
  });

  it("ignores a stale detail response after switching Tasks", async () => {
    let releaseT3: () => void = () => undefined;
    const t3Gate = new Promise<void>((resolve) => {
      releaseT3 = resolve;
    });

    stubFetch(async (url) => {
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
      const [pathname] = path.split("?");
      const match = /^\/v1\/tasks\/([^/]+)(\/tree)?$/.exec(pathname);
      const id = match === null ? "t1" : decodeURIComponent(match[1]);
      const summary = buildSnapshot().tasks.find((task) => task.id === id);
      if (summary === undefined) {
        return jsonResponse({ error: "NotFound", message: "Not found." }, 404);
      }
      if (match !== null && match[2] !== undefined) {
        return jsonResponse({
          rootTaskId: summary.rootTaskId,
          tasks: [taskRecord(summary)],
          truncated: false,
        });
      }
      if (id === "t3") {
        await t3Gate;
      }
      return jsonResponse(taskRecord(summary));
    });

    await connectWorkspace();
    openList();
    fireEvent.click(document.querySelector('.list-row[data-task-id="t3"]') as HTMLElement);

    // Loading is distinct from "nothing selected": the header already names the
    // pending Task from the Snapshot.
    await waitFor(() => {
      const pane = document.querySelector(".detail-pane") as HTMLElement;
      expect(pane.querySelector(".detail-title")?.textContent).toBe("Run tests");
      expect(pane.textContent ?? "").toContain("Loading");
    });
    expect(document.querySelector(".detail-pane")?.textContent ?? "").not.toContain(
      "No Task selected",
    );

    // Switch to another Task before the first response arrives.
    fireEvent.click(document.querySelector('.list-row[data-task-id="t1"]') as HTMLElement);
    await waitFor(() => {
      const pane = document.querySelector(".detail-pane") as HTMLElement;
      expect(pane.querySelector(".detail-title")?.textContent).toBe("Ship release");
      expect(pane.textContent ?? "").toContain("Work on Ship release.");
    });

    // The late response for the abandoned selection must not overwrite t1.
    releaseT3();
    await act(async () => {
      await t3Gate;
      await Promise.resolve();
    });
    const pane = document.querySelector(".detail-pane") as HTMLElement;
    expect(pane.querySelector(".detail-title")?.textContent).toBe("Ship release");
    expect(pane.textContent ?? "").toContain("Work on Ship release.");
    expect(pane.textContent ?? "").not.toContain("Work on Run tests.");
  });

  it("places a parent Task's subtask summary under its title without changing the card size", async () => {
    const snapshotTasks = [
      taskSummary({
        id: "p1",
        title: "Parent work",
        status: "dispatched",
        subtree: { total: 2, held: 0, queued: 0, dispatched: 2, finished: 0, cancelled: 0 },
      }),
      taskSummary({
        id: "c1",
        title: "Child work",
        parentTaskId: "p1",
        rootTaskId: "p1",
        depth: 2,
        status: "dispatched",
      }),
    ];
    stubFetch((url) => {
      const path = url.replace(/^https?:\/\/[^/]+/, "");
      if (path.startsWith("/v1/events")) {
        return idleEventStream();
      }
      if (path.startsWith("/v1/snapshot")) {
        return jsonResponse({ ...buildSnapshot(), totalTasks: 2, tasks: snapshotTasks });
      }
      if (path.startsWith("/v1/workers")) {
        return jsonResponse({ workers: [workerRecord()] });
      }
      return jsonResponse({ error: "NotFound", message: "Not found." }, 404);
    });

    await connectWorkspace();
    await openBoard();

    const card = document.querySelector('.atlas-node[data-task-id="p1"]') as HTMLElement;
    expect(card).not.toBeNull();
    const title = card.querySelector(".atlas-node-title") as HTMLElement;
    const summary = card.querySelector(".atlas-node-summary") as HTMLElement;
    expect(title.textContent).toBe("Parent work");
    expect(summary).not.toBeNull();
    // The full single-line summary sits directly below the title, not at the
    // bottom of the card.
    expect(title.nextElementSibling).toBe(summary);
    expect(summary.textContent ?? "").toContain("1 subtask");
    // Card geometry stays fixed: height comes from the layout constant.
    expect(card.style.height).toBe("140px");
  });

  it("lists the read-only shortcuts only", async () => {
    await connectWorkspace();

    fireEvent.keyDown(document, { key: "?" });
    const sheet = await screen.findByRole("dialog", { name: /keyboard shortcuts/i });
    const text = sheet.textContent ?? "";
    expect(text).toContain("Command Palette");
    expect(text).toContain("Fit the Board");
    expect(text).toContain("Show or hide the Inspector");
    expect(text).not.toMatch(/New Task/);
    expect(text).not.toMatch(/^Dispatch the selected Task/);
  });
});
