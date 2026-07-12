import { decodeCreateGraph, decodeGraphDraft, decodeGraphList, decodeGraphRevision } from "./decode-graphs";
import { decodeRolePlaySettings } from "./decode-roleplay";
import type { CreateGraphResult, GraphDraftView, GraphRevisionView, GraphSummary, JsonObject } from "./graph-types";
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
      body: JSON.stringify({ name }),
      signal: options.signal,
    }));
  }

  async getDraft(graphId: string, signal?: AbortSignal): Promise<GraphDraftView> {
    return decodeGraphDraft(await this.request(`/v1/graphs/${encodeURIComponent(graphId)}/draft`, { signal }));
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
      body: JSON.stringify(document),
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
      body: JSON.stringify({ operationTaxonomyVersion: 1, adapterDecoderVersion: 1 }),
      signal: options.signal,
    }));
  }

  async createRolePlayTemplate(
    name: string,
    channelId: string,
    presetId: string,
    options: GraphCommandOptions = {},
  ): Promise<GraphRevisionView> {
    return decodeGraphRevision(await this.request("/v1/roleplay/templates", {
      method: "POST",
      headers: this.commandHeaders(options.idempotencyKey),
      body: JSON.stringify({ name, channelId, presetId }),
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

  private commandHeaders(key?: string): Record<string, string> {
    return { "content-type": "application/json", "idempotency-key": key ?? createIdempotencyKey() };
  }

  private request(path: string, init: RequestInit): Promise<unknown> {
    return requestJson(this.baseUrl, path, init);
  }
}
