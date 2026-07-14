import { afterEach, describe, expect, it, vi } from "vitest";

import { decodePluginCandidate } from "./decode-plugin";
import { HttpApiClient } from "./http-client";
import { TauriPluginClient } from "./tauri-plugin-client";

const manifest = {
  apiVersion: 1, id: "example.renderer", name: "Example", version: "1.0.0",
  description: null, minimumHostVersion: null, entrypoints: { uiWorker: "dist/plugin.js" },
  permissions: ["ui_message_read_display", "ui_message_decorate"],
  renderers: [{ id: "message", slot: "conversation_message_body", priority: 10, roles: [] }],
  dependencies: [], settingsSchema: null,
};
const candidate = {
  id: "candidate_1", plannedVersionId: "version_1", sourceUrl: "https://example.test/plugin.git",
  sourceRef: "main", credentialSecretId: null, credentialUsername: null,
  resolvedCommit: "1".repeat(40), treeHash: "sha256:tree", manifestHash: "sha256:manifest",
  manifest, currentVersionId: null, addedPermissions: manifest.permissions, createdAt: 1,
};
const installation = {
  pluginId: manifest.id, sourceUrl: candidate.sourceUrl, sourceRef: "main",
  credentialSecretId: null, credentialUsername: null, updatePolicy: "automatic", enabled: true,
  activeVersion: {
    id: "version_1", pluginId: manifest.id, version: manifest.version,
    resolvedCommit: candidate.resolvedCommit, treeHash: candidate.treeHash,
    manifestHash: candidate.manifestHash, manifest, installedAt: 2,
  },
  previousVersions: [], createdAt: 2, updatedAt: 2,
};

describe("plugin clients", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("decodes versioned renderer declarations", () => {
    expect(decodePluginCandidate(candidate).manifest.renderers[0]).toEqual({
      id: "message", slot: "conversation_message_body", priority: 10, roles: [],
    });
  });

  it("sends explicit permission approval when activating over HTTP", async () => {
    let call: { input: RequestInfo | URL; init?: RequestInit } | null = null;
    vi.stubGlobal("fetch", async (input: RequestInfo | URL, init?: RequestInit) => {
      call = { input, init }; return new Response(JSON.stringify(installation));
    });
    const decoded = decodePluginCandidate(candidate);
    await new HttpApiClient("https://host.test").plugins.activate(decoded, "automatic", "install-key");
    const request = call as unknown as { input: string; init: RequestInit };
    expect(request.input).toBe("https://host.test/v1/plugins/candidates/candidate_1/activate");
    expect(request.init.headers).toMatchObject({ "idempotency-key": "install-key" });
    expect(JSON.parse(request.init.body as string)).toMatchObject({
      approvedPermissions: manifest.permissions, updatePolicy: "automatic",
    });
  });

  it("uses exact Tauri operations for update checks", async () => {
    const calls: Array<{ operation: string; payload: unknown }> = [];
    const client = new TauriPluginClient({
      invoke: async <T,>(operation: string, payload: unknown) => {
        calls.push({ operation, payload }); return null as T;
      },
      listen: async () => () => undefined,
    });
    expect(await client.checkUpdate("example.renderer")).toBeNull();
    expect(calls).toEqual([{ operation: "check_plugin_update", payload: { pluginId: "example.renderer" } }]);
  });
});
