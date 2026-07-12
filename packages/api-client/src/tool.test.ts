import { afterEach, describe, expect, it, vi } from "vitest";

import { HttpToolClient } from "./http-tool-client";

describe("HttpToolClient", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("reads only model-facing descriptor metadata", async () => {
    let requested: RequestInfo | URL | null = null;
    vi.stubGlobal("fetch", async (input: RequestInfo | URL) => {
      requested = input;
      return Response.json([{
        toolId: "weather/lookup",
        version: "1",
        name: "get_weather",
        description: "Look up current weather",
        inputSchema: {
          schemaVersion: 1,
          dialect: "https://json-schema.org/draft/2020-12/schema",
          validationProfileVersion: 1,
          formatPolicyVersion: 1,
          document: { type: "object" },
          limits: {},
        },
      }]);
    });

    const descriptors = await new HttpToolClient("https://studio.example").listDescriptors();
    expect(requested).toBe("https://studio.example/v1/tools/descriptors");
    expect(descriptors[0]).toMatchObject({ toolId: "weather/lookup", name: "get_weather" });
    expect(descriptors[0]).not.toHaveProperty("executorKey");
  });
});
