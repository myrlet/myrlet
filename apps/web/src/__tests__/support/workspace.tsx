/**
 * Shared plumbing for App-level workspace tests: the default Hub stub, the
 * Auth gate flow, and navigation helpers. Every helper here serves more than
 * one feature suite; feature-specific behaviour stays in its own file.
 */

import "@testing-library/jest-dom/vitest";

import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { vi } from "vitest";

import App from "../../App";
import { stubColorScheme } from "./appearance";
import { buildSnapshot, taskRecord, trustSettings, workerDetail, workerRecord } from "./fixtures";
import { idleEventStream, jsonResponse, stubFetch } from "./hub";

function defaultWorkspaceHandler(url: string): Response | Promise<Response> {
  const path = url.replace(/^https?:\/\/[^/]+/, "");
  if (path.startsWith("/v1/events")) {
    return idleEventStream();
  }
  if (path.startsWith("/v1/snapshot")) {
    return jsonResponse(buildSnapshot());
  }
  if (path.startsWith("/v1/settings/security-trust")) {
    return jsonResponse(trustSettings());
  }
  if (/^\/v1\/workers\/[^/]+$/.test(path.split("?")[0])) {
    // One Worker's Detail: the Snapshot carries no revisions/config, so the card reads
    // both here. This default Worker is approved and not pending.
    return jsonResponse(workerDetail(workerRecord()));
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
        related: [],
        truncated: false,
      });
    }
    return jsonResponse({ task: taskRecord(summary), summary: null, related: [] });
  }
  if (path.startsWith("/v1/tasks")) {
    return jsonResponse({
      task: taskRecord(buildSnapshot().tasks[0]),
      summary: null,
      related: [],
    });
  }
  return jsonResponse({ error: "NotFound", message: "Not found." }, 404);
}

/** Fresh storage, a light System appearance, and the default Hub stub. */
export function installWorkspaceHub(): void {
  window.localStorage.clear();
  stubColorScheme(false);
  stubFetch(defaultWorkspaceHandler);
}

/** Restore globals, DOM, storage, and the resolved theme after each test. */
export function resetWorkspaceTest(): void {
  cleanup();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  window.localStorage.clear();
  delete document.documentElement.dataset.theme;
}

/** Find the Auth gate's token field without depending on one exact label. */
export function findTokenInput(): HTMLInputElement {
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

export function submitAuthGate(): void {
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
export function modeSwitch(): HTMLElement {
  return screen.getByRole("group", { name: /workspace mode/i });
}

/** Render the app, complete the Auth gate, and wait for the first Snapshot. */
export async function connectWorkspace(): Promise<void> {
  render(<App />);
  expect(screen.queryByText("Handling ended")).toBeNull();
  const tokenField = findTokenInput();
  fireEvent.change(tokenField, { target: { value: "admin-token-value" } });
  submitAuthGate();
  await waitFor(() => {
    expect(document.querySelector(".app-bar")).not.toBeNull();
  });
  // The App Bar paints before the workspace's passive effects run. Keyboard
  // tests press document keys immediately after connecting, so let that effect
  // pass register its `keydown` listener before the test can race it.
  await act(async () => {});
}

export function localStorageKeys(): string[] {
  return Array.from({ length: window.localStorage.length }, (_value, index) =>
    window.localStorage.key(index),
  ).filter((key): key is string => key !== null);
}

export function appBar(): HTMLElement {
  return document.querySelector(".app-bar") as HTMLElement;
}

export function boardFloatingBar(): HTMLElement {
  return screen.getByRole("toolbar", { name: /board controls/i });
}

export function openList(): void {
  fireEvent.click(within(modeSwitch()).getByRole("button", { name: /^list$/i }));
}

export async function openBoard(): Promise<void> {
  fireEvent.click(within(modeSwitch()).getByRole("button", { name: /^board$/i }));
  await waitFor(() => {
    expect(document.querySelector('.view-body[data-view="board"]')).not.toBeNull();
  });
}
