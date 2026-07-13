import { afterEach, describe, expect, it, vi } from "vitest";

import { HttpConfigClient } from "./http-config-client";
import { TauriConfigClient } from "./tauri-config-client";
import type { TauriBridge } from "./transport";

describe("SillyTavern compatibility clients", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("uses the dedicated HTTP preview and idempotent import contracts", async () => {
    const calls: Array<{ input: RequestInfo | URL; init?: RequestInit }> = [];
    vi.stubGlobal("fetch", async (input: RequestInfo | URL, init?: RequestInit) => {
      calls.push({ input, init });
      return Response.json(calls.length === 1 ? preview() : result(), {
        status: calls.length === 1 ? 200 : 201,
      });
    });
    const client = new HttpConfigClient("https://settings.example");
    const document = { prompts: [], prompt_order: [] };
    const parsed = await client.previewSillyTavernImport({
      document,
      sourceName: "roleplay.json",
      targetPresetId: null,
    });
    const imported = await client.applySillyTavernImport({
      document,
      sourceName: "roleplay.json",
      targetPresetId: null,
      expectedHeadVersionId: null,
    }, "st-import-key");
    expect(calls[0]?.input).toBe(
      "https://settings.example/v1/compatibility/sillytavern/preview",
    );
    expect(calls[1]?.input).toBe(
      "https://settings.example/v1/compatibility/sillytavern/import",
    );
    expect(calls[1]?.init?.headers).toMatchObject({ "idempotency-key": "st-import-key" });
    expect(parsed.textTransforms[0]).toMatchObject({
      id: "clean", surfaces: ["canonical"], placements: ["ai_output"],
    });
    expect(imported.version.spec.textTransforms).toBeDefined();
  });

  it("maps Tauri preview and apply to the same command DTOs", async () => {
    const calls: Array<{ operation: string; payload: unknown }> = [];
    const bridge: TauriBridge = {
      invoke: async <T>(operation: string, payload: unknown) => {
        calls.push({ operation, payload });
        return (operation === "preview_sillytavern_import" ? preview() : result()) as T;
      },
      listen: async () => () => undefined,
    };
    const client = new TauriConfigClient(bridge);
    await client.previewSillyTavernImport({ document: [], sourceName: "regex.json" });
    await client.applySillyTavernImport({ document: [], sourceName: "regex.json" }, "native-key");
    expect(calls.map((call) => call.operation)).toEqual([
      "preview_sillytavern_import",
      "apply_sillytavern_import",
    ]);
    expect(calls[1]?.payload).toMatchObject({ command: {
      sourceName: "regex.json", targetPresetId: null,
      expectedHeadVersionId: null, idempotencyKey: "native-key",
    } });
  });
});

const preview = () => ({
  compatibilityVersion: 1,
  kind: "open_ai",
  name: "Roleplay",
  sourceHash: `sha256:${"a".repeat(64)}`,
  contextSpec: { mode: "chat", items: [], textTransforms: [] },
  generation: { temperature: 0.8, maxOutputTokens: 512, stop: [], seed: null },
  providerExtensions: null,
  textTransforms: [{
    id: "clean", name: "Clean", scope: "preset", order: 0,
    findRegex: "/foo/g", replaceString: "bar", trimStrings: [],
    placements: ["ai_output"], surfaces: ["canonical"], disabled: false,
    runOnEdit: false, macroMode: "none", minDepth: null, maxDepth: null,
  }],
  inactiveFields: [],
  warnings: [],
});

const result = () => ({
  preview: preview(),
  preset: {
    id: "preset_1", name: "Roleplay", headVersionId: "presetver_1",
    createdAt: 1, updatedAt: 2,
  },
  version: {
    id: "presetver_1", presetId: "preset_1", versionNo: 1,
    semanticPolicyVersion: 1,
    spec: { mode: "chat", items: [], textTransforms: preview().textTransforms },
    contentHash: `sha256:${"b".repeat(64)}`, createdAt: 2,
  },
});
