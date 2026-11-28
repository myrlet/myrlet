import type {
  GlobalSnapshot,
  SecurityTrustSettings,
  SnapshotTaskSummary,
  TaskRecord,
  WorkerRecord,
} from "../../api/types";

/** The Hub's default Security & Trust policies: both gates on. */
export function trustSettings(
  overrides: Partial<SecurityTrustSettings> = {},
): SecurityTrustSettings {
  return {
    requireWorkerApproval: true,
    requireConfigurationApply: true,
    ...overrides,
  };
}

/** A Snapshot Task summary with truthful defaults; override per test. */
export function taskSummary(
  overrides: Partial<SnapshotTaskSummary> & { id: string },
): SnapshotTaskSummary {
  return {
    parentTaskId: null,
    rootTaskId: overrides.id,
    depth: 1,
    sourceTaskId: null,
    dependencies: [],
    blockedBy: [],
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

/** One approved, enrolled Worker whose Slot capacity is unknown (`null`, never `0`). */
export function workerRecord(overrides: Partial<WorkerRecord> = {}): WorkerRecord {
  return {
    workerId: "wkr_00000000000a",
    online: true,
    revoked: false,
    approval: "approved",
    permissionTier: "worker",
    connectionEpoch: 4,
    lastSeenAtMs: Date.now(),
    activeDeliveries: 1,
    host: { os: "darwin", arch: "arm64", deploymentMode: "native" },
    reportedRevision: 2,
    appliedRevision: 2,
    appliedConfig: {
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
export function taskRecord(summary: SnapshotTaskSummary): TaskRecord {
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
    blockedBy: summary.blockedBy,
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

/** One Task per honest status, plus the Worker that makes the chrome whole. */
export function buildSnapshot(): GlobalSnapshot {
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
