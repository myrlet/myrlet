/**
 * Work Atlas Board behaviour: card click, canvas pan, card drag with persisted
 * offsets, Arrange / Restore, framing controls, and unobtainable storage.
 *
 * These tests mount `TaskGraphView` directly so the Board's own state —
 * transform, offsets, focus — is asserted without the rest of the workspace.
 */

import "@testing-library/jest-dom/vitest";

import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { SnapshotTaskSummary, WorkerRecord } from "../api/types";
import { TaskGraphView } from "../components/TaskGraphView";
import type { AtlasBoardMeta, TaskGraphControls } from "../components/TaskGraphView";
import { DEFAULT_ATLAS_STORAGE_KEY } from "../components/atlas/atlasOffsets";
import { NODE_HEIGHT, NODE_WIDTH } from "../components/workAtlasLayout";

/* --------------------------------------------------------------------- */
/* Fixtures                                                               */
/* --------------------------------------------------------------------- */

const CREATED_MS = 1_700_000_000_000;

function task(overrides: Partial<SnapshotTaskSummary> & Pick<SnapshotTaskSummary, "id" | "title">): SnapshotTaskSummary {
  const { id, title, ...rest } = overrides;
  return {
    id,
    title,
    parentTaskId: null,
    rootTaskId: id,
    depth: 1,
    sourceTaskId: null,
    dependencies: [],
    blockedBy: [],
    category: "general",
    status: "held",
    version: 1,
    createdAtMs: CREATED_MS,
    updatedAtMs: CREATED_MS,
    finishedAtMs: null,
    currentDelivery: null,
    subtree: { total: 1, held: 1, queued: 0, dispatched: 0, finished: 0, cancelled: 0 },
    ...rest,
  };
}

const ROOT = task({ id: "root", title: "Root task" });
const CHILD = task({
  id: "child",
  title: "Child task",
  parentTaskId: "root",
  rootTaskId: "root",
  depth: 2,
  subtree: { total: 1, held: 1, queued: 0, dispatched: 0, finished: 0, cancelled: 0 },
});
/** An unrelated standalone root, so selection has something to dim. */
const OTHER = task({ id: "other", title: "Other task", createdAtMs: CREATED_MS + 1 });
const TASKS = [ROOT, CHILD] as const;
const WORKERS: readonly WorkerRecord[] = [];

/**
 * The deterministic layout for ROOT → CHILD:
 * the parent sits at x 0, the child at x 296, both 34px below the group label.
 */
const ROOT_BASE = { x: 0, y: 34 };
const CHILD_BASE = { x: 296, y: 34 };
const VIEWPORT_WIDTH = 800;
const VIEWPORT_HEIGHT = 600;

interface Mounted {
  readonly controls: { current: TaskGraphControls | null };
  readonly meta: AtlasBoardMeta[];
  readonly selected: string[];
  readonly cleared: number;
}

function mountBoard(options: {
  tasks?: readonly SnapshotTaskSummary[];
  selectedTaskId?: string | null;
  storageKey?: string;
  onSelectTask?: (taskId: string) => void;
  onClearSelection?: () => void;
} = {}): Mounted {
  const controls: { current: TaskGraphControls | null } = { current: null };
  const meta: AtlasBoardMeta[] = [];
  const selected: string[] = [];
  let cleared = 0;
  render(
    <TaskGraphView
      tasks={options.tasks ?? TASKS}
      workers={WORKERS}
      selectedTaskId={options.selectedTaskId ?? null}
      inspectorOpen={false}
      activeStatusLabel={null}
      onToggleInspector={() => undefined}
      onClearFilter={() => undefined}
      onSelectTask={(taskId) => {
        selected.push(taskId);
        options.onSelectTask?.(taskId);
      }}
      onClearSelection={() => {
        cleared += 1;
        options.onClearSelection?.();
      }}
      onBoardMetaChange={(next) => meta.push(next)}
      controlsRef={controls}
      storageKey={options.storageKey ?? DEFAULT_ATLAS_STORAGE_KEY}
    />,
  );
  return {
    controls,
    meta,
    selected,
    get cleared() {
      return cleared;
    },
  };
}

const stage = (): HTMLElement => screen.getByLabelText("Task graph canvas");
const card = (id: string): HTMLElement =>
  document.querySelector(`.atlas-node[data-task-id="${id}"]`) as HTMLElement;
const world = (): HTMLElement => document.querySelector(".atlas-world") as HTMLElement;
const dockButton = (name: RegExp): HTMLElement =>
  screen.getByRole("button", { name });

/** A jsdom `PointerEvent`; jsdom ships none, and React reads `clientX` off it. */
class TestPointerEvent extends MouseEvent {
  readonly pointerId: number;
  readonly pointerType = "mouse";
  readonly isPrimary = true;
  constructor(type: string, init: PointerEventInit = {}) {
    super(type, { bubbles: true, cancelable: true, ...init });
    this.pointerId = init.pointerId ?? 1;
  }
}

const pointer = (type: string, clientX: number, clientY: number, button = 0): MouseEvent =>
  new TestPointerEvent(type, { clientX, clientY, button });

/** Dispatch a real pointer event instance, so `clientX` reaches the handler. */
const press = (target: Element, event: MouseEvent): void => {
  fireEvent(target, event);
};

function storedOffsets(storageKey = DEFAULT_ATLAS_STORAGE_KEY): unknown {
  const raw = window.localStorage.getItem(storageKey);
  return raw === null ? null : JSON.parse(raw);
}

beforeEach(() => {
  window.localStorage.clear();
  vi.stubGlobal("PointerEvent", TestPointerEvent);
  /* jsdom has no layout and no pointer capture; both are stubbed so the Board's
     own logic — not the environment — is what these tests exercise. */
  Object.defineProperty(HTMLElement.prototype, "clientWidth", {
    configurable: true,
    get: () => VIEWPORT_WIDTH,
  });
  Object.defineProperty(HTMLElement.prototype, "clientHeight", {
    configurable: true,
    get: () => VIEWPORT_HEIGHT,
  });
  Object.assign(HTMLElement.prototype, {
    setPointerCapture: function setPointerCapture(): void {
      /* No-op: gesture state lives in the hook, not the DOM. */
    },
    releasePointerCapture: function releasePointerCapture(): void {
      /* No-op. */
    },
    hasPointerCapture: function hasPointerCapture(): boolean {
      return true;
    },
  });
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  window.localStorage.clear();
  delete (HTMLElement.prototype as unknown as { clientWidth?: number }).clientWidth;
  delete (HTMLElement.prototype as unknown as { clientHeight?: number }).clientHeight;
  delete (HTMLElement.prototype as unknown as { setPointerCapture?: unknown }).setPointerCapture;
  delete (HTMLElement.prototype as unknown as { releasePointerCapture?: unknown }).releasePointerCapture;
  delete (HTMLElement.prototype as unknown as { hasPointerCapture?: unknown }).hasPointerCapture;
});

/* --------------------------------------------------------------------- */
/* Rendering and selection                                                */
/* --------------------------------------------------------------------- */

describe("Board canvas rendering", () => {
  it("renders one card per Task plus the real Parent edge and group label", () => {
    mountBoard();

    expect(document.querySelectorAll(".atlas-node")).toHaveLength(2);
    expect(card("root")).toHaveAttribute("style", expect.stringContaining(`left: ${ROOT_BASE.x}px`));
    expect(card("child").style.left).toBe(`${CHILD_BASE.x}px`);
    expect(card("child").style.top).toBe(`${CHILD_BASE.y}px`);
    expect(card("root").style.width).toBe(`${NODE_WIDTH}px`);
    expect(card("root").style.height).toBe(`${NODE_HEIGHT}px`);

    const edges = document.querySelectorAll(".atlas-edge");
    expect(edges).toHaveLength(1);
    expect(edges[0]).toHaveAttribute("data-kind", "parent");

    const labels = document.querySelectorAll(".atlas-group-header");
    expect(labels).toHaveLength(1);
    expect(labels[0]?.textContent).toContain("Root task");
  });

  it("selects a card by click and by keyboard, and clears the selection on an empty canvas click", () => {
    const board = mountBoard({ selectedTaskId: null });

    fireEvent.click(card("child"));
    expect(board.selected).toEqual(["child"]);

    fireEvent.keyDown(card("root"), { key: "Enter" });
    fireEvent.keyDown(card("root"), { key: " " });
    expect(board.selected).toEqual(["child", "root", "root"]);

    press(stage(), pointer("pointerdown", 500, 500));
    press(stage(), pointer("pointerup", 500, 500));
    expect(board.cleared).toBe(1);
  });

  it("dims unrelated cards around the selection but keeps their titles", () => {
    mountBoard({ tasks: [ROOT, CHILD, OTHER], selectedTaskId: "child" });
    // The ancestor stays part of the highlighted relationship path.
    expect(card("root")).not.toHaveAttribute("data-dimmed");
    expect(card("child")).not.toHaveAttribute("data-dimmed");
    expect(card("other")).toHaveAttribute("data-dimmed", "true");
    expect(card("other").querySelector(".atlas-node-title")?.textContent).toBe("Other task");
  });

  it("shows the empty board states and clears an active filter", () => {
    const clearFilter = vi.fn();
    const controls: { current: TaskGraphControls | null } = { current: null };
    render(
      <TaskGraphView
        tasks={[]}
        workers={[]}
        selectedTaskId={null}
        inspectorOpen={false}
        activeStatusLabel="Queued"
        onToggleInspector={() => undefined}
        onClearFilter={clearFilter}
        onSelectTask={() => undefined}
        onClearSelection={() => undefined}
        onBoardMetaChange={() => undefined}
        controlsRef={controls}
      />,
    );

    expect(screen.getByText("No Tasks match this status")).toBeInTheDocument();
    expect(document.querySelectorAll(".atlas-node")).toHaveLength(0);
    fireEvent.click(screen.getByRole("button", { name: /clear filter/i }));
    expect(clearFilter).toHaveBeenCalledTimes(1);
  });
});

/* --------------------------------------------------------------------- */
/* Pan, drag, and persisted offsets                                      */
/* --------------------------------------------------------------------- */

describe("Board gestures and stored offsets", () => {
  it("pans the canvas by dragging empty space and ignores sub-threshold presses", () => {
    mountBoard();
    const before = world().style.transform;

    press(stage(), pointer("pointerdown", 500, 500));
    press(stage(), pointer("pointermove", 503, 502));
    expect(world().style.transform).toBe(before);

    press(stage(), pointer("pointermove", 530, 520));
    expect(world().style.transform).not.toBe(before);
    expect(world().style.transform).toContain("scale(1)");
    press(stage(), pointer("pointerup", 530, 520));
  });

  it("drags a card, doubles the gesture in world space, and persists the snapped offset once", () => {
    mountBoard();
    const before = world().style.transform;

    press(card("root"), pointer("pointerdown", 100, 100));
    press(card("root"), pointer("pointermove", 160, 140));

    // The card follows the pointer while the camera stays put.
    expect(world().style.transform).toBe(before);
    expect(card("root").style.left).toBe(`${ROOT_BASE.x + 60}px`);
    expect(card("root").style.top).toBe(`${ROOT_BASE.y + 40}px`);
    expect(card("root")).toHaveAttribute("data-dragging", "true");
    // Nothing is written mid-drag.
    expect(storedOffsets()).toBeNull();

    press(card("root"), pointer("pointerup", 160, 140));
    expect(card("root")).toHaveAttribute("data-dragging", "false");
    // Settled to the 12px grid measured from the layout position: (60, 74) → (60, 72).
    expect(storedOffsets()).toEqual({ root: { x: 60, y: 38 } });
    expect(card("root").style.left).toBe("60px");
    expect(card("root").style.top).toBe("72px");
  });

  it("scales a drag by the current zoom so the card tracks the pointer", () => {
    mountBoard();
    act(() => {
      (screen.getByRole("button", { name: /zoom in/i }) as HTMLElement).click();
    });
    const scale = Number(/scale\(([\d.]+)\)/.exec(world().style.transform)?.[1]);
    expect(scale).toBeGreaterThan(1);

    press(card("root"), pointer("pointerdown", 100, 100));
    press(card("root"), pointer("pointermove", 150, 100));
    press(card("root"), pointer("pointerup", 150, 100));

    const offset = 50 / scale;
    expect(card("root").style.left).toBe(`${Math.round(offset / 12) * 12}px`);
  });

  it("treats a card press below the threshold as a click, not a drag", () => {
    const board = mountBoard();
    press(card("child"), pointer("pointerdown", 200, 200));
    press(card("child"), pointer("pointermove", 202, 201));
    press(card("child"), pointer("pointerup", 202, 201));
    expect(storedOffsets()).toBeNull();
    expect(card("child").style.left).toBe(`${CHILD_BASE.x}px`);
    expect(board.cleared).toBe(0);
  });

  it("applies offsets already stored for this board on mount", () => {
    window.localStorage.setItem(
      DEFAULT_ATLAS_STORAGE_KEY,
      JSON.stringify({ root: { x: 24, y: 36 }, child: { x: -12, y: 0 } }),
    );
    mountBoard();

    expect(card("root").style.left).toBe(`${ROOT_BASE.x + 24}px`);
    expect(card("root").style.top).toBe(`${ROOT_BASE.y + 36}px`);
    expect(card("child").style.left).toBe(`${CHILD_BASE.x - 12}px`);
    expect(card("child").style.top).toBe(`${CHILD_BASE.y}px`);
  });

  it("starts clean when offsets are unavailable and keeps dragging non-fatal", () => {
    vi.spyOn(Storage.prototype, "getItem").mockReturnValue(
      JSON.stringify({ root: { x: "nope", y: 4 } }),
    );
    mountBoard();
    expect(card("root").style.left).toBe(`${ROOT_BASE.x}px`);

    vi.restoreAllMocks();
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("QuotaExceededError");
    });
    press(card("root"), pointer("pointerdown", 100, 100));
    press(card("root"), pointer("pointermove", 160, 140));
    press(card("root"), pointer("pointerup", 160, 140));
    // The Board keeps the moved card for this session; only persistence failed.
    expect(card("root").style.left).toBe("60px");
  });

  it("reads a second board's offsets when its storage key changes", () => {
    const otherKey = "myrlet.atlas.positions.other";
    window.localStorage.setItem(otherKey, JSON.stringify({ root: { x: 48, y: 0 } }));
    const controls: { current: TaskGraphControls | null } = { current: null };
    const view = (storageKey: string) => (
      <TaskGraphView
        tasks={TASKS}
        workers={[]}
        selectedTaskId={null}
        inspectorOpen={false}
        activeStatusLabel={null}
        onToggleInspector={() => undefined}
        onClearFilter={() => undefined}
        onSelectTask={() => undefined}
        onClearSelection={() => undefined}
        onBoardMetaChange={() => undefined}
        controlsRef={controls}
        storageKey={storageKey}
      />
    );
    const { rerender } = render(view(DEFAULT_ATLAS_STORAGE_KEY));
    expect(card("root").style.left).toBe("0px");

    rerender(view(otherKey));
    expect(card("root").style.left).toBe("48px");
  });
});

/* --------------------------------------------------------------------- */
/* Arrange and Restore                                                   */
/* --------------------------------------------------------------------- */

describe("Board Arrange and Restore", () => {
  it("arranges stored offsets back to the layout and restores them from the snapshot", () => {
    window.localStorage.setItem(
      DEFAULT_ATLAS_STORAGE_KEY,
      JSON.stringify({ root: { x: 24, y: 24 } }),
    );
    mountBoard();
    expect(card("root").style.left).toBe("24px");
    expect(screen.queryByRole("button", { name: /restore/i })).toBeNull();

    fireEvent.click(dockButton(/^Arrange$/));
    expect(card("root").style.left).toBe(`${ROOT_BASE.x}px`);
    expect(card("root").style.top).toBe(`${ROOT_BASE.y}px`);
    expect(window.localStorage.getItem(DEFAULT_ATLAS_STORAGE_KEY)).toBeNull();
    expect(screen.getByRole("button", { name: /restore/i })).toBeInTheDocument();

    fireEvent.click(dockButton(/^Restore$/));
    expect(card("root").style.left).toBe("24px");
    expect(storedOffsets()).toEqual({ root: { x: 24, y: 24 } });
    expect(screen.queryByRole("button", { name: /restore/i })).toBeNull();
  });

  it("offers nothing to restore when the layout was already arranged", () => {
    mountBoard();
    fireEvent.click(dockButton(/^Arrange$/));
    expect(screen.queryByRole("button", { name: /restore/i })).toBeNull();
  });

  it("publishes relation, zoom, and restore state to the status bar", () => {
    window.localStorage.setItem(
      DEFAULT_ATLAS_STORAGE_KEY,
      JSON.stringify({ root: { x: 12, y: 0 } }),
    );
    const board = mountBoard();

    const latest = board.meta[board.meta.length - 1];
    expect(latest).toEqual({ zoomPercent: 100, relationCount: 1, canRestore: false });

    fireEvent.click(dockButton(/^Arrange$/));
    const arranged = board.meta[board.meta.length - 1];
    expect(arranged?.canRestore).toBe(true);
    expect(arranged?.zoomPercent).toBe(100);
  });
});

/* --------------------------------------------------------------------- */
/* Controls contract, framing, zoom, minimap                             */
/* --------------------------------------------------------------------- */

describe("Board framing controls", () => {
  it("exposes fit, focus, reset, arrange, and restore through controlsRef", () => {
    const board = mountBoard({ selectedTaskId: "child" });
    const controls = board.controls.current;
    expect(controls).not.toBeNull();
    for (const name of ["fit", "focusSelected", "resetZoom", "arrange", "restore"] as const) {
      expect(typeof controls?.[name]).toBe("function");
    }

    const fitted = world().style.transform;
    act(() => {
      screen.getByRole("button", { name: /zoom in/i }).click();
    });
    expect(world().style.transform).not.toBe(fitted);

    act(() => board.controls.current?.fit());
    expect(world().style.transform).toBe(fitted);

    act(() => {
      screen.getByRole("button", { name: /zoom in/i }).click();
    });
    act(() => board.controls.current?.resetZoom());
    expect(world().style.transform).toContain("scale(1)");

    act(() => board.controls.current?.arrange());
    expect(world().style.transform).toContain("scale(1)");
  });

  it("focuses the selected card at the viewport center and no-ops without a selection", () => {
    const unselected = mountBoard();
    const before = world().style.transform;
    act(() => unselected.controls.current?.focusSelected());
    expect(world().style.transform).toBe(before);
    cleanup();

    mountBoard({ selectedTaskId: "child" });
    act(() => screen.getByRole("button", { name: /focus/i }).click());
    // CHILD_BASE center at scale 1 maps to the exact viewport center.
    const transform = world().style.transform;
    const panX = Number(/translate\(([-\d.]+)px/.exec(transform)?.[1]);
    expect(panX).toBeCloseTo(VIEWPORT_WIDTH / 2 - (CHILD_BASE.x + NODE_WIDTH / 2), 5);
  });

  it("shows Focus only with a selection and toggles the Inspector pressed state", () => {
    mountBoard({ selectedTaskId: null });
    expect(screen.queryByRole("button", { name: /focus/i })).toBeNull();
    expect(dockButton(/^Inspector$/)).toHaveAttribute("aria-pressed", "false");
    cleanup();

    mountBoard({ selectedTaskId: "root" });
    expect(screen.getByRole("button", { name: /focus/i })).toBeInTheDocument();
  });

  it("zooms toward a readable first frame and frames the atlas on Fit", () => {
    mountBoard();
    // A wide stage lands on the whole atlas at natural size.
    expect(world().style.transform).toContain("scale(1)");

    fireEvent.click(screen.getByRole("button", { name: /zoom out/i }));
    expect(Number(/scale\(([\d.]+)\)/.exec(world().style.transform)?.[1])).toBeLessThan(1);
    fireEvent.click(screen.getByRole("button", { name: /zoom in/i }));
    fireEvent.click(screen.getByRole("button", { name: /reset to 100%/i }));
    expect(world().style.transform).toContain("scale(1)");
  });

  it("starts the minimap collapsed on a phone and open on desktop", () => {
    const media = (matches: boolean): void => {
      vi.stubGlobal(
        "matchMedia",
        vi.fn((query: string) => ({
          matches: query.includes("max-width: 720px") ? matches : false,
          media: query,
          onchange: null,
          addEventListener: () => undefined,
          removeEventListener: () => undefined,
          addListener: () => undefined,
          removeListener: () => undefined,
          dispatchEvent: () => true,
        })),
      );
    };

    media(false);
    mountBoard();
    expect(document.querySelector(".atlas-minimap")).toHaveAttribute("data-collapsed", "false");
    expect(screen.getByRole("button", { name: /collapse minimap/i })).toBeInTheDocument();
    cleanup();

    media(true);
    mountBoard();
    expect(document.querySelector(".atlas-minimap")).toHaveAttribute("data-collapsed", "true");
    expect(screen.getByRole("button", { name: /expand minimap/i })).toBeInTheDocument();
  });

  it("pans the canvas to the world point clicked in the minimap body", () => {
    mountBoard();
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue(
      new DOMRect(0, 0, 176, 112),
    );
    const transformBefore = world().style.transform;
    fireEvent.click(
      screen.getByRole("button", { name: /minimap overview/i }),
      { clientX: 88, clientY: 56 },
    );
    expect(world().style.transform).not.toBe(transformBefore);
  });
});
