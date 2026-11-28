import { describe, expect, it } from "vitest";

import { isWorkerOnline } from "../../model/workerPresence";
import { workerRecord } from "../support/fixtures";

describe("isWorkerOnline", () => {
  it("treats a missing record as offline", () => {
    expect(isWorkerOnline(undefined)).toBe(false);
  });

  it("treats a connected, unrevoked, approved Worker as online", () => {
    expect(isWorkerOnline(workerRecord())).toBe(true);
  });

  it("treats a disconnected or revoked Worker as offline", () => {
    expect(isWorkerOnline(workerRecord({ online: false }))).toBe(false);
    expect(isWorkerOnline(workerRecord({ revoked: true }))).toBe(false);
    expect(isWorkerOnline(workerRecord({ online: false, revoked: true }))).toBe(false);
  });

  it("never calls a pending Worker online, even while it is connected", () => {
    expect(isWorkerOnline(workerRecord({ approval: "pending" }))).toBe(false);
  });
});
