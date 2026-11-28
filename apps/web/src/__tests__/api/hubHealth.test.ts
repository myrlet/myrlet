import { afterEach, describe, expect, it, vi } from "vitest";

import { probeHubHealth } from "../../api/client";

describe("public Hub health probe", () => {
  afterEach(() => vi.unstubAllGlobals());

  it.each([
    [" / ", "/health"],
    [" https://hub.example/prefix/// ", "https://hub.example/prefix/health"],
  ])("probes %j without a bearer token", async (input, expectedUrl) => {
    const fetch = vi.fn<typeof globalThis.fetch>().mockResolvedValue(
      Response.json({ version: "0.1.0" }),
    );
    vi.stubGlobal("fetch", fetch);

    await expect(probeHubHealth(input)).resolves.toEqual({ ok: true, version: "0.1.0" });
    const [[url, init]] = fetch.mock.calls;
    expect(url).toBe(expectedUrl);
    expect(new Headers(init?.headers).has("Authorization")).toBe(false);
  });

  it("treats an unreadable body as reachable and an HTTP failure as unreachable", async () => {
    vi.stubGlobal("fetch", vi.fn<typeof fetch>()
      .mockResolvedValueOnce(new Response("not JSON"))
      .mockResolvedValueOnce(new Response(null, { status: 503 })));

    await expect(probeHubHealth("")).resolves.toEqual({ ok: true });
    await expect(probeHubHealth("")).resolves.toEqual({ ok: false });
  });
});
