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
  /** Dependencies that are not `finished`, with their status. */
  blockedBy: BlockingDependency[];
  /** Latest Harness-written Markdown, when the Delivery has one. */
  resultMarkdown: string | null;
  /** When Hub last stored the result; independent of `finishedAtMs`. */
  resultRecordedAtMs: number | null;
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
  /** Dependencies that are not `finished`, so `held` has a visible reason. */
  blockedBy: BlockingDependency[];
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

/**
 * Delivery lifecycle states. A claim commits the Delivery directly, so there is no
 * offer to accept, refuse, or release: `accepted` is the one live state.
 */
export type DeliveryStatus = "accepted" | "finished" | "cancelled";

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

/** The reviewable scheduling content the Hub keeps per Worker revision. */
export interface WorkerSchedulingConfig {
  slots: SlotInventory[];
}

/** Effective trust state of one enrolled Worker. */
export type WorkerApprovalStatus = "pending" | "approved" | "revoked";

/** One enrolled Worker's connectivity, approval, and applied scheduling inventory. */
export interface WorkerRecord {
  workerId: WorkerId;
  online: boolean;
  revoked: boolean;
  approval: WorkerApprovalStatus;
  connectionEpoch: number;
  lastSeenAtMs: number | null;
  activeDeliveries: number;
  /** Host facts from the latest accepted report; ordinary telemetry, never a gate. */
  host: HostDescriptor | null;
  /** Hub-assigned revision of the latest accepted report, or `null` before one arrives. */
  reportedRevision: number | null;
  /** Hub-assigned revision the Hub schedules with, or `null` before an Apply. */
  appliedRevision: number | null;
  appliedConfig: WorkerSchedulingConfig | null;
}

/** Alias matching the Rust `SnapshotWorker` name. */
export type SnapshotWorker = WorkerRecord;

/** One Worker's detail, its reported snapshot under review, and claimable Task IDs. */
export interface WorkerDetail {
  worker: WorkerRecord;
  reportedConfig: WorkerSchedulingConfig | null;
  claimableTaskIds: TaskId[];
}

/** The two Hub-wide Security & Trust policies; both default to on. */
export interface SecurityTrustSettings {
  requireWorkerApproval: boolean;
  requireConfigurationApply: boolean;
}

/** `PATCH /v1/settings/security-trust` request; omitted fields retain their values. */
export interface UpdateSecurityTrustRequest {
  requireWorkerApproval?: boolean;
  requireConfigurationApply?: boolean;
}

/** `POST /v1/workers/{id}/config/apply` response. */
export interface ApplyWorkerConfigResponse {
  appliedRevision: number;
}

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
/* Task commands                                                          */
/* --------------------------------------------------------------------- */

/** One declared prerequisite that keeps a Task `held`, with its current status. */
export interface BlockingDependency {
  taskId: TaskId;
  status: TaskStatus;
}

/** Bounded newest-first Task summary page returned by `GET /v1/tasks`. */
export interface TaskListResponse {
  tasks: SnapshotTaskSummary[];
  totalTasks: number;
  truncated: boolean;
  eventCursor: number;
}

/** `POST /v1/tasks` response: the authorized Task and whether it became `queued`. */
export interface SubmitTaskResponse {
  task: TaskRecord;
  eligible: boolean;
}

/** Where a Task stands for a would-be claimer. */
export interface TaskEligibility {
  taskId: TaskId;
  claimable: boolean;
  blockedBy: BlockingDependency[];
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
