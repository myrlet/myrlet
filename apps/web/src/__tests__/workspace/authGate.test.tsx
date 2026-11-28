/**
 * Auth gate: the workspace is never shown before a token is accepted, and the
 * bearer token never reaches storage.
 */

import "@testing-library/jest-dom/vitest";

import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import App from "../../App";
import {
  findTokenInput,
  installWorkspaceHub,
  localStorageKeys,
  resetWorkspaceTest,
  submitAuthGate,
} from "../support/workspace";

beforeEach(installWorkspaceHub);
afterEach(resetWorkspaceTest);

describe("Auth gate", () => {
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

  it("leaves workspace shortcuts inactive on the auth gate", async () => {
    render(<App />);
    expect(fireEvent.keyDown(findTokenInput(), { key: "k", ctrlKey: true })).toBe(true);
    fireEvent.change(findTokenInput(), { target: { value: "test-token" } });
    submitAuthGate();
    await screen.findByRole("toolbar", { name: /board controls/i });
    expect(screen.queryByRole("dialog", { name: /command palette/i })).toBeNull();
  });
});
