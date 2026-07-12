import { afterEach, describe, expect, it, vi } from "vitest";

import { DecodeError } from "./decode-error";
import { decodeChannelRevision, decodeContextPresetVersion } from "./decode-config";
import { HttpConfigClient } from "./http-config-client";
import { HttpSecretClient } from "./http-secret-client";

describe("HttpConfigClient", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("publishes an exact allowlisted generation channel with a SecretRef", async () => {
    let call: { input: RequestInfo | URL; init?: RequestInit } | null = null;
    vi.stubGlobal("fetch", async (input: RequestInfo | URL, init?: RequestInit) => {
      call = { input, init };
      return new Response(JSON.stringify({ id: "channelrev_1", channelId: "channel_1", revisionNo: 1, operationTaxonomyVersion: 1, adapterDecoderVersion: 1, baseUrl: "https://api.example/v1", contentHash: "hash", createdAt: 1 }), { status: 201 });
    });
    const client = new HttpConfigClient("https://settings.example");
    await client.publishChannel("channel_1", {
      expectedHeadRevisionId: null,
      baseUrl: "https://api.example/v1",
      providerKind: "open_ai_responses",
      modelId: "gpt-roleplay",
      credentialSecretId: "provider-key",
      allowLoopbackHttp: false,
      allowUnauthenticated: false,
    }, "publish-key");

    expect(call).not.toBeNull();
    const request = call as unknown as { input: RequestInfo | URL; init: RequestInit };
    expect(request.input).toBe("https://settings.example/v1/channels/channel_1/revisions");
    expect(request.init.headers).toMatchObject({ "idempotency-key": "publish-key" });
    const body = JSON.parse(request.init.body as string);
    expect(body.expectedHeadRevisionId).toBeNull();
    expect(body.spec.credential).toEqual({ type: "secret", apiKeyRef: { scheme: "secret", id: "provider-key" } });
    expect(body.spec.modelCatalogs[0]).toMatchObject({ policy: "allowlist", models: [{ id: "gpt-roleplay", capabilities: {} }] });
    expect(body.spec.operationKeys).toEqual([{ operation: "generate_content", kind: "open_ai_responses" }]);
  });

  it("publishes the complete canonical ContextPreset document", async () => {
    let body: unknown;
    vi.stubGlobal("fetch", async (_input: RequestInfo | URL, init?: RequestInit) => {
      body = JSON.parse(init?.body as string);
      return new Response(JSON.stringify({ id: "presetver_1", presetId: "preset_1", versionNo: 1, semanticPolicyVersion: 1, spec: (body as { spec: unknown }).spec, contentHash: "hash", createdAt: 1 }), { status: 201 });
    });
    const spec = { mode: "chat", items: [{ id: "character", source: { type: "literal", text: "You are Alice." } }] };
    await new HttpConfigClient().publishPreset("preset_1", { expectedHeadVersionId: null, spec }, "preset-key");
    expect(body).toEqual({ expectedHeadVersionId: null, spec });
  });

  it("rejects config revisions with unsupported semantic versions", () => {
    expect(() => decodeChannelRevision({
      id: "channelrev_1",
      channelId: "channel_1",
      revisionNo: 1,
      operationTaxonomyVersion: 2,
      adapterDecoderVersion: 1,
      baseUrl: "https://api.example/v1",
      contentHash: "hash",
      createdAt: 1,
    })).toThrow(DecodeError);
    expect(() => decodeContextPresetVersion({
      id: "presetver_1",
      presetId: "preset_1",
      versionNo: 1,
      semanticPolicyVersion: 2,
      spec: {},
      contentHash: "hash",
      createdAt: 1,
    })).toThrow(DecodeError);
  });
});

describe("HttpSecretClient metadata commands", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("puts plaintext only in the dedicated request and returns metadata", async () => {
    let requestBody: unknown;
    vi.stubGlobal("fetch", async (_input: RequestInfo | URL, init?: RequestInit) => {
      requestBody = JSON.parse(init?.body as string);
      return new Response(JSON.stringify({ secretRef: { scheme: "secret", id: "provider/key" }, name: "Provider", kind: "api_key", createdAt: 1, updatedAt: 2 }), { status: 200 });
    });
    const client = new HttpSecretClient("");
    const result = await client.put({ secretId: "provider/key", name: "Provider", kind: "api_key", value: "plaintext-value", sessionId: "session_1", idempotencyKey: "put-key" });
    expect(requestBody).toEqual({ name: "Provider", kind: "api_key", value: "plaintext-value", sessionId: "session_1" });
    expect(result).toEqual({ secretRef: { scheme: "secret", id: "provider/key" }, name: "Provider", kind: "api_key", createdAt: 1, updatedAt: 2 });
    expect(JSON.stringify(result)).not.toContain("plaintext-value");
  });
});
