/**
 * Regression tests for the Result reading flow (Issue #9).
 *
 * Covers: Result placement and provenance, a full short preview, a long
 * preview measured to ~six rendered lines where clipped content is inert,
 * `Copy Markdown` copying the original text, the native-dialog reader (the
 * desktop/mobile split is CSS), Escape closing only the reader, focus and
 * scroll restoration, and a late SSE arrival never changing tab, opening the
 * reader, or stealing focus.
 *
 * jsdom does no layout, so the tests install a simple deterministic box model:
 * each top-level Markdown block is 24px tall and each line is 20px.
 */

import "@testing-library/jest-dom/vitest";

import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import App from "../App";
import type {
  GlobalSnapshot,
  SnapshotTaskSummary,
  TaskRecord,
  WorkerRecord,
} from "../api/types";

/* --------------------------------------------------------------------- */
/* Fixtures                                                               */
/* --------------------------------------------------------------------- */

const LONG_RESULT = [
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

const SHORT_RESULT = "# Summary\n\nAll checks passed.\n\n[CI run](https://example.test/run/9)";

const WORKER: WorkerRecord = {
  workerId: "wkr_00000000000a",
  online: true,
  revoked: false,
  connectionEpoch: 4,
  lastSeenAtMs: 1_700_000_000_000,
  activeDeliveries: 1,
  inventory: {
    revision: 1,
    observedAtMs: 1_700_000_000_000,
    host: { os: "darwin", arch: "arm64", deploymentMode: "native" },
    slots: [],
  },
};

function summary(): SnapshotTaskSummary {
  return {
    id: "t1",
    parentTaskId: null,
    rootTaskId: "t1",
    depth: 1,
    sourceTaskId: null,
    dependencies: [],
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

function stubHub(initial: string | null): Harness {
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
        return jsonResponse({ rootTaskId: "t1", tasks: [record(current)], truncated: false });
      }
      if (/^\/v1\/tasks\/[^/]+$/.test(pathname)) {
        return jsonResponse(record(current));
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

function stubClipboard(): ReturnType<typeof vi.fn> {
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
function stubPreviewLayout(): void {
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

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  window.localStorage.clear();
});

/* --------------------------------------------------------------------- */
/* Navigation helpers                                                     */
/* --------------------------------------------------------------------- */

function detailBody(): HTMLElement {
  return document.querySelector(".detail-body") as HTMLElement;
}

async function connectAndSelectTask(): Promise<HTMLElement> {
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

/* --------------------------------------------------------------------- */
/* Tests                                                                  */
/* --------------------------------------------------------------------- */

describe("Task Result preview", () => {
  beforeEach(stubPreviewLayout);

  it("places Result after the Overview metadata and shows a short result in full", async () => {
    stubHub(SHORT_RESULT);
    await connectAndSelectTask();

    const pane = document.querySelector(".detail-pane") as HTMLElement;
    await waitFor(() => {
      expect(pane.querySelector(".result-preview .result-markdown")).not.toBeNull();
    });
    const meta = pane.querySelector(".meta-grid-2x2") as HTMLElement;
    const result = pane.querySelector(".result-section") as HTMLElement;
    expect(meta.compareDocumentPosition(result) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();

    expect(within(result).getByRole("heading", { name: "Result" })).toBeInTheDocument();
    expect(result.textContent ?? "").toContain("Synced");
    expect(result.textContent ?? "").toContain("All checks passed.");
    expect(within(result).getByRole("link", { name: /ci run/i })).toBeInTheDocument();

    // A short result needs no reader affordance at all.
    expect(within(result).queryByRole("button", { name: /read full result/i })).toBeNull();
  });

  it("measures a long result to about six lines and makes everything below inert", async () => {
    stubHub(LONG_RESULT);
    await connectAndSelectTask();

    const result = document.querySelector(".result-section") as HTMLElement;
    await waitFor(() => {
      expect(result.querySelector(".result-preview .result-markdown")).not.toBeNull();
      expect((result.querySelector(".result-preview-clip") as HTMLElement).style.maxHeight).toBe(
        "120px",
      );
    });
    const clip = result.querySelector(".result-preview-clip") as HTMLElement;
    expect(clip.style.maxHeight).toBe("120px");

    const preview = result.querySelector(".result-preview .result-markdown") as HTMLElement;
    // The whole document is rendered; only the tail is hidden and inert.
    expect(preview.textContent ?? "").toContain("Long trailing paragraph");
    const hiddenBlocks = Array.from(preview.children).filter(
      (child) => child.getAttribute("data-result-clipped") !== null,
    );
    expect(hiddenBlocks.length).toBeGreaterThan(0);
    for (const block of hiddenBlocks) {
      expect(block).toHaveAttribute("inert");
    }
    // The link in the hidden remainder is inert, so it cannot take focus.
    const hiddenLink = preview.querySelector("a") as HTMLElement;
    expect(hiddenLink.closest("[data-result-clipped]")).not.toBeNull();
    expect(hiddenLink.closest("[inert]")).not.toBeNull();

    const readFull = within(result).getByRole("button", { name: /read full result/i });
    expect(readFull).toBeInTheDocument();
  });

  it("copies the complete original Markdown from the Overview, not the preview", async () => {
    const writeText = stubClipboard();
    stubHub(LONG_RESULT);
    await connectAndSelectTask();

    const result = document.querySelector(".result-section") as HTMLElement;
    fireEvent.click(within(result).getByRole("button", { name: /copy markdown/i }));

    await waitFor(() => {
      expect(writeText).toHaveBeenCalledWith(LONG_RESULT);
    });
  });
});

describe("Result reader", () => {
  beforeEach(stubPreviewLayout);

  async function openReader(): Promise<{ dialog: HTMLElement; trigger: HTMLElement }> {
    const section = document.querySelector(".result-section") as HTMLElement;
    const trigger = await within(section).findByRole("button", { name: /read full result/i });
    fireEvent.click(trigger);
    const dialog = await screen.findByRole("dialog", { name: /result — ship release/i });
    return { dialog, trigger };
  }

  it("opens a native dialog with the task name and full Markdown, and copies the full text", async () => {
    const writeText = stubClipboard();
    stubHub(LONG_RESULT);
    await connectAndSelectTask();

    const dialog = (await openReader()).dialog;
    expect(dialog.tagName).toBe("DIALOG");
    expect(dialog).toHaveClass("result-reader");
    expect(dialog).toHaveAttribute("open");
    expect(within(dialog).getByRole("heading", { name: "Ship release" })).toBeInTheDocument();

    await waitFor(() => {
      expect(dialog.querySelector(".result-markdown")).not.toBeNull();
    });
    const full = dialog.querySelector(".result-markdown") as HTMLElement;
    expect(full.textContent ?? "").toContain("Long trailing paragraph");
    expect(within(full).getByRole("link", { name: /the hidden pr link/i })).toBeInTheDocument();
    // Nothing is clipped inside the reader.
    expect(full.querySelector("[inert]")).toBeNull();

    // Tables and fenced code scroll inside their own containers.
    expect(full.querySelector(".result-table-scroll table")).not.toBeNull();
    expect(full.querySelector("pre code")?.textContent).toContain("cargo test");

    fireEvent.click(within(dialog).getByRole("button", { name: /copy markdown/i }));
    await waitFor(() => {
      expect(writeText).toHaveBeenCalledWith(LONG_RESULT);
    });
  });

  it("Escape closes only the reader, then restores focus, tab, task, and Overview scroll", async () => {
    stubHub(LONG_RESULT);
    await connectAndSelectTask();

    const body = detailBody();
    body.scrollTop = 180;
    const { dialog, trigger } = await openReader();
    await waitFor(() => {
      expect(document.activeElement).toBe(
        within(dialog).getByRole("button", { name: /close result reader/i }),
      );
    });

    // Pretend layout shifted while the reader was open.
    body.scrollTop = 0;
    fireEvent.keyDown(dialog, { key: "Escape" });

    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    // Escape must not fall through and clear the Task selection.
    expect(document.querySelector(".detail-pane")).not.toBeNull();
    expect(screen.getByRole("tab", { name: "Overview" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    await waitFor(() => {
      expect(document.activeElement).toBe(trigger);
    });
    expect(body.scrollTop).toBe(180);
  });

  it("closes from its own close button and returns focus to the trigger", async () => {
    stubHub(LONG_RESULT);
    await connectAndSelectTask();

    const { dialog, trigger } = await openReader();
    fireEvent.click(within(dialog).getByRole("button", { name: /close result reader/i }));

    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    await waitFor(() => {
      expect(document.activeElement).toBe(trigger);
    });
  });

  it("keeps the reading position, tab, and focus when a late SSE frame adds a Result", async () => {
    const hub = stubHub(null);
    await connectAndSelectTask();

    // No Result yet: wait for the Task heading, not for a preview.
    expect(document.querySelector(".detail-pane .result-section")).toBeNull();
    const body = detailBody();
    body.scrollTop = 240;
    const overviewTab = screen.getByRole("tab", { name: "Overview" });
    overviewTab.focus();
    expect(document.activeElement).toBe(overviewTab);

    hub.setResult(LONG_RESULT);
    hub.events.push(13);

    await waitFor(() => {
      expect(document.querySelector(".detail-pane .result-section")).not.toBeNull();
    });

    // Late facts arrive in place: same tab, no reader, no focus or scroll change.
    expect(screen.getByRole("tab", { name: "Overview" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(document.activeElement).toBe(overviewTab);
    expect(detailBody().scrollTop).toBe(240);
  });
});
