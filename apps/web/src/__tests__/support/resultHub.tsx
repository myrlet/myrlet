/**
 * Result fixtures and a controllable Hub stub for the Result suites.
 *
 * jsdom does no layout, so `stubPreviewLayout` installs a simple deterministic
 * box model: each top-level Markdown block is 24px tall.
 */

import "@testing-library/jest-dom/vitest";

import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { vi } from "vitest";

import App from "../../App";
import type {
  GlobalSnapshot,
  SnapshotTaskSummary,
  TaskRecord,
  WorkerRecord,
} from "../../api/types";

export const LONG_RESULT = [
  "# Findings",
  "",
  "The build is reproducible and the acceptance checks passed on the CI runner.",
  "",
  "- first finding",
  "- second finding",
  "- third finding",
  "",
  "| Check | State |",
  "| --- | --- |",
  "| Build | ok |",
  "| Tests | ok |",
  "",
  "```sh",
  "cargo test",
  "```",
  "",
  "See [the hidden PR link](https://example.test/pr/1) for the complete discussion.",
  "",
  "## Appendix",
  "",
  "Long trailing paragraph that must not appear in the Overview preview at all.",
].join("\n");

export const SHORT_RESULT =
  "# Summary\n\nAll checks passed.\n\n[CI run](https://example.test/run/9)";

const WORKER: WorkerRecord = {
  workerId: "wkr_00000000000a",
  online: true,
  revoked: false,
  approval: "approved",
  permissionTier: "worker",
  connectionEpoch: 4,
  lastSeenAtMs: 1_700_000_000_000,
  activeDeliveries: 1,
  host: { os: "darwin", arch: "arm64", deploymentMode: "native" },
  configurationPending: false,
};

function summary(): SnapshotTaskSummary {
  return {
    id: "t1",
    parentTaskId: null,
    rootTaskId: "t1",
    depth: 1,
    sourceTaskId: null,
    dependencies: [],
    blockedBy: [],
    category: "general",
    title: "Ship release",
    status: "finished",
    version: 3,
    createdAtMs: 1_700_000_000_000,
    updatedAtMs: 1_700_000_000_000,
    finishedAtMs: 1_700_000_000_500,
    currentDelivery: null,
    subtree: { total: 1, held: 0, queued: 0, dispatched: 0, finished: 1, cancelled: 0 },
  };
}

function snapshot(): GlobalSnapshot {
  return {
    schemaVersion: "myrlet.snapshot/v1",
    generatedAtMs: 1_700_000_000_500,
    eventCursor: 12,
    tasksTruncated: false,
    totalTasks: 1,
    workers: [WORKER],
    tasks: [summary()],
  };
}

function record(resultMarkdown: string | null): TaskRecord {
  return {
    id: "t1",
    rootTaskId: "t1",
    depth: 1,
    status: "finished",
    version: 3,
    createdAtMs: 1_700_000_000_000,
    updatedAtMs: 1_700_000_000_000,
    finishedAtMs: 1_700_000_000_500,
    currentDelivery: null,
    blockedBy: [],
    resultMarkdown,
    resultRecordedAtMs: resultMarkdown === null ? null : 1_700_000_000_600,
    subtree: { total: 1, held: 0, queued: 0, dispatched: 0, finished: 1, cancelled: 0 },
    task: {
      clientRef: null,
      parentTaskId: null,
      sourceTaskId: null,
      dependencies: [],
      category: "general",
      tags: [],
      title: "Ship release",
      prompt: "Ship the release.",
      input: null,
      expectedOutput: null,
      externalReferences: [],
      routing: {},
      execution: {},
      metadata: {},
    },
  };
}

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

interface EventStream {
  readonly response: Response;
  push: (seq: number) => void;
}

/** A live SSE stream we can push one invalidation hint into. */
function eventStream(): EventStream {
  const encoder = new TextEncoder();
  let controller: ReadableStreamDefaultController<Uint8Array> | null = null;
  const stream = new ReadableStream<Uint8Array>({
    start(next) {
      controller = next;
    },
  });
  return {
    response: new Response(stream, {
      status: 200,
      headers: { "Content-Type": "text/event-stream" },
    }),
    push(seq: number) {
      controller?.enqueue(
        encoder.encode(
          `id: ${seq}\ndata: {"seq":${seq},"kind":"task.updated","createdAtMs":1,"payload":{"taskId":"t1"}}\n\n`,
        ),
      );
    },
  };
}

interface Harness {
  readonly events: EventStream;
  /** Replace the Result the task detail endpoint returns. */
  setResult: (markdown: string | null) => void;
}

export function stubHub(initial: string | null): Harness {
  let current = initial;
  const events = eventStream();
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL) => {
      const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
      const path = url.replace(/^https?:\/\/[^/]+/, "");
      if (path.startsWith("/v1/events")) {
        return events.response;
      }
      if (path.startsWith("/v1/snapshot")) {
        return jsonResponse(snapshot());
      }
      if (path.startsWith("/v1/workers")) {
        return jsonResponse({ workers: [WORKER] });
      }
      const [pathname] = path.split("?");
      if (/^\/v1\/tasks\/[^/]+\/tree$/.test(pathname)) {
        return jsonResponse({
          rootTaskId: "t1",
          tasks: [record(current)],
          related: [],
          truncated: false,
        });
      }
      if (/^\/v1\/tasks\/[^/]+$/.test(pathname)) {
        return jsonResponse({ task: record(current), summary: null, related: [] });
      }
      return jsonResponse({ error: "NotFound", message: "Not found." }, 404);
    }),
  );
  return {
    events,
    setResult(markdown) {
      current = markdown;
    },
  };
}

export function stubClipboard(): ReturnType<typeof vi.fn> {
  const writeText = vi.fn(async () => undefined);
  vi.stubGlobal("navigator", { ...navigator, clipboard: { writeText } });
  return writeText;
}

/**
 * Deterministic layout for the preview: each top-level Markdown block is a
 * 24px-tall box, independent of its text, so the six-line (120px) budget keeps
 * exactly five blocks. jsdom has no layout at all, hence this stand-in.
 * Blocks are addressed by their position in the `.result-markdown` element.
 */
export function stubPreviewLayout(): void {
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (
    this: HTMLElement,
  ): DOMRect {
    const content = this.closest(".result-markdown") as HTMLElement | null;
    if (content !== null) {
      if (this === content) {
        return new DOMRect(0, 0, 480, content.children.length * 24);
      }
      const index = Array.from(content.children).indexOf(this);
      if (index >= 0) {
        return new DOMRect(0, index * 24, 480, 24);
      }
    }
    if (this.classList.contains("result-markdown")) {
      return new DOMRect(0, 0, 480, this.children.length * 24);
    }
    return new DOMRect(0, 0, 0, 0);
  });
}

export function detailBody(): HTMLElement {
  return document.querySelector(".detail-body") as HTMLElement;
}

/** Sign in, switch to the List, and select the finished Ship release Task. */
export async function connectAndSelectTask(): Promise<HTMLElement> {
  render(<App />);
  const tokenField =
    screen.queryByLabelText(/token/i, { selector: "input" }) ??
    document.querySelector<HTMLInputElement>('input[type="password"]');
  if (!(tokenField instanceof HTMLInputElement)) {
    throw new Error("The Auth gate did not render a token field.");
  }
  fireEvent.change(tokenField, { target: { value: "admin-token-value" } });
  const connect = screen.queryByRole("button", { name: /connect|sign in/i });
  if (connect) {
    fireEvent.click(connect);
  } else {
    fireEvent.submit(document.querySelector("form") as HTMLFormElement);
  }
  await waitFor(() => {
    expect(document.querySelector(".app-bar")).not.toBeNull();
  });
  // Let the workspace's passive effects register their document listeners
  // before the caller presses keys or clicks surfaces that depend on them.
  await act(async () => {});

  fireEvent.click(
    within(screen.getByRole("group", { name: /workspace mode/i })).getByRole("button", {
      name: /^list$/i,
    }),
  );

  const pane = await screen.findByRole("region", { name: /task tree/i });
  const rows = await within(pane).findAllByRole("row");
  const row = rows.find((candidate) => /ship release/i.test(candidate.textContent ?? ""));
  if (row === undefined) {
    throw new Error("The List did not render the Ship release Task row.");
  }
  fireEvent.click(row);
  await waitFor(() => {
    expect(document.querySelector(".detail-pane")).not.toBeNull();
  });
  return row;
}
