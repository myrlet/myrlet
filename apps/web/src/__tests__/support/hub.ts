import { vi } from "vitest";

/** A stubbed Hub endpoint: return a `Response` (possibly async) per request. */
export type FetchHandler = (url: string, init: RequestInit) => Response | Promise<Response>;

function readUrl(input: RequestInfo | URL): string {
  if (typeof input === "string") {
    return input;
  }
  if (input instanceof URL) {
    return input.toString();
  }
  return input.url;
}

/** Replace global `fetch` with `handler`; restore with `vi.unstubAllGlobals`. */
export function stubFetch(handler: FetchHandler): void {
  const mock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) =>
    handler(readUrl(input), init ?? {}),
  );
  vi.stubGlobal("fetch", mock);
}

export function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

/** A short, already-closed SSE body of the given frames. */
export function streamResponse(chunks: readonly string[]): Response {
  const encoder = new TextEncoder();
  const stream = new ReadableStream<Uint8Array>({
    start(controller) {
      for (const chunk of chunks) {
        controller.enqueue(encoder.encode(chunk));
      }
      controller.close();
    },
  });
  return new Response(stream, {
    status: 200,
    headers: { "Content-Type": "text/event-stream" },
  });
}

/** A Hub event stream that stays open, like a live connection with no news. */
export function idleEventStream(): Response {
  const stream = new ReadableStream<Uint8Array>({
    start() {
      // Intentionally never enqueues or closes: the SSE read stays pending.
    },
  });
  return new Response(stream, {
    status: 200,
    headers: { "Content-Type": "text/event-stream" },
  });
}
