/**
 * Dedicated coverage for the split Task Detail views and the JSON viewer.
 *
 * Each tab is rendered on its own with narrow props, so a fact missing from one
 * view (a Delivery, a routing hint, a dependency the bounded read did not
 * return) is asserted in isolation rather than through the whole workspace.
 */

import "@testing-library/jest-dom/vitest";

import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { DeliveryView, TaskRecord, TaskTreeResponse } from "../../api/types";
import { JsonViewer } from "../../components/taskDetail/JsonViewer";
import { tokenizeJson } from "../../components/taskDetail/jsonTokens";
import { DeliveryTabView } from "../../components/taskDetail/DeliveryTabView";
import { JsonTabView } from "../../components/taskDetail/JsonTabView";
import { OverviewView } from "../../components/taskDetail/OverviewView";
import { TreeView } from "../../components/taskDetail/TreeView";

/* --------------------------------------------------------------------- */
/* Fixtures                                                               */
/* --------------------------------------------------------------------- */

function delivery(overrides: Partial<DeliveryView> = {}): DeliveryView {
  return {
    id: "dlv_000000000001",
    taskId: "leaf",
    workerId: "wkr_00000000000a",
    slotId: "slot-1",
    executorId: "exec-claude",
    connectionEpoch: 7,
    state: "accepted",
    cancelState: null,
    cancelReason: null,
    createdAtMs: 1_700_000_000_000,
    updatedAtMs: 1_700_000_000_100,
    progress: null,
    ...overrides,
  };
}

interface RecordOptions {
  readonly parentTaskId?: string | null;
  readonly depth?: number;
  readonly createdAtMs?: number;
  readonly title?: string;
  readonly currentDelivery?: DeliveryView | null;
  readonly resultMarkdown?: string | null;
}

function record(id: string, options: RecordOptions = {}): TaskRecord {
  return {
    id,
    rootTaskId: "root",
    depth: options.depth ?? 1,
    status: "queued",
    version: 4,
    createdAtMs: options.createdAtMs ?? 1_700_000_000_000,
    updatedAtMs: 1_700_000_000_100,
    finishedAtMs: null,
    currentDelivery: options.currentDelivery ?? null,
    blockedBy: [],
    resultMarkdown: options.resultMarkdown ?? null,
    resultRecordedAtMs: options.resultMarkdown == null ? null : 1_700_000_000_200,
    subtree: { total: 1, held: 0, queued: 1, dispatched: 0, finished: 0, cancelled: 0 },
    task: {
      clientRef: null,
      parentTaskId: options.parentTaskId ?? null,
      sourceTaskId: null,
      dependencies: [],
      category: "general",
      tags: [],
      title: options.title ?? id,
      prompt: `Work on ${id}.`,
      input: null,
      expectedOutput: null,
      externalReferences: [],
      routing: {},
      execution: {},
      metadata: {},
    },
  };
}

function tree(tasks: TaskRecord[], truncated = false): TaskTreeResponse {
  return { rootTaskId: "root", tasks, related: [], relatedTruncated: false, truncated };
}

const noop = (): void => undefined;

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

/* --------------------------------------------------------------------- */
/* Overview                                                               */
/* --------------------------------------------------------------------- */

describe("OverviewView", () => {
  it("states absent facts instead of guessing them", () => {
    render(
      <OverviewView task={record("leaf")} onCopyText={noop} onOpenReader={noop} />,
    );

    const workerCells = screen.getAllByText("— (no Delivery)");
    expect(workerCells).toHaveLength(1);
    expect(screen.getByText("Queued")).toBeInTheDocument();
    expect(screen.getByText("v4")).toBeInTheDocument();
    expect(screen.getByText("No routing constraints; any eligible Worker/Slot matches.")).toBeInTheDocument();
    expect(screen.getByText("No tags.")).toBeInTheDocument();
    // No Result and no Progress means neither section exists at all.
    expect(screen.queryByLabelText("Task result")).toBeNull();
    expect(screen.queryByLabelText("Latest Harness progress")).toBeNull();
    expect(document.querySelector(".result-section")).toBeNull();
    expect(screen.getByText("Work on leaf.")).toBeInTheDocument();
  });

  it("shows routing hints, tags, the assigned Worker, and the latest Progress", () => {
    const task = record("leaf", {
      currentDelivery: delivery({
        progress: {
          deliveryId: "dlv_000000000001",
          taskId: "leaf",
          text: "Halfway through the build.",
          observedAtMs: 1_700_000_000_100,
          receivedAtMs: 1_700_000_000_120,
          firstReceivedAtMs: 1_700_000_000_050,
        },
      }),
    });
    task.task.category = "release";
    task.task.tags = ["urgent", "ci"];
    task.task.routing = {
      requiredCapabilities: ["rust", "docker"],
      preferredWorkerId: "wkr_00000000000a",
      preferredRole: "builder",
      preferredExecutorId: "exec-claude",
    };

    render(
      <OverviewView task={task} onCopyText={noop} onOpenReader={noop} />,
    );

    expect(screen.getByText("release")).toBeInTheDocument();
    expect(
      within(screen.getByTitle("capability: rust")).getByText("rust"),
    ).toBeInTheDocument();
    expect(
      within(screen.getByTitle("worker: wkr_00000000000a")).getByText("wkr_00000000000a"),
    ).toBeInTheDocument();
    expect(within(screen.getByTitle("role: builder")).getByText("builder")).toBeInTheDocument();
    expect(
      within(screen.getByTitle("executor: exec-claude")).getByText("exec-claude"),
    ).toBeInTheDocument();
    expect(screen.getByText("urgent")).toBeInTheDocument();
    expect(screen.getByText("ci")).toBeInTheDocument();
    // The committed Delivery's Worker is the same fact the routing hint repeats.
    expect(screen.getAllByText("wkr_00000000000a")).toHaveLength(2);
    expect(screen.getByLabelText("Latest Harness progress")).toHaveTextContent(
      "Halfway through the build.",
    );
    expect(screen.queryByText("No tags.")).toBeNull();
  });

  it("copies the Prompt and the complete Result Markdown verbatim", () => {
    const onCopyText = vi.fn();
    const task = record("leaf", { resultMarkdown: "# Summary\n\nEverything ran." });

    render(
      <OverviewView task={task} onCopyText={onCopyText} onOpenReader={noop} />,
    );

    fireEvent.click(screen.getByTitle("Copy Prompt"));
    expect(onCopyText).toHaveBeenCalledWith("Prompt", "Work on leaf.");

    const result = screen.getByLabelText("Task result");
    fireEvent.click(within(result).getByRole("button", { name: /copy markdown/i }));
    expect(onCopyText).toHaveBeenCalledWith("Result", "# Summary\n\nEverything ran.");
  });

  it("opens the reader from the Result block only when the preview offers it", () => {
    const onOpenReader = vi.fn();
    // jsdom reports every box as 0px, so a short Result is never clipped.
    render(
      <OverviewView
        task={record("leaf", { resultMarkdown: "Short result." })}
        onCopyText={noop}
        onOpenReader={onOpenReader}
      />,
    );
    const result = screen.getByLabelText("Task result");
    expect(within(result).queryByRole("button", { name: /read full result/i })).toBeNull();
    expect(onOpenReader).not.toHaveBeenCalled();
  });
});

/* --------------------------------------------------------------------- */
/* Tree & Deps                                                            */
/* --------------------------------------------------------------------- */

describe("TreeView", () => {
  it("states a directly created Task and that related Tasks were not loaded", () => {
    render(<TreeView task={record("leaf")} tree={null} onSelectTask={noop} />);

    expect(
      screen.getByText("No source Task; this Task was created directly."),
    ).toBeInTheDocument();
    expect(screen.getByText("No sibling dependencies declared.")).toBeInTheDocument();
    // The empty read and the empty mini-tree each state it, as every tab does.
    expect(screen.getAllByText("Related Tasks were not loaded.")).toHaveLength(2);
  });

  it("resolves known dependencies, flags a missing parent, and reports truncation", () => {
    const leaf = record("leaf", { parentTaskId: "missing-parent" });
    leaf.task.dependencies = ["dep-known", "dep-missing"];
    const read = tree([record("root"), record("dep-known", { title: "Known" })], true);

    const onSelectTask = vi.fn();
    render(<TreeView task={leaf} tree={read} onSelectTask={onSelectTask} />);

    // The dependency card and the mini-tree both name the resolved Task.
    expect(screen.getAllByText("Known")).toHaveLength(2);
    expect(screen.getByText("Not in the loaded Task tree")).toBeInTheDocument();
    expect(
      screen.getByText("The related-Task tree read was truncated; some Tasks may be missing."),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByText("Not in the loaded Task tree"));
    expect(onSelectTask).toHaveBeenCalledWith("dep-missing");
  });

  it("opens a resolved dependency and hides the truncation notice when the read is complete", () => {
    const leaf = record("leaf");
    leaf.task.dependencies = ["dep-known"];
    const read = tree([record("root"), record("dep-known", { title: "Known" })]);

    const onSelectTask = vi.fn();
    render(<TreeView task={leaf} tree={read} onSelectTask={onSelectTask} />);

    expect(
      screen.queryByText(/tree read was truncated/i),
    ).toBeNull();
    fireEvent.click(screen.getAllByText("Known")[0]);
    expect(onSelectTask).toHaveBeenCalledWith("dep-known");
  });

  it("opens a source Task when the definition carries one", () => {
    const leaf = record("leaf");
    leaf.task.sourceTaskId = "source-1";
    const onSelectTask = vi.fn();
    render(<TreeView task={leaf} tree={null} onSelectTask={onSelectTask} />);

    fireEvent.click(screen.getByTitle("Open source Task source-1"));
    expect(onSelectTask).toHaveBeenCalledWith("source-1");
  });

  it("renders the Context Mini-Tree with the current Task marked and cycles listed once", () => {
    const tasks = [
      record("root", { depth: 1 }),
      record("a", { parentTaskId: "root", depth: 2, createdAtMs: 20 }),
      record("b", { parentTaskId: "root", depth: 2, createdAtMs: 10 }),
    ];
    const onSelectTask = vi.fn();
    render(<TreeView task={tasks[1]} tree={tree(tasks)} onSelectTask={onSelectTask} />);

    const nodes = document.querySelectorAll<HTMLElement>(".mini-tree-node");
    expect(Array.from(nodes).map((node) => node.title.split(" ")[0])).toEqual([
      "root",
      "b",
      "a",
    ]);
    const current = document.querySelector('.mini-tree-node[data-current="true"]');
    expect(current?.getAttribute("aria-current")).toBe("true");
    expect(current?.textContent).toContain("a");

    fireEvent.click(nodes[0]);
    expect(onSelectTask).toHaveBeenCalledWith("root");
  });

  it("lists every member of a parent cycle exactly once", () => {
    const read = tree([
      record("a", { parentTaskId: "b", depth: 2 }),
      record("b", { parentTaskId: "a", depth: 2 }),
    ]);
    render(<TreeView task={read.tasks[0]} tree={read} onSelectTask={noop} />);

    expect(document.querySelectorAll(".mini-tree-node")).toHaveLength(2);
  });
});

/* --------------------------------------------------------------------- */
/* Delivery                                                               */
/* --------------------------------------------------------------------- */

describe("DeliveryTabView", () => {
  it("states that no Delivery and no Progress exist", () => {
    render(<DeliveryTabView task={record("leaf")} />);
    expect(screen.getAllByText("No Delivery assigned yet.")).toHaveLength(2);
    expect(screen.queryByText(/Connection epoch/)).toBeNull();
  });

  it("states a Delivery without Progress", () => {
    render(<DeliveryTabView task={record("leaf", { currentDelivery: delivery() })} />);
    expect(screen.getAllByText("No Harness progress reported yet.")).toHaveLength(1);
    expect(screen.getByText("dlv_000000000001")).toBeInTheDocument();
    expect(screen.getByText("Claimed")).toBeInTheDocument();
    expect(screen.getByText("7")).toBeInTheDocument();
    // The executor is present, so no empty placeholder is rendered.
    expect(screen.getByText("exec-claude")).toBeInTheDocument();
    expect(screen.queryByText("—")).toBeNull();
  });

  it("keeps an uncertain cancellation and its reason visible", () => {
    render(
      <DeliveryTabView
        task={record("leaf", {
          currentDelivery: delivery({
            state: "cancelled",
            cancelState: "uncertain",
            cancelReason: "Worker disconnected mid-cancel.",
            executorId: null,
          }),
        })}
      />,
    );

    expect(screen.getByText("Cancel uncertain")).toBeInTheDocument();
    expect(screen.getByText("Worker disconnected mid-cancel.")).toBeInTheDocument();
    // An absent executor stays an explicit empty cell.
    expect(screen.getByText("—")).toBeInTheDocument();
  });

  it("relays the latest Progress with its execution-start and last-update times", () => {
    render(
      <DeliveryTabView
        task={record("leaf", {
          currentDelivery: delivery({
            progress: {
              deliveryId: "dlv_000000000001",
              taskId: "leaf",
              text: "Compiling workspace.",
              observedAtMs: 1_700_000_000_100,
              receivedAtMs: 1_700_000_360_000,
              firstReceivedAtMs: 1_700_000_000_050,
            },
          }),
        })}
      />,
    );

    expect(screen.getByText("Compiling workspace.")).toBeInTheDocument();
    expect(screen.getByText(/Execution started:/)).toBeInTheDocument();
    expect(screen.getByText(/Last updated:/)).toBeInTheDocument();
  });
});

/* --------------------------------------------------------------------- */
/* JSON                                                                   */
/* --------------------------------------------------------------------- */

describe("tokenizeJson", () => {
  it("classifies keys, strings, numbers, literals, and punctuation", () => {
    const tokens = tokenizeJson('{"id":"t1","n":-2.5e3,"ok":true,"x":null}');
    const kinds = tokens.filter((token) => token.kind !== "plain");
    expect(kinds.map((token) => token.text)).toEqual([
      "{",
      '"id"',
      ":",
      '"t1"',
      ",",
      '"n"',
      ":",
      "-2.5e3",
      ",",
      '"ok"',
      ":",
      "true",
      ",",
      '"x"',
      ":",
      "null",
      "}",
    ]);
    expect(
      tokens.find((token) => token.text === '"id"')?.kind,
    ).toBe("key");
    expect(
      tokens.find((token) => token.text === '"t1"')?.kind,
    ).toBe("string");
    expect(tokens.find((token) => token.text === "true")?.kind).toBe("literal");
  });

  it("keeps text the pattern does not recognize as plain", () => {
    expect(tokenizeJson("not json at all")).toEqual([
      { text: "not json at all", kind: "plain" },
    ]);
    expect(tokenizeJson("")).toEqual([]);
  });
});

describe("JsonViewer", () => {
  it("renders the serialized value with highlighted tokens", () => {
    render(<JsonViewer value={{ id: "t1", n: 2, ok: false }} />);
    const viewer = screen.getByLabelText("Task JSON");
    expect(viewer).toHaveClass("json-viewer");
    expect(viewer).toHaveAttribute("tabindex", "0");
    expect(viewer.querySelector(".json-key")?.textContent).toBe('"id"');
    expect(viewer.querySelector(".json-string")?.textContent).toBe('"t1"');
    expect(viewer.querySelector(".json-number")?.textContent).toBe("2");
    expect(viewer.querySelector(".json-literal")?.textContent).toBe("false");
    expect(viewer.textContent).toContain('"id"');
  });

  it("renders an absent value as null rather than crashing", () => {
    render(<JsonViewer value={undefined} />);
    expect(screen.getByLabelText("Task JSON").textContent).toBe("null");
  });
});

describe("JsonTabView", () => {
  it("copies the pretty-printed Task JSON and the CLI command", () => {
    const onCopyText = vi.fn();
    const task = record("leaf");
    render(<JsonTabView task={task} onCopyText={onCopyText} />);

    fireEvent.click(screen.getByTitle("Copy the full Task JSON"));
    expect(onCopyText).toHaveBeenCalledWith("Task JSON", JSON.stringify(task, null, 2));

    expect(screen.getByText("myrlet tasks show leaf")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Copy CLI command" }));
    expect(onCopyText).toHaveBeenCalledWith("CLI command", "myrlet tasks show leaf");
  });
});
