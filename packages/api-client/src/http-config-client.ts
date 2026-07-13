import { decodeChannel, decodeChannelModelDiscovery, decodeChannelRevision, decodeChannels, decodeContextPreset, decodeContextPresetPreview, decodeContextPresets, decodeContextPresetVersion } from "./decode-config";
import type { ChannelModelDiscoveryView, ChannelRevisionView, ChannelView, ContextPresetPreviewView, ContextPresetVersionView, ContextPresetView, DiscoveredChannelModel, PublishChannelInput, PublishPresetInput } from "./config-types";
import { DecodeError } from "./decode-error";
import { stringifyJsonExact } from "./exact-json";
import { requestJson } from "./http-json";
import { createIdempotencyKey } from "./idempotency";
import { buildDiscoveredModelRevisionSpec } from "./channel-model-selection";

export class HttpConfigClient {
  constructor(private readonly baseUrl = "") {}

  async listChannels(signal?: AbortSignal): Promise<ChannelView[]> {
    return decodeChannels(await requestJson(this.baseUrl, "/v1/channels", { signal }));
  }

  async createChannel(name: string, idempotencyKey = createIdempotencyKey()): Promise<ChannelView> {
    return decodeChannel(await this.command("/v1/channels", { name }, idempotencyKey));
  }

  async discoverModels(
    channelId: string,
    input: { revisionId?: string | null; operationKey?: Record<string, unknown> | null } = {},
    signal?: AbortSignal,
  ): Promise<ChannelModelDiscoveryView> {
    return decodeChannelModelDiscovery(await requestJson(
      this.baseUrl,
      `/v1/channels/${encodeURIComponent(channelId)}/model-discovery`,
      {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: stringifyJsonExact({
          revisionId: input.revisionId ?? null,
          operationKey: input.operationKey ?? null,
        }),
        signal,
      },
    ));
  }

  async getChannelRevision(revisionId: string, signal?: AbortSignal): Promise<ChannelRevisionView> {
    const revision = decodeChannelRevision(await requestJson(
      this.baseUrl,
      `/v1/channel-revisions/${encodeURIComponent(revisionId)}`,
      { signal },
    ));
    if (revision.id !== revisionId) throw new DecodeError("channelRevision.id");
    return revision;
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
    return decodeChannelRevision(await this.command(
      `/v1/channels/${encodeURIComponent(channelId)}/revisions`,
      { expectedHeadRevisionId: source.id, spec },
      idempotencyKey,
    ));
  }

  async publishChannel(channelId: string, input: PublishChannelInput, idempotencyKey = createIdempotencyKey()): Promise<ChannelRevisionView> {
    const operationKey = { operation: "generate_content", kind: input.providerKind };
    const credential = input.credentialSecretId
      ? { type: "secret", apiKeyRef: { scheme: "secret", id: input.credentialSecretId } }
      : { type: "none" };
    const spec = {
      operationTaxonomyVersion: 1,
      adapterDecoderVersion: 1,
      baseUrl: input.baseUrl,
      transportPolicy: { allowLoopbackHttp: input.allowLoopbackHttp, allowUnauthenticated: input.allowUnauthenticated },
      credential,
      operationKeys: [operationKey],
      modelCatalogs: [{ operationKey, policy: "allowlist", models: [{ id: input.modelId, capabilities: { structuredOutput: input.structuredOutput } }] }],
      capabilities: [],
    };
    return decodeChannelRevision(await this.command(`/v1/channels/${encodeURIComponent(channelId)}/revisions`, { expectedHeadRevisionId: input.expectedHeadRevisionId, spec }, idempotencyKey));
  }

  async listPresets(signal?: AbortSignal): Promise<ContextPresetView[]> {
    return decodeContextPresets(await requestJson(this.baseUrl, "/v1/context-presets", { signal }));
  }

  async getPreset(presetId: string, signal?: AbortSignal): Promise<ContextPresetView> {
    const preset = decodeContextPreset(await requestJson(
      this.baseUrl,
      `/v1/context-presets/${encodeURIComponent(presetId)}`,
      { signal },
    ));
    if (preset.id !== presetId) throw new DecodeError("contextPreset.id");
    return preset;
  }

  async createPreset(name: string, idempotencyKey = createIdempotencyKey()): Promise<ContextPresetView> {
    return decodeContextPreset(await this.command("/v1/context-presets", { name }, idempotencyKey));
  }

  async publishPreset(presetId: string, input: PublishPresetInput, idempotencyKey = createIdempotencyKey()): Promise<ContextPresetVersionView> {
    return decodeContextPresetVersion(await this.command(`/v1/context-presets/${encodeURIComponent(presetId)}/revisions`, input, idempotencyKey));
  }

  async getPresetVersion(versionId: string, signal?: AbortSignal): Promise<ContextPresetVersionView> {
    const version = decodeContextPresetVersion(await requestJson(
      this.baseUrl,
      `/v1/context-preset-versions/${encodeURIComponent(versionId)}`,
      { signal },
    ));
    if (version.id !== versionId) throw new DecodeError("contextPresetVersion.id");
    return version;
  }

  async previewPreset(presetId: string, versionId?: string | null, signal?: AbortSignal): Promise<ContextPresetPreviewView> {
    return decodeContextPresetPreview(await requestJson(
      this.baseUrl,
      `/v1/context-presets/${encodeURIComponent(presetId)}/preview`,
      {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: stringifyJsonExact({
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
        }),
        signal,
      },
    ));
  }

  private command(path: string, body: unknown, idempotencyKey: string): Promise<unknown> {
    return requestJson(this.baseUrl, path, { method: "POST", headers: { "content-type": "application/json", "idempotency-key": idempotencyKey }, body: stringifyJsonExact(body) });
  }
}
