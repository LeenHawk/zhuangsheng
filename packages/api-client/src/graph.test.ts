import { afterEach, describe, expect, it, vi } from "vitest";

import { DecodeError } from "./decode-error";
import { decodeGraphDraft, projectGraphStructure } from "./decode-graphs";
import { decodeRolePlaySettings } from "./decode-roleplay";
import { HttpGraphClient } from "./http-graph-client";
import type { JsonObject } from "./graph-types";

const draftDocument: JsonObject = {
  graphId: "graph_1",
  name: "Role play",
  nodes: [
    { id: "input", kind: "input", name: "Message", isEntry: true, outputs: [{ name: "message" }] },
    { id: "reply", kind: "llm", name: "Reply", inputs: [{ name: "message" }], outputs: [{ name: "reply" }], config: { future: true } },
  ],
  edges: [{ from: { nodeId: "input", output: "message" }, to: { nodeId: "reply", input: "message" } }],
  futureTopLevelField: { preserved: true },
};

describe("graph decoders", () => {
  it("keeps the canonical document intact while projecting safe structure", () => {
    const view = decodeGraphDraft({
      graphId: "graph_1",
      document: draftDocument,
      revisionToken: "draftrev_1",
      updatedAt: 10,
    });
    expect(view.document).toEqual(draftDocument);
    expect(projectGraphStructure(view.document)).toEqual({
      nodes: [
        { id: "input", kind: "input", name: "Message", isEntry: true, inputs: [], outputs: [{ name: "message" }] },
        { id: "reply", kind: "llm", name: "Reply", isEntry: false, inputs: [{ name: "message" }], outputs: [{ name: "reply" }] },
      ],
      edges: [{
        id: "input:message->reply:message:0",
        source: "input",
        sourcePort: "message",
        target: "reply",
        targetPort: "message",
      }],
    });
  });

  it("fails closed when the draft identity disagrees", () => {
    expect(() => decodeGraphDraft({
      graphId: "graph_2",
      document: draftDocument,
      revisionToken: "draftrev_1",
      updatedAt: 10,
    })).toThrow(DecodeError);
  });

  it("decodes the server-projected role play settings and pins profile version", () => {
    const settings = {
      profileVersion: 1,
      revisionId: "graphrev_1",
      primaryLlmNodeId: "reply",
      compatibility: { mode: "editable", profileVersion: 1, editableFields: ["model"] },
      model: {
        channelId: "channel_1",
        modelId: "model_1",
        modelName: null,
        operationKey: { operation: "generate_content", kind: "open_ai_responses" },
      },
      generation: { temperature: 0.7, stop: [] },
      streaming: { enabled: true, audience: "user", persistChunks: false },
      contextPresetId: "preset_1",
    };
    expect(decodeRolePlaySettings(settings).model.channelId).toBe("channel_1");
    expect(() => decodeRolePlaySettings({ ...settings, profileVersion: 2 })).toThrow(DecodeError);
    expect(() => decodeRolePlaySettings({
      ...settings,
      streaming: { ...settings.streaming, audience: "future" },
    })).toThrow(DecodeError);
  });
});

describe("HttpGraphClient", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("sends exact CAS and stable command headers for save and apply", async () => {
    const calls: Array<{ input: RequestInfo | URL; init?: RequestInit }> = [];
    vi.stubGlobal("fetch", async (input: RequestInfo | URL, init?: RequestInit) => {
      calls.push({ input, init });
      const payload = calls.length === 1
        ? { graphId: "graph_1", document: draftDocument, revisionToken: "draftrev_2", updatedAt: 11 }
        : {
            id: "graphrev_1",
            graphId: "graph_1",
            revisionNo: 1,
            operationTaxonomyVersion: 1,
            adapterDecoderVersion: 1,
            definition: { schemaVersion: 1, ...draftDocument },
            contentHash: "sha256:abc",
            createdAt: 12,
            warnings: [],
          };
      return new Response(JSON.stringify(payload), { status: 200 });
    });
    const client = new HttpGraphClient("https://studio.example");

    await client.updateDraft("graph/1", "draftrev_1", draftDocument, { idempotencyKey: "save-key" });
    await client.apply("graph/1", "draftrev_2", { idempotencyKey: "apply-key" });

    expect(calls.map((call) => call.input)).toEqual([
      "https://studio.example/v1/graphs/graph%2F1/draft",
      "https://studio.example/v1/graphs/graph%2F1/apply",
    ]);
    expect(calls[0]?.init?.headers).toMatchObject({ "if-match": '"draftrev_1"', "idempotency-key": "save-key" });
    expect(calls[1]?.init?.headers).toMatchObject({ "if-match": '"draftrev_2"', "idempotency-key": "apply-key" });
    expect(JSON.parse(calls[1]?.init?.body as string)).toEqual({ operationTaxonomyVersion: 1, adapterDecoderVersion: 1 });
  });

  it("loads the exact immutable revision path and rejects identity drift", async () => {
    const requested: Array<RequestInfo | URL> = [];
    vi.stubGlobal("fetch", async (input: RequestInfo | URL) => {
      requested.push(input);
      return new Response(JSON.stringify({
        id: requested.length === 1 ? "graphrev/1" : "graphrev_other",
        graphId: "graph_1",
        revisionNo: 1,
        operationTaxonomyVersion: 1,
        adapterDecoderVersion: 1,
        definition: { nodes: [], edges: [] },
        contentHash: "sha256:abc",
        createdAt: 12,
        warnings: [],
      }), { status: 200 });
    });
    const client = new HttpGraphClient("https://studio.example");

    await expect(client.getRevision("graphrev/1")).resolves.toMatchObject({ id: "graphrev/1" });
    await expect(client.getRevision("graphrev/1")).rejects.toBeInstanceOf(DecodeError);
    expect(requested).toEqual([
      "https://studio.example/v1/graph-revisions/graphrev%2F1",
      "https://studio.example/v1/graph-revisions/graphrev%2F1",
    ]);
  });

  it("loads a revision through its graph ownership route", async () => {
    let requested: RequestInfo | URL | null = null;
    vi.stubGlobal("fetch", async (input: RequestInfo | URL) => {
      requested = input;
      return Response.json({
        id: "graphrev/1", graphId: "graph/1", revisionNo: 1,
        operationTaxonomyVersion: 1, adapterDecoderVersion: 1,
        definition: { nodes: [], edges: [] }, contentHash: "sha256:abc",
        createdAt: 12, warnings: [],
      });
    });
    await new HttpGraphClient("https://studio.example")
      .getGraphRevision("graph/1", "graphrev/1");
    expect(requested).toBe(
      "https://studio.example/v1/graphs/graph%2F1/revisions/graphrev%2F1",
    );
  });

  it("creates a user-mode role play template through the server facade", async () => {
    let call: { input: RequestInfo | URL; init?: RequestInit } | null = null;
    vi.stubGlobal("fetch", async (input: RequestInfo | URL, init?: RequestInit) => {
      call = { input, init };
      return new Response(JSON.stringify({ id: "graphrev_1", graphId: "graph_1", revisionNo: 1, operationTaxonomyVersion: 1, adapterDecoderVersion: 1, definition: {}, contentHash: "hash", createdAt: 1, warnings: [] }), { status: 201 });
    });
    await new HttpGraphClient("https://role.example").createRolePlayTemplate("Alice", "channel_1", "preset_1", { idempotencyKey: "template-key" });
    const request = call as unknown as { input: RequestInfo | URL; init: RequestInit };
    expect(request.input).toBe("https://role.example/v1/roleplay/templates");
    expect(request.init.headers).toMatchObject({ "idempotency-key": "template-key" });
    expect(JSON.parse(request.init.body as string)).toEqual({ name: "Alice", channelId: "channel_1", presetId: "preset_1" });
  });

  it("reads role play settings without making the browser inspect graph definitions", async () => {
    let requested: RequestInfo | URL | null = null;
    vi.stubGlobal("fetch", async (input: RequestInfo | URL) => {
      requested = input;
      return new Response(JSON.stringify({
        profileVersion: 1,
        revisionId: "graphrev/1",
        primaryLlmNodeId: "reply",
        compatibility: { mode: "editable", profileVersion: 1, editableFields: [] },
        model: {
          channelId: "channel_1",
          modelId: "model_1",
          modelName: "Model One",
          operationKey: { operation: "generate_content", kind: "open_ai_responses" },
        },
        generation: null,
        streaming: null,
        contextPresetId: null,
      }), { status: 200 });
    });

    const result = await new HttpGraphClient("https://role.example")
      .getRolePlaySettings("graphrev/1");

    expect(requested).toBe(
      "https://role.example/v1/graph-revisions/graphrev%2F1/roleplay-settings",
    );
    expect(result.model.modelName).toBe("Model One");
  });

  it("reads the server-owned role play compatibility projection", async () => {
    let requested: RequestInfo | URL | null = null;
    vi.stubGlobal("fetch", async (input: RequestInfo | URL) => {
      requested = input;
      return Response.json({
        mode: "partial",
        profileVersion: 1,
        editableFields: ["model"],
        lockedReasons: ["custom_context"],
      });
    });

    const result = await new HttpGraphClient("https://role.example")
      .getRolePlayCompatibility("graphrev/1");

    expect(requested).toBe(
      "https://role.example/v1/graph-revisions/graphrev%2F1/roleplay-compatibility",
    );
    expect(result.mode).toBe("partial");
  });
});
