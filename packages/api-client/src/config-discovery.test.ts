import { afterEach, describe, expect, it, vi } from "vitest";

import { HttpConfigClient } from "./http-config-client";

describe("HttpConfigClient model discovery", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("discovers temporary models without publishing them", async () => {
    let path = "";
    vi.stubGlobal("fetch", async (input: RequestInfo | URL) => {
      path = String(input);
      return Response.json({
        channelId: "channel_1",
        channelRevisionId: "channelrev_1",
        operationKey: { operation: "list_models", kind: "open_ai" },
        models: [{ id: "gpt-a", name: null, contextWindow: null, maxOutputTokens: null }],
      });
    });
    const result = await new HttpConfigClient("https://settings.example")
      .discoverModels("channel_1");
    expect(path).toBe("https://settings.example/v1/channels/channel_1/model-discovery");
    expect(result.models).toEqual([
      { id: "gpt-a", name: null, contextWindow: null, maxOutputTokens: null },
    ]);
  });

  it("reads the exact pinned revision from its encoded canonical route", async () => {
    let path = "";
    vi.stubGlobal("fetch", async (input: RequestInfo | URL) => {
      path = String(input);
      return Response.json({ ...channelRevision(), id: "channelrev/1" });
    });

    const revision = await new HttpConfigClient("https://settings.example")
      .getChannelRevision("channelrev/1");

    expect(path).toBe("https://settings.example/v1/channel-revisions/channelrev%2F1");
    expect(revision.id).toBe("channelrev/1");
  });

  it("publishes an explicitly selected discovered model from the pinned revision", async () => {
    let requestBody = "";
    const source = channelRevision();
    vi.stubGlobal("fetch", async (_input: RequestInfo | URL, init?: RequestInit) => {
      requestBody = init?.body as string;
      const request = JSON.parse(requestBody);
      return Response.json({ ...source, id: "channelrev_2", revisionNo: 2, ...request.spec });
    });
    const discovery = {
      channelId: "channel_1",
      channelRevisionId: "channelrev_1",
      operationKey: { operation: "list_models", kind: "open_ai" },
      models: [{ id: "gpt-new", name: "GPT New", contextWindow: 1000, maxOutputTokens: 100 }],
    };

    const result = await new HttpConfigClient("https://settings.example")
      .publishDiscoveredModel("channel_1", source, discovery, discovery.models[0]!, true, "select-key");

    const body = JSON.parse(requestBody);
    expect(body.expectedHeadRevisionId).toBe("channelrev_1");
    expect(body.spec.credential).toEqual(source.credential);
    expect(body.spec.modelCatalogs[0].models).toEqual([{
      id: "gpt-new",
      name: "GPT New",
      contextWindow: 1000,
      maxOutputTokens: 100,
      capabilities: { structuredOutput: true },
    }]);
    expect(result.id).toBe("channelrev_2");
  });
});

const channelRevision = () => ({
  id: "channelrev_1", channelId: "channel_1", revisionNo: 1,
  operationTaxonomyVersion: 1 as const, adapterDecoderVersion: 1 as const,
  baseUrl: "https://api.example/v1",
  transportPolicy: { allowLoopbackHttp: false, allowUnauthenticated: false },
  credential: { type: "secret", apiKeyRef: { scheme: "secret", id: "provider-key" } },
  operationKeys: [{ operation: "generate_content", kind: "open_ai_responses" }],
  modelCatalogs: [{
    operationKey: { operation: "generate_content", kind: "open_ai_responses" },
    policy: "allowlist" as const,
    models: [{ id: "gpt-old", capabilities: { structuredOutput: true } }],
  }],
  capabilities: [], contentHash: "hash", createdAt: 1,
});
