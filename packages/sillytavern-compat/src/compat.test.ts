import { describe, expect, it } from "vitest";
import type {
  ContextPresetVersionView,
  ContextPresetView,
  GraphRevisionView,
  JsonObject,
  RolePlayTemplateSpec,
} from "@zhuangsheng/api-client";

import {
  createSillyTavernWorkflow,
  exportSillyTavernBundle,
  previewSillyTavernImport,
  testSillyTavernRegex,
} from "./index";

describe("frontend SillyTavern compatibility", () => {
  it("imports prompts, generation and scoped regex without backend-specific semantics", async () => {
    const preset = await previewSillyTavernImport({ document: openAiDocument() });
    const global = await previewSillyTavernImport({
      document: [regex("global", [1])], baseSpec: preset.contextSpec,
    });
    const character = await previewSillyTavernImport({
      document: { data: { extensions: { regex_scripts: [regex("character", [2])] } } },
      baseSpec: global.contextSpec,
    });
    expect(preset.generation).toMatchObject({ temperature: 0.8, maxOutputTokens: 512 });
    expect(character.textTransforms.map((rule) => [rule.sourceScope, rule.priority])).toEqual([
      ["global", 0], ["character", 1000], ["preset", 2000],
    ]);
    expect((character.contextSpec?.textTransforms as unknown[])[0]).not.toHaveProperty("sourceScope");
  });

  it("compiles ST replacement syntax before the generic runtime shape and round-trips export", async () => {
    const preview = await previewSillyTavernImport({ document: openAiDocument() });
    expect(preview.textTransforms[0]?.replaceString).toBe("<$0>");
    const tested = await testSillyTavernRegex({
      document: openAiDocument(), input: "foo foo", target: "assistant_output", surface: "canonical",
    });
    expect(tested).toEqual({ text: "<foo> <foo>", appliedRuleIds: ["preset"] });
    const bundle = await exportSillyTavernBundle("Roleplay", preview.contextSpec!, preview.generation, preview.providerExtensions);
    const imported = await previewSillyTavernImport({ document: bundle.documents[0]!.document });
    expect(imported.generation).toEqual(preview.generation);
    expect(imported.textTransforms[0]).toMatchObject({ id: "preset", targets: ["assistant_output"] });
  });

  it("keeps slash commands inactive instead of teaching the runtime STscript", async () => {
    const preview = await previewSillyTavernImport({ document: [regex("slash", [3])] });
    expect(preview.textTransforms[0]).toMatchObject({ targets: [], inactivePlacements: [3] });
    expect(preview.contextSpec?.textTransforms).toEqual([expect.not.objectContaining({ inactivePlacements: expect.anything() })]);
  });

  it("publishes only generic specs and forwards generation through the role-play API", async () => {
    const base = (await previewSillyTavernImport({ document: openAiDocument() })).contextSpec!;
    const preset: ContextPresetView = {
      id: "preset_1", name: "Existing", headVersionId: "version_1", createdAt: 1, updatedAt: 1,
    };
    const version: ContextPresetVersionView = {
      id: "version_1", presetId: preset.id, versionNo: 1, semanticPolicyVersion: 1,
      spec: base, contentHash: "hash_1", createdAt: 1,
    };
    const published = { ...version, id: "version_2", versionNo: 2 };
    const graph: GraphRevisionView = {
      id: "graphrev_1", graphId: "graph_1", revisionNo: 1,
      operationTaxonomyVersion: 1, adapterDecoderVersion: 1,
      definition: {}, contentHash: "graph_hash", createdAt: 1, warnings: [],
    };
    let rolePlayOptions: RolePlayTemplateSpec | null = null;
    const publishedSpecs: JsonObject[] = [];
    const workflow = createSillyTavernWorkflow({
      presets: () => [preset],
      versions: () => ({ [version.id]: version }),
      createPreset: async () => preset,
      publishPreset: async (_id, input) => { publishedSpecs.push(input.spec); return published; },
      createRolePlayTemplate: async (_name, _channel, _preset, options) => {
        rolePlayOptions = options;
        return graph;
      },
    });

    const result = await workflow.apply({
      document: openAiDocument(), targetPresetId: preset.id,
      expectedHeadVersionId: version.id, channelId: "channel_1",
    });

    expect(result.version.id).toBe("version_2");
    expect(publishedSpecs[0]?.textTransforms).toEqual([
      expect.not.objectContaining({ sourceScope: expect.anything(), inactivePlacements: expect.anything() }),
    ]);
    expect(rolePlayOptions).toMatchObject({
      generation: { temperature: 0.8, maxOutputTokens: 512 },
      extensions: { openai: { extraBody: { frequency_penalty: 0.2 } } },
    });
  });
});

function openAiDocument(): JsonObject {
  return {
    name: "Roleplay", temperature: 0.8, top_p: 0.9, openai_max_tokens: 512,
    frequency_penalty: 0.2,
    prompts: [
      { identifier: "main", name: "Main", role: "system", content: "Write a reply." },
      { identifier: "chatHistory", name: "History", marker: true },
    ],
    prompt_order: [{ character_id: 100001, order: [
      { identifier: "main", enabled: true }, { identifier: "chatHistory", enabled: true },
    ] }],
    extensions: { regex_scripts: [regex("preset", [2], "<{{match}}>")] },
  };
}

function regex(id: string, placement: number[], replacement = "bar"): JsonObject {
  return { id, scriptName: id, findRegex: "/foo/g", replaceString: replacement, trimStrings: [], placement, disabled: false, markdownOnly: false, promptOnly: false, runOnEdit: false, substituteRegex: 0, minDepth: null, maxDepth: null };
}
