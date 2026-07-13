import {
  createIdempotencyKey,
  stringifyJsonExact,
  type ContextPresetVersionView,
  type ContextPresetView,
  type GraphRevisionView,
  type JsonObject,
  type PublishPresetInput,
  type RolePlayTemplateSpec,
} from "@zhuangsheng/api-client";

import { exportSillyTavernBundle } from "./export";
import { previewSillyTavernImport } from "./import";
import { testSillyTavernRegex } from "./execute";
import type {
  ApplySillyTavernImportInput,
  SillyTavernExportBundleView,
  SillyTavernImportInput,
  SillyTavernImportPreviewView,
  SillyTavernImportResultView,
  SillyTavernRegexTestResultView,
  TestSillyTavernRegexInput,
} from "./types";

export interface SillyTavernWorkflowResources {
  presets(): readonly ContextPresetView[];
  versions(): Readonly<Record<string, ContextPresetVersionView>>;
  createPreset(name: string, idempotencyKey: string): Promise<ContextPresetView>;
  publishPreset(
    presetId: string,
    input: PublishPresetInput,
    idempotencyKey: string,
  ): Promise<ContextPresetVersionView>;
  createRolePlayTemplate(
    name: string,
    channelId: string,
    presetId: string,
    options: RolePlayTemplateSpec & { idempotencyKey: string },
  ): Promise<GraphRevisionView>;
}

export interface SillyTavernWorkflowActions {
  preview(input: SillyTavernImportInput): Promise<SillyTavernImportPreviewView>;
  apply(input: ApplySillyTavernImportInput): Promise<SillyTavernImportResultView>;
  test(input: TestSillyTavernRegexInput): Promise<SillyTavernRegexTestResultView>;
  export(versionId: string): Promise<SillyTavernExportBundleView>;
}

export function createSillyTavernWorkflow(
  resources: SillyTavernWorkflowResources,
): SillyTavernWorkflowActions {
  const idempotencyKeys = new Map<string, string>();
  const keyFor = (signature: string) => {
    let key = idempotencyKeys.get(signature);
    if (!key) {
      key = createIdempotencyKey();
      idempotencyKeys.set(signature, key);
    }
    return key;
  };

  return {
    preview: (input) => previewSillyTavernImport(withBaseSpec(input, resources)),
    test: (input) => testSillyTavernRegex(withBaseSpec(input, resources)),
    apply: (input) => applyImport(input, resources, keyFor),
    export: (versionId) => exportVersion(versionId, resources),
  };
}

async function applyImport(
  input: ApplySillyTavernImportInput,
  resources: SillyTavernWorkflowResources,
  keyFor: (signature: string) => string,
): Promise<SillyTavernImportResultView> {
  const target = targetResources(input.targetPresetId, resources);
  if (input.expectedHeadVersionId !== undefined
      && input.expectedHeadVersionId !== (target?.preset.headVersionId ?? null)) {
    throw new Error("target preset changed after preview; refresh before importing");
  }
  const prepared = { ...input, baseSpec: target?.version?.spec ?? null };
  const preview = await previewSillyTavernImport(prepared);
  const signature = stringifyJsonExact({
    document: input.document,
    sourceName: input.sourceName ?? null,
    targetPresetId: input.targetPresetId ?? null,
    expectedHeadVersionId: input.expectedHeadVersionId ?? null,
    channelId: input.channelId ?? null,
  });

  let preset = target?.preset;
  let version = target?.version;
  if (preview.contextSpec) {
    preset ??= await resources.createPreset(preview.name, keyFor(`${signature}:preset`));
    version = await resources.publishPreset(preset.id, {
      expectedHeadVersionId: target?.preset.headVersionId ?? null,
      spec: preview.contextSpec,
    }, keyFor(`${signature}:version`));
  }
  if (!preset || !version) {
    throw new Error("generation-only imports require an existing published context preset");
  }

  const graphRevision = input.channelId
    ? await resources.createRolePlayTemplate(preview.name, input.channelId, preset.id, {
        generation: preview.generation,
        extensions: preview.providerExtensions,
        idempotencyKey: keyFor(`${signature}:graph`),
      })
    : null;
  return { preview, preset, version, graphRevision };
}

function withBaseSpec<T extends SillyTavernImportInput>(
  input: T,
  resources: SillyTavernWorkflowResources,
): T {
  const target = targetResources(input.targetPresetId, resources);
  return { ...input, baseSpec: target?.version?.spec ?? null };
}

function targetResources(
  presetId: string | null | undefined,
  resources: SillyTavernWorkflowResources,
) {
  if (!presetId) return null;
  const preset = resources.presets().find((item) => item.id === presetId);
  if (!preset) throw new Error("target context preset is not loaded");
  if (!preset.headVersionId) return { preset, version: null };
  const version = resources.versions()[preset.headVersionId];
  if (!version) throw new Error("target context preset version is not loaded");
  return { preset, version };
}

function exportVersion(
  versionId: string,
  resources: SillyTavernWorkflowResources,
) {
  const version = resources.versions()[versionId];
  if (!version) throw new Error("context preset version is not loaded");
  const name = resources.presets().find((preset) => preset.id === version.presetId)?.name
    ?? version.presetId;
  return exportSillyTavernBundle(name, version.spec as JsonObject);
}
