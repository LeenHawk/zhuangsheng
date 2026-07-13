import { decodeCreateGraph, decodeGraphDraft, decodeGraphList, decodeGraphRevision } from "./decode-graphs";
import { DecodeError } from "./decode-error";
import { stringifyJsonExact } from "./exact-json";
import { decodeRolePlayCompatibility, decodeRolePlaySettings } from "./decode-roleplay";
import type { CreateGraphResult, GraphDraftView, GraphRevisionView, GraphSummary, JsonObject, RolePlayTemplateSpec } from "./graph-types";
import type { RolePlayCompatibilityView } from "./types";
import type { RolePlaySettingsView } from "./roleplay-types";
import { requestJson } from "./http-json";
import { createIdempotencyKey } from "./idempotency";

export interface GraphCommandOptions {
  idempotencyKey?: string;
  signal?: AbortSignal;
}

export class HttpGraphClient {
  constructor(private readonly baseUrl = "") {}

  async list(signal?: AbortSignal): Promise<GraphSummary[]> {
    return decodeGraphList(await this.request("/v1/graphs", { signal }));
  }

  async create(name: string, options: GraphCommandOptions = {}): Promise<CreateGraphResult> {
    return decodeCreateGraph(await this.request("/v1/graphs", {
      method: "POST",
      headers: this.commandHeaders(options.idempotencyKey),
      body: stringifyJsonExact({ name }),
      signal: options.signal,
    }));
  }

  async getDraft(graphId: string, signal?: AbortSignal): Promise<GraphDraftView> {
    return decodeGraphDraft(await this.request(`/v1/graphs/${encodeURIComponent(graphId)}/draft`, { signal }));
  }

  async getRevision(revisionId: string, signal?: AbortSignal): Promise<GraphRevisionView> {
    const revision = decodeGraphRevision(await this.request(
      `/v1/graph-revisions/${encodeURIComponent(revisionId)}`,
      { signal },
    ));
    if (revision.id !== revisionId) throw new DecodeError("graphRevision.id");
    return revision;
  }

  async getGraphRevision(
    graphId: string,
    revisionId: string,
    signal?: AbortSignal,
  ): Promise<GraphRevisionView> {
    const revision = decodeGraphRevision(await this.request(
      `/v1/graphs/${encodeURIComponent(graphId)}/revisions/${encodeURIComponent(revisionId)}`,
      { signal },
    ));
    if (revision.id !== revisionId || revision.graphId !== graphId) {
      throw new DecodeError("graphRevision");
    }
    return revision;
  }

  async updateDraft(
    graphId: string,
    revisionToken: string,
    document: JsonObject,
    options: GraphCommandOptions = {},
  ): Promise<GraphDraftView> {
    return decodeGraphDraft(await this.request(`/v1/graphs/${encodeURIComponent(graphId)}/draft`, {
      method: "PUT",
      headers: { ...this.commandHeaders(options.idempotencyKey), "if-match": `"${revisionToken}"` },
      body: stringifyJsonExact(document),
      signal: options.signal,
    }));
  }

  async apply(
    graphId: string,
    revisionToken: string,
    options: GraphCommandOptions = {},
  ): Promise<GraphRevisionView> {
    return decodeGraphRevision(await this.request(`/v1/graphs/${encodeURIComponent(graphId)}/apply`, {
      method: "POST",
      headers: { ...this.commandHeaders(options.idempotencyKey), "if-match": `"${revisionToken}"` },
      body: stringifyJsonExact({ operationTaxonomyVersion: 1, adapterDecoderVersion: 1 }),
      signal: options.signal,
    }));
  }

  async createRolePlayTemplate(
    name: string,
    channelId: string,
    presetId: string,
    options: GraphCommandOptions & RolePlayTemplateSpec = {},
  ): Promise<GraphRevisionView> {
    return decodeGraphRevision(await this.request("/v1/roleplay/templates", {
      method: "POST",
      headers: this.commandHeaders(options.idempotencyKey),
      body: stringifyJsonExact({
        name,
        channelId,
        presetId,
        generation: options.generation ?? null,
        extensions: options.extensions ?? null,
      }),
      signal: options.signal,
    }));
  }

  async getRolePlaySettings(
    revisionId: string,
    signal?: AbortSignal,
  ): Promise<RolePlaySettingsView> {
    return decodeRolePlaySettings(await this.request(
      `/v1/graph-revisions/${encodeURIComponent(revisionId)}/roleplay-settings`,
      { signal },
    ));
  }

  async getRolePlayCompatibility(
    revisionId: string,
    signal?: AbortSignal,
  ): Promise<RolePlayCompatibilityView> {
    return decodeRolePlayCompatibility(await this.request(
      `/v1/graph-revisions/${encodeURIComponent(revisionId)}/roleplay-compatibility`,
      { signal },
    ));
  }

  private commandHeaders(key?: string): Record<string, string> {
    return { "content-type": "application/json", "idempotency-key": key ?? createIdempotencyKey() };
  }

  private request(path: string, init: RequestInit): Promise<unknown> {
    return requestJson(this.baseUrl, path, init);
  }
}
