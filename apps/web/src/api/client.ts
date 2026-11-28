/**
 * Typed Hub Admin API client and fetch-based SSE reader.
 *
 * Why `fetch` and not `EventSource`: a browser `EventSource` cannot attach an
 * `Authorization: Bearer <admin-token>` header, while the Hub requires one on
 * `/v1/events`. We therefore stream `response.body` manually and parse SSE
 * frames ourselves.
 *
 * The client is created per session and holds the bearer token in memory only.
 */

import type {
  CreateEnrollmentCodeRequest,
  CreateEnrollmentCodeResponse,
  ErrorResponse,
  GlobalSnapshot,
  HealthResponse,
  HubEventEnvelope,
  ListWorkersResponse,
  RotateWorkerResponse,
  TaskRecord,
  TaskTreeDirection,
  TaskTreeResponse,
  WorkerDetail,
} from "./types";

export interface HubClientOptions {
  /** Hub origin such as `https://hub.example:4140`; `""` means same-origin. */
  readonly baseUrl: string;
  /** Admin bearer token; kept in memory for the lifetime of this client. */
  readonly token: string;
}

/** Stable failure shape for every Hub call (network failures use status `0`). */
export class HubApiError extends Error {
  readonly status: number;
  readonly code: string;

  constructor(status: number, code: string, message: string) {
    super(message);
    this.name = "HubApiError";
    this.status = status;
    this.code = code;
  }

  get isUnauthorized(): boolean {
    return this.status === 401;
  }

  get isCursorExpired(): boolean {
    return this.status === 410;
  }

  get isConflict(): boolean {
    return this.status === 409;
  }

  get isNotFound(): boolean {
    return this.status === 404;
  }

  get isNetworkFailure(): boolean {
    return this.status === 0;
  }
}

/** Terminal reason an event stream stopped; the caller decides how to retry. */
export type EventStreamOutcome =
  | "aborted"
  | "unauthorized"
  | "cursor_expired"
  | "disconnected"
  | "ended";

export interface EventStreamHandlers {
  /** The stream is open and authenticated. */
  onOpen?: () => void;
  /** One parsed invalidation hint. */
  onEvent: (event: HubEventEnvelope) => void;
  /** The token was rejected; the stream is gone. */
  onUnauthorized?: (error: HubApiError) => void;
  /** The resume cursor fell outside retention; reload the Snapshot. */
  onCursorExpired?: (error: HubApiError) => void;
  /** A transient network/stream failure; retry with backoff. */
  onDisconnect?: (error?: unknown) => void;
}

type HttpMethod = "GET" | "POST";

type QueryValue = string | number | boolean | null | undefined;
type QueryParams = Record<string, QueryValue>;

interface RequestOptions {
  readonly body?: unknown;
  readonly signal?: AbortSignal;
  readonly query?: QueryParams;
}

const NETWORK_ERROR_CODE = "NetworkError";

/** Normalize a user-entered Hub URL: trim, drop trailing slashes, keep `""`. */
function normalizeBaseUrl(raw: string): string {
  const trimmed = raw.trim();
  if (trimmed === "" || trimmed === "/") {
    return "";
  }
  return trimmed.replace(/\/+$/, "");
}

/**
 * Public reachability probe for the Auth gate. A successful HTTP response
 * establishes reachability even when the body has no readable version.
 */
export async function probeHubHealth(
  baseUrl: string,
): Promise<{ ok: boolean; version?: string }> {
  let response: Response;
  try {
    response = await fetch(`${normalizeBaseUrl(baseUrl)}/health`, {
      method: "GET",
      headers: { Accept: "application/json" },
      cache: "no-store",
    });
  } catch {
    return { ok: false };
  }

  if (!response.ok) {
    return { ok: false };
  }

  try {
    const body: unknown = await response.json();
    if (typeof body === "object" && body !== null) {
      const record = body as Partial<HealthResponse> & Record<string, unknown>;
      return {
        ok: true,
        version: typeof record.version === "string" ? record.version : undefined,
      };
    }
  } catch {
    // A reachable process with an unreadable body is still reachable.
  }

  return { ok: true };
}

function isAbortError(error: unknown): boolean {
  return (
    typeof error === "object" &&
    error !== null &&
    "name" in error &&
    (error as { name?: unknown }).name === "AbortError"
  );
}

function defaultErrorCode(status: number): string {
  switch (status) {
    case 400:
      return "BadRequest";
    case 401:
      return "Unauthorized";
    case 404:
      return "NotFound";
    case 408:
      return "RequestTimeout";
    case 409:
      return "Conflict";
    case 410:
      return "CursorExpired";
    case 413:
      return "PayloadTooLarge";
    case 503:
      return "ServiceUnavailable";
    default:
      return `Http${status}`;
  }
}

function parseJson(text: string): unknown {
  const trimmed = text.trim();
  if (trimmed === "") {
    return undefined;
  }
  try {
    return JSON.parse(trimmed) as unknown;
  } catch {
    return undefined;
  }
}

function readErrorFields(value: unknown): { code?: string; message?: string } {
  if (typeof value !== "object" || value === null) {
    return {};
  }
  const record = value as Partial<ErrorResponse> & Record<string, unknown>;
  return {
    code: typeof record.error === "string" ? record.error : undefined,
    message: typeof record.message === "string" ? record.message : undefined,
  };
}

/**
 * Parse one SSE frame into a `HubEventEnvelope`.
 *
 * Frames carry `id:` (the event sequence) and `data:` (the JSON envelope).
 * Comment lines (`:`) and unknown fields (`event:`, `retry:`) are ignored.
 */
function parseEventFrame(frame: string): HubEventEnvelope | null {
  let frameId: number | null = null;
  const dataLines: string[] = [];

  for (const rawLine of frame.split("\n")) {
    const line = rawLine.endsWith("\r") ? rawLine.slice(0, -1) : rawLine;
    if (line === "" || line.startsWith(":")) {
      continue;
    }
    const colon = line.indexOf(":");
    const field = colon === -1 ? line : line.slice(0, colon);
    let value = colon === -1 ? "" : line.slice(colon + 1);
    if (value.startsWith(" ")) {
      value = value.slice(1);
    }
    if (field === "data") {
      dataLines.push(value);
    } else if (field === "id") {
      const parsed = Number.parseInt(value, 10);
      if (Number.isFinite(parsed)) {
        frameId = parsed;
      }
    }
  }

  if (dataLines.length === 0) {
    return null;
  }

  const parsed = parseJson(dataLines.join("\n"));
  if (typeof parsed !== "object" || parsed === null) {
    return null;
  }

  const envelope = parsed as HubEventEnvelope;
  if (typeof envelope.seq !== "number") {
    if (frameId === null) {
      return null;
    }
    envelope.seq = frameId;
  }
  return envelope;
}

/** Read `HubClient` — construct one per authenticated session. */
export class HubClient {
  /** Normalized base URL (`""` = same-origin). */
  readonly baseUrl: string;

  private readonly token: string;

  constructor(options: HubClientOptions) {
    this.baseUrl = normalizeBaseUrl(options.baseUrl);
    this.token = options.token.trim();
  }

  /* -------------------------------- Reads ------------------------------ */

  /** Unauthenticated liveness probe. */
  async health(): Promise<HealthResponse> {
    return this.request<HealthResponse>("GET", "/v1/health");
  }

  /** Primary read view: workers, bounded Task summaries, and `eventCursor`. */
  async getSnapshot(): Promise<GlobalSnapshot> {
    return this.request<GlobalSnapshot>("GET", "/v1/snapshot");
  }

  /** Authoritative Task record, its current Delivery, and latest Progress. */
  async getTask(taskId: string): Promise<TaskRecord> {
    return this.request<TaskRecord>("GET", `/v1/tasks/${encodeURIComponent(taskId)}`);
  }

  /** Bounded ancestors or descendants of one Task (simple limit, no cursor). */
  async getTaskTree(
    taskId: string,
    direction: TaskTreeDirection = "descendants",
    maxDepth = 5,
    limit = 200,
  ): Promise<TaskTreeResponse> {
    return this.request<TaskTreeResponse>(
      "GET",
      `/v1/tasks/${encodeURIComponent(taskId)}/tree`,
      { query: { direction, maxDepth, limit } },
    );
  }

  /* ------------------------------- Workers ----------------------------- */

  /** Enrolled Workers with connectivity and reported inventory. */
  async listWorkers(): Promise<ListWorkersResponse> {
    return this.request<ListWorkersResponse>("GET", "/v1/workers");
  }

  /** One Worker's detail, inventory, and the Task IDs it could claim next. */
  async getWorker(workerId: string): Promise<WorkerDetail> {
    return this.request<WorkerDetail>(
      "GET",
      `/v1/workers/${encodeURIComponent(workerId)}`,
    );
  }

  /** Issue a short-lived one-time Worker pairing code. */
  async createEnrollmentCode(
    request: CreateEnrollmentCodeRequest,
  ): Promise<CreateEnrollmentCodeResponse> {
    return this.request<CreateEnrollmentCodeResponse>("POST", "/v1/enrollment-codes", {
      body: request,
    });
  }

  /** Rotate a Worker's bearer credential (fences its live WSS session). */
  async rotateWorker(workerId: string): Promise<RotateWorkerResponse> {
    return this.request<RotateWorkerResponse>(
      "POST",
      `/v1/workers/${encodeURIComponent(workerId)}/rotate`,
    );
  }

  /** Revoke a Worker's credential and disconnect its live WSS session. */
  async revokeWorker(workerId: string): Promise<void> {
    await this.request<void>("POST", `/v1/workers/${encodeURIComponent(workerId)}/revoke`);
  }

  /* -------------------------------- SSE -------------------------------- */

  /**
   * Stream Hub invalidation hints until the connection ends or `signal` aborts.
   *
   * Resolves with the terminal outcome after invoking the matching handler;
   * it never throws for expected stream states. `cursor` is sent as
   * `Last-Event-ID` so Hub can backfill (and answer `410 CursorExpired`).
   */
  async openEvents(
    cursor: number | null | undefined,
    signal: AbortSignal,
    handlers: EventStreamHandlers,
  ): Promise<EventStreamOutcome> {
    const headers: Record<string, string> = {
      Accept: "text/event-stream",
      "Cache-Control": "no-cache",
    };
    if (this.token !== "") {
      headers.Authorization = `Bearer ${this.token}`;
    }
    if (typeof cursor === "number" && Number.isFinite(cursor) && cursor > 0) {
      headers["Last-Event-ID"] = String(cursor);
    }

    let response: Response;
    try {
      response = await fetch(this.buildUrl("/v1/events"), {
        method: "GET",
        headers,
        signal,
        cache: "no-store",
      });
    } catch (error) {
      if (signal.aborted || isAbortError(error)) {
        return "aborted";
      }
      handlers.onDisconnect?.(error);
      return "disconnected";
    }

    if (!response.ok) {
      const error = await this.readError(response);
      if (response.status === 401) {
        handlers.onUnauthorized?.(error);
        return "unauthorized";
      }
      if (response.status === 410) {
        handlers.onCursorExpired?.(error);
        return "cursor_expired";
      }
      handlers.onDisconnect?.(error);
      return "disconnected";
    }

    const body = response.body;
    if (!body) {
      handlers.onDisconnect?.(
        new HubApiError(
          response.status,
          "StreamUnavailable",
          "The Hub returned an event stream without a body.",
        ),
      );
      return "disconnected";
    }

    handlers.onOpen?.();

    const reader = body.getReader();
    const decoder = new TextDecoder();
    let buffer = "";

    try {
      while (true) {
        const { done, value } = await reader.read();
        if (done) {
          break;
        }
        buffer += decoder.decode(value, { stream: true });
        buffer = buffer.replace(/\r\n/g, "\n");

        let boundary = buffer.indexOf("\n\n");
        while (boundary !== -1) {
          const frame = buffer.slice(0, boundary);
          buffer = buffer.slice(boundary + 2);
          const event = parseEventFrame(frame);
          if (event) {
            handlers.onEvent(event);
          }
          boundary = buffer.indexOf("\n\n");
        }
      }

      buffer += decoder.decode();
      if (buffer.trim() !== "") {
        const event = parseEventFrame(buffer);
        if (event) {
          handlers.onEvent(event);
        }
      }
      return signal.aborted ? "aborted" : "ended";
    } catch (error) {
      if (signal.aborted || isAbortError(error)) {
        return "aborted";
      }
      handlers.onDisconnect?.(error);
      return "disconnected";
    } finally {
      try {
        reader.releaseLock();
      } catch {
        // The reader was already released; nothing to do.
      }
      try {
        await body.cancel();
      } catch {
        // The stream is already closed or errored.
      }
    }
  }

  /* ----------------------------- Internals ----------------------------- */

  private buildUrl(path: string, query?: QueryParams): string {
    const base = `${this.baseUrl}${path}`;
    if (!query) {
      return base;
    }
    const params = new URLSearchParams();
    for (const [key, value] of Object.entries(query)) {
      if (value === undefined || value === null) {
        continue;
      }
      params.set(key, String(value));
    }
    const serialized = params.toString();
    return serialized === "" ? base : `${base}?${serialized}`;
  }

  private async request<T>(
    method: HttpMethod,
    path: string,
    options: RequestOptions = {},
  ): Promise<T> {
    const headers: Record<string, string> = { Accept: "application/json" };
    if (this.token !== "") {
      headers.Authorization = `Bearer ${this.token}`;
    }
    if (options.body !== undefined) {
      headers["Content-Type"] = "application/json";
    }

    let response: Response;
    try {
      response = await fetch(this.buildUrl(path, options.query), {
        method,
        headers,
        body: options.body === undefined ? undefined : JSON.stringify(options.body),
        signal: options.signal,
        cache: "no-store",
      });
    } catch (error) {
      if (isAbortError(error)) {
        throw error;
      }
      throw new HubApiError(
        0,
        NETWORK_ERROR_CODE,
        "The Hub could not be reached. Check the Hub URL and that the Hub is running.",
      );
    }

    if (response.status === 204) {
      return undefined as T;
    }

    const text = await response.text();
    const parsed = parseJson(text);

    if (!response.ok) {
      const fields = readErrorFields(parsed);
      throw new HubApiError(
        response.status,
        fields.code ?? defaultErrorCode(response.status),
        fields.message ?? `The Hub returned status ${response.status}.`,
      );
    }

    if (parsed === undefined) {
      throw new HubApiError(
        response.status,
        "InvalidResponse",
        "The Hub returned a response that could not be read.",
      );
    }

    return parsed as T;
  }

  private async readError(response: Response): Promise<HubApiError> {
    let text = "";
    try {
      text = await response.text();
    } catch {
      text = "";
    }
    const fields = readErrorFields(parseJson(text));
    return new HubApiError(
      response.status,
      fields.code ?? defaultErrorCode(response.status),
      fields.message ?? `The Hub returned status ${response.status}.`,
    );
  }
}
