import {
  decodeChannel,
  decodeChannelModelDiscovery,
  decodeChannelRevision,
  decodeChannels,
  decodeContextPreset,
  decodeContextPresetPreview,
  decodeContextPresets,
  decodeContextPresetVersion,
} from "./decode-config";
import { decodeSillyTavernImportPreview, decodeSillyTavernImportResult } from "./decode-sillytavern";
import { decodeGraphRevision } from "./decode-graphs";
import { decodeRolePlaySettings } from "./decode-roleplay";
import { DecodeError } from "./decode-error";
import { createIdempotencyKey } from "./idempotency";
import type {
  ChannelRevisionView,
  ChannelModelDiscoveryView,
  ChannelView,
  ContextPresetPreviewView,
  ContextPresetVersionView,
  ContextPresetView,
  DiscoveredChannelModel,
  PublishChannelInput,
  PublishPresetInput,
  ApplySillyTavernImportInput,
  SillyTavernImportInput,
  SillyTavernImportPreviewView,
  SillyTavernImportResultView,
} from "./config-types";
import type { GraphRevisionView } from "./graph-types";
import type { RolePlaySettingsView } from "./roleplay-types";
import type { TauriBridge } from "./transport";
import { buildDiscoveredModelRevisionSpec } from "./channel-model-selection";

export class TauriConfigClient {
  constructor(private readonly bridge: TauriBridge) {}

  async listChannels(): Promise<ChannelView[]> {
    return decodeChannels(await this.bridge.invoke("list_channels", {}));
  }

  async createChannel(name: string, idempotencyKey = createIdempotencyKey()): Promise<ChannelView> {
    return decodeChannel(await this.bridge.invoke("create_channel", { command: { name, idempotencyKey } }));
  }

  async publishChannel(
    channelId: string,
    input: PublishChannelInput,
    idempotencyKey = createIdempotencyKey(),
  ): Promise<ChannelRevisionView> {
    const operationKey = { operation: "generate_content", kind: input.providerKind };
    const credential = input.credentialSecretId
      ? { type: "secret", apiKeyRef: { scheme: "secret", id: input.credentialSecretId } }
      : { type: "none" };
    const spec = {
      operationTaxonomyVersion: 1,
      adapterDecoderVersion: 1,
      baseUrl: input.baseUrl,
      transportPolicy: {
        allowLoopbackHttp: input.allowLoopbackHttp,
        allowUnauthenticated: input.allowUnauthenticated,
      },
      credential,
      operationKeys: [operationKey],
      modelCatalogs: [{
        operationKey,
        policy: "allowlist",
        models: [{ id: input.modelId, capabilities: { structuredOutput: input.structuredOutput } }],
      }],
      capabilities: [],
    };
    return decodeChannelRevision(await this.bridge.invoke("publish_channel_revision", { command: {
      channelId, expectedHeadRevisionId: input.expectedHeadRevisionId, spec, idempotencyKey,
    } }));
  }

  async getChannelRevision(revisionId: string): Promise<ChannelRevisionView> {
    const result = decodeChannelRevision(await this.bridge.invoke("get_channel_revision", { revisionId }));
    if (result.id !== revisionId) throw new DecodeError("channelRevision.id");
    return result;
  }

  async discoverModels(
    channelId: string,
    input: { revisionId?: string | null } = {},
  ): Promise<ChannelModelDiscoveryView> {
    return decodeChannelModelDiscovery(await this.bridge.invoke("discover_channel_models", { command: {
      channelId, revisionId: input.revisionId ?? null, operationKey: null,
    } }));
  }

  async publishDiscoveredModel(
    channelId: string,
    source: ChannelRevisionView,
    discovery: ChannelModelDiscoveryView,
    model: DiscoveredChannelModel,
    structuredOutput: boolean,
    idempotencyKey = createIdempotencyKey(),
  ): Promise<ChannelRevisionView> {
    const spec = buildDiscoveredModelRevisionSpec(
      channelId, source, discovery, model, structuredOutput,
    );
    return decodeChannelRevision(await this.bridge.invoke("publish_channel_revision", { command: {
      channelId, expectedHeadRevisionId: source.id, spec, idempotencyKey,
    } }));
  }

  async listPresets(): Promise<ContextPresetView[]> {
    return decodeContextPresets(await this.bridge.invoke("list_context_presets", {}));
  }

  async createPreset(name: string, idempotencyKey = createIdempotencyKey()): Promise<ContextPresetView> {
    return decodeContextPreset(await this.bridge.invoke("create_context_preset", { command: {
      name, idempotencyKey,
    } }));
  }

  async publishPreset(
    presetId: string,
    input: PublishPresetInput,
    idempotencyKey = createIdempotencyKey(),
  ): Promise<ContextPresetVersionView> {
    return decodeContextPresetVersion(await this.bridge.invoke("publish_context_preset_version", { command: {
      presetId, expectedHeadVersionId: input.expectedHeadVersionId, spec: input.spec, idempotencyKey,
    } }));
  }

  async getPresetVersion(versionId: string): Promise<ContextPresetVersionView> {
    const value = decodeContextPresetVersion(await this.bridge.invoke("get_context_preset_version", { versionId }));
    if (value.id !== versionId) throw new DecodeError("contextPresetVersion.id");
    return value;
  }

  async previewPreset(presetId: string, versionId?: string | null): Promise<ContextPresetPreviewView> {
    return decodeContextPresetPreview(await this.bridge.invoke("preview_context_preset", { command: {
      presetId,
      versionId: versionId ?? null,
      nodeInput: {},
      sampleBindings: {},
      budget: {
        contextWindowTokens: 16_384,
        reservedOutputTokens: 2_048,
        fixedRequestTokens: 0,
        safetyMarginTokens: 512,
        countSource: "estimate",
      },
    } }));
  }

  async previewSillyTavernImport(
    input: SillyTavernImportInput,
  ): Promise<SillyTavernImportPreviewView> {
    return decodeSillyTavernImportPreview(await this.bridge.invoke(
      "preview_sillytavern_import",
      { command: {
        document: input.document,
        sourceName: input.sourceName ?? null,
        targetPresetId: input.targetPresetId ?? null,
      } },
    ));
  }

  async applySillyTavernImport(
    input: ApplySillyTavernImportInput,
    idempotencyKey = createIdempotencyKey(),
  ): Promise<SillyTavernImportResultView> {
    return decodeSillyTavernImportResult(await this.bridge.invoke(
      "apply_sillytavern_import",
      { command: {
        document: input.document,
        sourceName: input.sourceName ?? null,
        targetPresetId: input.targetPresetId ?? null,
        expectedHeadVersionId: input.expectedHeadVersionId ?? null,
        idempotencyKey,
      } },
    ));
  }

  async createRolePlayTemplate(
    name: string,
    channelId: string,
    presetId: string,
    idempotencyKey = createIdempotencyKey(),
  ): Promise<GraphRevisionView> {
    return decodeGraphRevision(await this.bridge.invoke("create_roleplay_template", { command: {
      name, channelId, presetId, idempotencyKey,
    } }));
  }

  async getGraphRevision(revisionId: string): Promise<GraphRevisionView> {
    const value = decodeGraphRevision(await this.bridge.invoke("get_graph_revision", { revisionId }));
    if (value.id !== revisionId) throw new DecodeError("graphRevision.id");
    return value;
  }

  async getRolePlaySettings(revisionId: string): Promise<RolePlaySettingsView> {
    return decodeRolePlaySettings(await this.bridge.invoke("get_roleplay_settings", { revisionId }));
  }
}
