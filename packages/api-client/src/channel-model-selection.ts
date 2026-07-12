import type {
  ChannelModelDiscoveryView,
  ChannelRevisionView,
  DiscoveredChannelModel,
} from "./config-types";
import { DecodeError } from "./decode-error";
import type { JsonObject } from "./graph-types";

export function buildDiscoveredModelRevisionSpec(
  channelId: string,
  source: ChannelRevisionView,
  discovery: ChannelModelDiscoveryView,
  model: DiscoveredChannelModel,
  structuredOutput: boolean,
): JsonObject {
  if (source.channelId !== channelId || discovery.channelId !== channelId
    || source.id !== discovery.channelRevisionId) {
    throw new DecodeError("channelModelSelection");
  }
  const catalogs = source.modelCatalogs.filter((catalog) => {
    const operation = catalog.operationKey.operation;
    return operation === "generate_content" || operation === "stream_generate_content";
  });
  if (catalogs.length !== 1) throw new DecodeError("channelModelSelection.generationCatalog");
  const selected = {
    id: model.id,
    name: model.name,
    contextWindow: model.contextWindow,
    maxOutputTokens: model.maxOutputTokens,
    capabilities: { structuredOutput },
  };
  return {
    operationTaxonomyVersion: source.operationTaxonomyVersion,
    adapterDecoderVersion: source.adapterDecoderVersion,
    baseUrl: source.baseUrl,
    transportPolicy: source.transportPolicy,
    credential: source.credential,
    operationKeys: source.operationKeys,
    modelCatalogs: source.modelCatalogs.map((catalog) =>
      catalog === catalogs[0]
        ? { ...catalog, policy: "allowlist", models: [selected] }
        : catalog),
    capabilities: source.capabilities,
  };
}
