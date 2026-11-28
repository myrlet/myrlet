/**
 * Panel-level orchestration for Task Detail: the four tabs, the Overview reset
 * on a new selection, header actions, and the failure notice while a Task is
 * already loaded. The tab bodies have their own tests in `taskDetailViews`.
 */

import "@testing-library/jest-dom/vitest";

import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { SnapshotTaskSummary, TaskRecord, TaskTreeResponse } from "../../api/types";
import { TaskDetailPanel } from "../../components/TaskDetailPanel";
import type { TaskDetailPanelProps } from "../../components/TaskDetailPanel";

function record(id: string, title = id): TaskRecord {
  return {
    id,
    rootTaskId: id,
    depth: 1,
    status: "queued",
    version: 2,
    createdAtMs: 1_700_000_000_000,
    updatedAtMs: 1_700_000_000_100,
    finishedAtMs: null,
    currentDelivery: null,
    blockedBy: [],
    resultMarkdown: null,
    resultRecordedAtMs: null,
    subtree: { total: 1, held: 0, queued: 1, dispatched: 0, finished: 0, cancelled: 0 },
    task: {
      clientRef: null,
      parentTaskId: null,
      sourceTaskId: null,
      dependencies: [],
      category: "general",
      tags: [],
      title,
      prompt: `Work on ${title}.`,
      input: null,
      expectedOutput: null,
      externalReferences: [],
      routing: {},
      execution: {},
      metadata: {},
    },
  };
}

function summary(task: TaskRecord): SnapshotTaskSummary {
  return {
    id: task.id,
    parentTaskId: null,
    rootTaskId: task.rootTaskId,
    depth: task.depth,
    sourceTaskId: null,
    dependencies: [],
    blockedBy: [],
    category: task.task.category,
    title: task.task.title,
    status: task.status,
    version: task.version,
    createdAtMs: task.createdAtMs,
    updatedAtMs: task.updatedAtMs,
    finishedAtMs: task.finishedAtMs,
    currentDelivery: task.currentDelivery,
    subtree: task.subtree,
  };
}

function props(overrides: Partial<TaskDetailPanelProps> = {}): TaskDetailPanelProps {
  return {
    selectedTaskId: "leaf",
    summary: null,
    task: null,
    tree: null,
    loading: false,
    error: null,
    onClose: vi.fn(),
    onSelectTask: vi.fn(),
    onCopyText: vi.fn(),
    onRetry: vi.fn(),
    onDismissError: vi.fn(),
    ...overrides,
  };
}

function loaded(
  task: TaskRecord,
  overrides: Partial<TaskDetailPanelProps> = {},
): TaskDetailPanelProps {
  const read: TaskTreeResponse = { rootTaskId: task.rootTaskId, tasks: [task], truncated: false };
  return props({ selectedTaskId: task.id, task, tree: read, summary: summary(task), ...overrides });
}

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("TaskDetailPanel", () => {
  it("states that nothing is selected without a selected Task ID", () => {
    render(<TaskDetailPanel {...props({ selectedTaskId: null })} />);
    expect(screen.getByText("No Task selected")).toBeInTheDocument();
    expect(screen.queryByRole("tablist")).toBeNull();
  });

  it("keeps the Snapshot header while the full read is still loading", () => {
    const task = record("leaf", "Run tests");
    render(
      <TaskDetailPanel
        {...props({ summary: summary(task), loading: true })}
      />,
    );
    expect(screen.getByRole("heading", { name: "Run tests" })).toBeInTheDocument();
    // The header's loading marker and the body's placeholder both speak.
    expect(screen.getAllByRole("status").map((node) => node.textContent)).toEqual([
      "Loading…",
      "Loading Task…",
    ]);
    expect(screen.queryByText("No Task selected")).toBeNull();
    expect(screen.queryByRole("tablist")).toBeNull();
  });

  it("offers a load action when the read is idle and failed without a message", () => {
    const onRetry = vi.fn();
    render(
      <TaskDetailPanel
        {...props({ summary: summary(record("leaf", "Run tests")), onRetry })}
      />,
    );
    expect(screen.getByText("Details are not loaded.")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /^load task$/i }));
    expect(onRetry).toHaveBeenCalledTimes(1);
  });

  it("switches between the four tabs and exposes the selected panel", () => {
    const task = record("leaf", "Run tests");
    render(<TaskDetailPanel {...loaded(task)} />);

    const tabs = within(screen.getByRole("tablist", { name: /task detail sections/i }));
    expect(tabs.getAllByRole("tab")).toHaveLength(4);
    expect(screen.getByRole("tab", { name: "Overview" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(screen.getByRole("tabpanel")).toHaveAttribute(
      "aria-labelledby",
      "detail-tab-overview",
    );

    fireEvent.click(screen.getByRole("tab", { name: "Tree & Deps" }));
    expect(screen.getByRole("tab", { name: "Tree & Deps" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(screen.getByRole("heading", { name: "Source Task" })).toBeInTheDocument();
    expect(screen.getByRole("tabpanel")).toHaveAttribute("aria-labelledby", "detail-tab-tree");

    fireEvent.click(screen.getByRole("tab", { name: "Delivery" }));
    expect(screen.getByRole("heading", { name: "Current Delivery" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("tab", { name: "JSON" }));
    expect(screen.getByRole("heading", { name: "Task JSON" })).toBeInTheDocument();
    expect(screen.getByLabelText("Task JSON")).toBeInTheDocument();
  });

  it("returns to Overview when a different Task is selected", () => {
    const first = record("leaf", "Run tests");
    const { rerender } = render(<TaskDetailPanel {...loaded(first)} />);

    fireEvent.click(screen.getByRole("tab", { name: "JSON" }));
    expect(screen.getByRole("tab", { name: "JSON" })).toHaveAttribute("aria-selected", "true");

    const second = record("other", "Ship release");
    rerender(<TaskDetailPanel {...loaded(second)} />);
    expect(screen.getByRole("tab", { name: "Overview" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(screen.getByRole("heading", { name: "Ship release" })).toBeInTheDocument();
  });

  it("keeps the chosen tab when the same Task refreshes", () => {
    const task = record("leaf", "Run tests");
    const { rerender } = render(<TaskDetailPanel {...loaded(task)} />);

    fireEvent.click(screen.getByRole("tab", { name: "Delivery" }));
    rerender(<TaskDetailPanel {...loaded({ ...task, version: 5 })} />);
    expect(screen.getByRole("tab", { name: "Delivery" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });

  it("copies the Task ID, selects an ancestor, and closes from the header", () => {
    const parent = record("parent", "Parent work");
    const leaf = record("leaf", "Leaf work");
    leaf.task.parentTaskId = "parent";
    leaf.depth = 2;
    const tree: TaskTreeResponse = { rootTaskId: "parent", tasks: [parent, leaf], truncated: false };
    const onCopyText = vi.fn();
    const onSelectTask = vi.fn();
    const onClose = vi.fn();

    render(
      <TaskDetailPanel
        {...props({ task: leaf, tree, onCopyText, onSelectTask, onClose })}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Copy Task ID" }));
    expect(onCopyText).toHaveBeenCalledWith("Task ID", "leaf");

    fireEvent.click(screen.getByTitle("Open Parent work (parent)"));
    expect(onSelectTask).toHaveBeenCalledWith("parent");

    fireEvent.click(screen.getByRole("button", { name: "Close task detail" }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("reports an ancestor the bounded tree read did not contain as a root Task", () => {
    const leaf = record("leaf", "Leaf work");
    leaf.task.parentTaskId = "outside";
    render(
      <TaskDetailPanel
        {...props({ task: leaf, tree: { rootTaskId: "outside", tasks: [leaf], truncated: true } })}
      />,
    );
    // The breadcrumb states only relationships the read returned.
    expect(screen.getByText("Root Task")).toBeInTheDocument();
  });

  it("keeps loaded content under a failure notice and dismisses only the notice", () => {
    const task = record("leaf", "Leaf work");
    const onRetry = vi.fn();
    const onDismissError = vi.fn();
    const { rerender } = render(<TaskDetailPanel {...loaded(task)} />);

    rerender(
      <TaskDetailPanel
        {...loaded(task, {
          error: "The Hub returned status 500.",
          onRetry,
          onDismissError,
        })}
      />,
    );

    expect(screen.getByRole("alert", { name: /task load failed/i })).toBeInTheDocument();
    expect(screen.getByText("The Hub returned status 500.")).toBeInTheDocument();
    // The last known Task stays visible behind the notice.
    expect(screen.getByRole("heading", { name: "Leaf work" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Timestamps" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /^retry$/i }));
    expect(onRetry).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByRole("button", { name: "Dismiss load failure" }));
    expect(onDismissError).toHaveBeenCalledTimes(1);
  });

  it("marks a retry as busy so it cannot be fired twice", () => {
    const task = record("leaf");
    render(
      <TaskDetailPanel
        {...loaded(task, { error: "The Hub returned status 500.", loading: true })}
      />,
    );
    const retry = screen.getByRole("button", { name: /retrying/i });
    expect(retry).toBeDisabled();
    expect(retry).toHaveAttribute("aria-busy", "true");
  });
});
