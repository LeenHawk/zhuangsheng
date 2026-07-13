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
      const url = String(input);
      return Response.json(url.endsWith("/regex/test") ? regexResult() : url.endsWith("/import") ? result() : preview(), {
        status: url.endsWith("/import") ? 201 : 200,
      });
    });
    const client = new HttpConfigClient("https://settings.example");
    const document = { prompts: [], prompt_order: [] };
    const parsed = await client.previewSillyTavernImport({
      document,
      sourceName: "roleplay.json",
      targetPresetId: null,
    });
    const tested = await client.testSillyTavernRegex({
      document, input: "foo", placement: "ai_output", surface: "canonical",
    });
    const imported = await client.applySillyTavernImport({
      document,
      sourceName: "roleplay.json",
      targetPresetId: null,
      expectedHeadVersionId: null,
      channelId: null,
    }, "st-import-key");
    expect(calls[0]?.input).toBe(
      "https://settings.example/v1/compatibility/sillytavern/preview",
    );
    expect(calls[1]?.input).toBe(
      "https://settings.example/v1/compatibility/sillytavern/regex/test",
    );
    expect(calls[2]?.input).toBe(
      "https://settings.example/v1/compatibility/sillytavern/import",
    );
    expect(calls[2]?.init?.headers).toMatchObject({ "idempotency-key": "st-import-key" });
    expect(parsed.textTransforms[0]).toMatchObject({
      id: "clean", surfaces: ["canonical"], placements: ["ai_output"],
    });
    expect(imported.version.spec.textTransforms).toBeDefined();
    expect(tested).toEqual(regexResult());
  });

  it("maps Tauri preview and apply to the same command DTOs", async () => {
    const calls: Array<{ operation: string; payload: unknown }> = [];
    const bridge: TauriBridge = {
      invoke: async <T>(operation: string, payload: unknown) => {
        calls.push({ operation, payload });
        return (operation === "preview_sillytavern_import" ? preview() : operation === "test_sillytavern_regex" ? regexResult() : result()) as T;
      },
      listen: async () => () => undefined,
    };
    const client = new TauriConfigClient(bridge);
    await client.previewSillyTavernImport({ document: [], sourceName: "regex.json" });
    await client.testSillyTavernRegex({ document: [], input: "foo", placement: "ai_output", surface: "canonical" });
    await client.applySillyTavernImport({ document: [], sourceName: "regex.json" }, "native-key");
    expect(calls.map((call) => call.operation)).toEqual([
      "preview_sillytavern_import",
      "test_sillytavern_regex",
      "apply_sillytavern_import",
    ]);
    expect(calls[2]?.payload).toMatchObject({ command: {
      sourceName: "regex.json", targetPresetId: null,
      expectedHeadVersionId: null, channelId: null, idempotencyKey: "native-key",
    } });
  });
});

const regexResult = () => ({ text: "bar", appliedRuleIds: ["clean"] });

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
  graphRevision: null,
});
