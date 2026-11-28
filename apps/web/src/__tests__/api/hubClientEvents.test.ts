/**
 * Fetch-based SSE transport: bearer and resume headers, frame parsing, and the
 * terminal outcomes the Hub can report.
 */

import { describe, expect, it, vi } from "vitest";

import { HubClient } from "../../api/client";
import type { HubEventEnvelope } from "../../api/types";
import { jsonResponse, streamResponse, stubFetch } from "../support/hub";

describe("HubClient.openEvents", () => {
  const noopHandlers = { onEvent: () => undefined };

  it("sends bearer and Last-Event-ID headers and parses streamed frames", async () => {
    let seenUrl = "";
    let seenInit: RequestInit = {};
    stubFetch((url, init) => {
      seenUrl = url;
      seenInit = init;
      return streamResponse([
        ": keepalive\n\n",
        'id: 7\ndata: {"seq":7,"kind":"task.created","createdAtMs":1,"payload":{"taskId":"t1"}}\n\n',
        'id: 8\ndata: {"seq":8,"kind":"task.updated","createdAtMs":2,"payload":null}\n\n',
      ]);
    });

    const client = new HubClient({ baseUrl: "https://hub.test", token: "test-token" });
    const events: HubEventEnvelope[] = [];
    const onOpen = vi.fn();

    const outcome = await client.openEvents(
      42,
      new AbortController().signal,
      { ...noopHandlers, onOpen, onEvent: (event) => events.push(event) },
    );

    expect(outcome).toBe("ended");
    expect(onOpen).toHaveBeenCalledTimes(1);
    expect(seenUrl).toBe("https://hub.test/v1/events");

    const headers = seenInit.headers as Record<string, string>;
    expect(headers.Authorization).toBe("Bearer test-token");
    expect(headers["Last-Event-ID"]).toBe("42");
    expect(headers.Accept).toBe("text/event-stream");

    expect(events.map((event) => event.seq)).toEqual([7, 8]);
    expect(events[0]?.kind).toBe("task.created");
  });

  it("reports an expired cursor on HTTP 410", async () => {
    stubFetch(() =>
      jsonResponse({ error: "CursorExpired", message: "Cursor fell outside retention." }, 410),
    );

    const onCursorExpired = vi.fn();
    const onUnauthorized = vi.fn();
    const client = new HubClient({ baseUrl: "", token: "test-token" });

    const outcome = await client.openEvents(null, new AbortController().signal, {
      ...noopHandlers,
      onCursorExpired,
      onUnauthorized,
    });

    expect(outcome).toBe("cursor_expired");
    expect(onCursorExpired).toHaveBeenCalledTimes(1);
    expect(onUnauthorized).not.toHaveBeenCalled();
  });

  it("reports an unauthorized session on HTTP 401", async () => {
    stubFetch(() => jsonResponse({ error: "Unauthorized", message: "Invalid token." }, 401));

    const onCursorExpired = vi.fn();
    const onUnauthorized = vi.fn();
    const client = new HubClient({ baseUrl: "", token: "bad-token" });

    const outcome = await client.openEvents(null, new AbortController().signal, {
      ...noopHandlers,
      onCursorExpired,
      onUnauthorized,
    });

    expect(outcome).toBe("unauthorized");
    expect(onUnauthorized).toHaveBeenCalledTimes(1);
    expect(onCursorExpired).not.toHaveBeenCalled();
  });
});
