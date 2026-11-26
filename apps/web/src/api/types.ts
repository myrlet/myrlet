/**
 * Wire DTOs for the Myrlet Hub Admin API.
 *
 * Mirrors `crates/protocol/src/{task,delivery,inventory,progress,snapshot,api,bundle}.rs`
 * exactly: every JSON object uses `camelCase` keys and every enum value is the
 * `snake_case` variant serde emits. Identifiers are opaque strings on the wire
 * (`taskId`, `workerId`, `deliveryId`, `slotId`, `executorId`).
 *
 * These are *facts*. Do not model success/failure verdicts, percentages, or
 * step counts here — Hub does not own them.
 */

export type TaskId = string;
export type WorkerId = string;
export type DeliveryId = string;
export type SlotId = string;
export type ExecutorId = string;

/* --------------------------------------------------------------------- */
/* Task                                                                   */
/* --------------------------------------------------------------------- */

/** The five Hub-owned Task states; no `running` and no success/failure verdict. */
export type TaskStatus = "held" | "queued" | "dispatched" | "finished" | "cancelled";

/** Derived per-status counts for a Task subtree; never changes the Parent's status. */
export interface SubtreeCounts {
  total: number;
  held: number;
  queued: number;
  dispatched: number;
  finished: number;
  cancelled: number;
}

/**
 * Routing constraints (`TaskRouting` on the wire): capabilities are hard
 * constraints, every other field is a preference.
 */
export interface TaskRoutingHints {
  requiredCapabilities?: string[];
  preferredWorkerId?: WorkerId | null;
  preferredRole?: string | null;
  preferredExecutorId?: ExecutorId | null;
}

/**
 * Execution hints (`TaskExecution` on the wire) passed through to the Harness
 * without Hub interpretation. Omitted when the Harness has no native flag.
 */
export interface TaskExecutionHints {
  preferredModel?: string | null;
  timeoutSeconds?: number | null;
  parameters?: Record<string, unknown>;
}

/** Immutable client-supplied Task fields as serialized by Hub on Task reads. */
export interface TaskDefinition {
  clientRef: string | null;
  parentTaskId: TaskId | null;
  sourceTaskId: TaskId | null;
  dependencies: TaskId[];
  category: string;
  tags: string[];
  title: string;
  prompt: string;
  input: unknown;
  expectedOutput: unknown;
  externalReferences: string[];
  routing: TaskRoutingHints;
  execution: TaskExecutionHints;
  metadata: Record<string, unknown>;
}

/**
 * Task definition accepted by `POST /v1/tasks` and Task Bundle import.
 * Fields serde would default (`category` -> `"general"`) or treat as absent
 * (`Option`/`Vec`/maps) may be omitted; `title` and `prompt` are required.
 */
export interface TaskDefinitionInput {
  clientRef?: string | null;
  parentTaskId?: TaskId | null;
  sourceTaskId?: TaskId | null;
  dependencies?: TaskId[];
  category?: string;
  tags?: string[];
  title: string;
  prompt: string;
  input?: unknown;
  expectedOutput?: unknown;
  externalReferences?: string[];
  routing?: TaskRoutingHints;
  execution?: TaskExecutionHints;
  metadata?: Record<string, unknown>;
}

/** Authoritative Hub-owned Task record returned by `GET /v1/tasks/{id}`. */
export interface TaskRecord {
  id: TaskId;
  rootTaskId: TaskId;
  depth: number;
  status: TaskStatus;
  version: number;
  createdAtMs: number;
  updatedAtMs: number;
  finishedAtMs: number | null;
  task: TaskDefinition;
  currentDelivery: DeliveryView | null;
  subtree: SubtreeCounts;
}

/** Bounded Task read model embedded in the Global Snapshot. */
export interface SnapshotTaskSummary {
  id: TaskId;
  parentTaskId: TaskId | null;
  rootTaskId: TaskId;
  depth: number;
  sourceTaskId: TaskId | null;
  dependencies: TaskId[];
  category: string;
  title: string;
  status: TaskStatus;
  version: number;
  createdAtMs: number;
  updatedAtMs: number;
  finishedAtMs: number | null;
  currentDelivery: DeliveryView | null;
  subtree: SubtreeCounts;
}

/** Direction of a bounded Task tree read. */
export type TaskTreeDirection = "ancestors" | "descendants";

/** `GET /v1/tasks/{id}/tree` response. */
export interface TaskTreeResponse {
  rootTaskId: TaskId;
  tasks: TaskRecord[];
  truncated: boolean;
}

/* --------------------------------------------------------------------- */
/* Delivery and Progress                                                  */
/* --------------------------------------------------------------------- */

/** Delivery lifecycle states. */
export type DeliveryStatus =
  | "offered"
  | "accepted"
  | "rejected"
  | "released"
  | "finished"
  | "cancelled";

/** Pre-registration refusal reason reported by a Worker. */
export type RejectionCode =
  | "executor_unavailable"
  | "worker_shutting_down"
  | "recovery_incomplete";

/**
 * Harness cancellation response relayed by a Worker. Never a claim of physical
 * termination: `uncertain` stays visible, `unsupported` stays honest.
 */
export type CancelState = "confirmed" | "failed" | "uncertain" | "unsupported";

/** Latest accepted Progress for a Delivery; Hub stores no Progress history. */
export interface DeliveryProgressView {
  deliveryId: DeliveryId;
  taskId: TaskId;
  text: string;
  observedAtMs: number;
  receivedAtMs: number;
  /** Immutable timestamp of the first accepted Progress (execution-start evidence). */
  firstReceivedAtMs: number;
}

/** Current Delivery summary attached to Task reads and the Snapshot. */
export interface DeliveryView {
  id: DeliveryId;
  taskId: TaskId;
  workerId: WorkerId;
  slotId: SlotId;
  executorId: ExecutorId | null;
  /** Epoch of the WSS connection this Delivery is fenced to. */
  connectionEpoch: number;
  state: DeliveryStatus;
  rejectionCode: RejectionCode | null;
  cancelState: CancelState | null;
  cancelReason: string | null;
  createdAtMs: number;
  updatedAtMs: number;
  /** Latest accepted Progress for this Delivery, if any. */
  progress: DeliveryProgressView | null;
}

/* --------------------------------------------------------------------- */
/* Inventory                                                              */
/* --------------------------------------------------------------------- */

/** Closed capability state set. */
export type CapabilityState = "available" | "degraded" | "unavailable";

/** Harness adapter kinds shipped in V1. */
export type ExecutorKind = "claude" | "codex" | "pi" | "process";

/** Basic host facts only; Worker Core never probes toolchains or SDKs. */
export type DeploymentMode = "native" | "container";

/** `HostFacts` on the wire. */
export interface HostDescriptor {
  os: string;
  arch: string;
  deploymentMode: DeploymentMode;
}

/** `CapabilityClaim` on the wire. */
export interface CapabilityEntry {
  name: string;
  state: CapabilityState;
}

/** `ExecutorInfo` on the wire. */
export interface ExecutorDescriptor {
  id: ExecutorId;
  kind: ExecutorKind;
}

/** One selectable execution/concurrency unit reported by a Worker. */
export interface SlotInventory {
  slotId: SlotId;
  enabled: boolean;
  /** Advisory only; `null` means unknown capacity and must never render as `0`. */
  advisoryCapacity: number | null;
  roles: string[];
  capabilities: CapabilityEntry[];
  executors: ExecutorDescriptor[];
}

/** Worker-reported inventory (`worker.hello` / `worker.inventory`). */
export interface WorkerInventory {
  revision: number;
  observedAtMs: number;
  host: HostDescriptor;
  slots: SlotInventory[];
}

/** One enrolled Worker's connectivity and last reported inventory. */
export interface WorkerRecord {
  workerId: WorkerId;
  online: boolean;
  revoked: boolean;
  connectionEpoch: number;
  lastSeenAtMs: number | null;
  activeDeliveries: number;
  inventory: WorkerInventory | null;
}

/** Alias matching the Rust `SnapshotWorker` name. */
export type SnapshotWorker = WorkerRecord;

/* --------------------------------------------------------------------- */
/* Snapshot and events                                                    */
/* --------------------------------------------------------------------- */

/** Consistent cluster-wide read model returned by `GET /v1/snapshot`. */
export interface GlobalSnapshot {
  /** Always `"myrlet.snapshot/v1"`. */
  schemaVersion: string;
  generatedAtMs: number;
  eventCursor: number;
  tasksTruncated: boolean;
  totalTasks: number;
  workers: WorkerRecord[];
  tasks: SnapshotTaskSummary[];
}

/**
 * One invalidation hint replayed on the Hub SSE event stream. SSE frames are
 * hints, never authoritative state: refresh the Snapshot or Task detail.
 */
export interface HubEventEnvelope {
  seq: number;
  kind: string;
  createdAtMs: number;
  payload: unknown;
}

/* --------------------------------------------------------------------- */
/* Dispatch and Task commands                                             */
/* --------------------------------------------------------------------- */

/**
 * How a dispatch command picks a Worker/Slot.
 * `targeted` never falls back; `match` is deterministic and closed-set.
 */
export type DispatchSelection =
  | {
      mode: "targeted";
      workerId: WorkerId;
      slotId: SlotId;
      executorId?: ExecutorId | null;
    }
  | {
      mode: "match";
      fallbackAllowed?: boolean;
    };

/** `POST /v1/tasks/{id}/dispatch` request. */
export interface DispatchTaskRequest {
  expectedTaskVersion: number;
  selection: DispatchSelection;
  queue?: boolean;
}

/** `POST /v1/tasks/{id}/dispatch` response. */
export interface DispatchTaskResponse {
  taskId: TaskId;
  status: TaskStatus;
  version: number;
  delivery: DeliveryView | null;
}

/** `POST /v1/tasks/{id}/cancel` request. */
export interface CancelTaskRequest {
  expectedTaskVersion: number;
  reason?: string | null;
}

/** `POST /v1/tasks/{id}/finish` request (coordination-only `held` Parent). */
export interface FinishTaskRequest {
  expectedTaskVersion: number;
}

/* --------------------------------------------------------------------- */
/* Task Bundle import                                                     */
/* --------------------------------------------------------------------- */

/** One Task definition inside a Bundle import (`tasks[]` entry). */
export type TaskBundleItem = TaskDefinitionInput;

/** `POST /v1/task-bundles/import` request (1..=100 definitions). */
export interface TaskBundleImportRequest {
  tasks: TaskBundleItem[];
}

/** `POST /v1/task-bundles/import` response with resolved local references. */
export interface TaskBundleImportResponse {
  clientRefToTaskId: Record<string, TaskId>;
  tasks: TaskRecord[];
}

/* --------------------------------------------------------------------- */
/* Workers, enrollment, health, errors                                    */
/* --------------------------------------------------------------------- */

/** `GET /v1/workers` response. */
export interface ListWorkersResponse {
  workers: WorkerRecord[];
}

/** `POST /v1/enrollment-codes` request. */
export interface CreateEnrollmentCodeRequest {
  ttlSeconds?: number | null;
  suggestedWorkerId?: WorkerId | null;
}

/** `POST /v1/enrollment-codes` response. */
export interface CreateEnrollmentCodeResponse {
  enrollmentCode: string;
  expiresAtMs: number;
  suggestedWorkerId: WorkerId | null;
}

/** `POST /v1/workers/{id}/rotate` response. */
export interface RotateWorkerResponse {
  hubId: string;
  workerId: WorkerId;
  credential: string;
}

/** Stable error body; never leaks internal SQLite causes. */
export interface ErrorResponse {
  error: string;
  message: string;
}

/** Process liveness probe body. */
export interface HealthResponse {
  status: string;
  version: string;
}
