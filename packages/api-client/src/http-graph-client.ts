import { decodeCreateGraph, decodeGraphDraft, decodeGraphList, decodeGraphRevision } from "./decode-graphs";
import type { CreateGraphResult, GraphDraftView, GraphRevisionView, GraphSummary, JsonObject } from "./graph-types";
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

  private commandHeaders(key?: string): Record<string, string> {
    return { "content-type": "application/json", "idempotency-key": key ?? createIdempotencyKey() };
  }

  private request(path: string, init: RequestInit): Promise<unknown> {
    return requestJson(this.baseUrl, path, init);
  }
}
