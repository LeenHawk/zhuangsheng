import {
  decodeCreateGraph,
  decodeGraphDraft,
  decodeGraphList,
  decodeGraphRevision,
} from "./decode-graphs";
import { DecodeError } from "./decode-error";
import { createIdempotencyKey } from "./idempotency";
import type {
  CreateGraphResult,
  GraphDraftView,
  GraphRevisionView,
  GraphSummary,
  JsonObject,
} from "./graph-types";
import type { TauriBridge } from "./transport";

export class TauriGraphClient {
  constructor(private readonly bridge: TauriBridge) {}

  async list(): Promise<GraphSummary[]> {
    return decodeGraphList(await this.bridge.invoke("list_graphs", {}));
  }

  async create(name: string, idempotencyKey = createIdempotencyKey()): Promise<CreateGraphResult> {
    return decodeCreateGraph(await this.bridge.invoke("create_graph", { command: {
      name, idempotencyKey,
    } }));
  }

  async getDraft(graphId: string): Promise<GraphDraftView> {
    const value = decodeGraphDraft(await this.bridge.invoke("get_graph_draft", { graphId }));
    if (value.graphId !== graphId) throw new DecodeError("graphDraft.graphId");
    return value;
  }

  async updateDraft(
    graphId: string,
    revisionToken: string,
    document: JsonObject,
    idempotencyKey = createIdempotencyKey(),
  ): Promise<GraphDraftView> {
    return decodeGraphDraft(await this.bridge.invoke("update_graph_draft", { command: {
      graphId, expectedRevisionToken: revisionToken, document, idempotencyKey,
    } }));
  }

  async apply(
    graphId: string,
    revisionToken: string,
    idempotencyKey = createIdempotencyKey(),
  ): Promise<GraphRevisionView> {
    return decodeGraphRevision(await this.bridge.invoke("apply_graph", { command: {
      graphId,
      expectedRevisionToken: revisionToken,
      operationTaxonomyVersion: 1,
      adapterDecoderVersion: 1,
      idempotencyKey,
    } }));
  }

  async getRevision(revisionId: string): Promise<GraphRevisionView> {
    const value = decodeGraphRevision(await this.bridge.invoke("get_graph_revision", { revisionId }));
    if (value.id !== revisionId) throw new DecodeError("graphRevision.id");
    return value;
  }

  async getGraphRevision(graphId: string, revisionId: string): Promise<GraphRevisionView> {
    const value = decodeGraphRevision(await this.bridge.invoke("get_graph_revision_for_graph", {
      graphId, revisionId,
    }));
    if (value.id !== revisionId || value.graphId !== graphId) {
      throw new DecodeError("graphRevision");
    }
    return value;
  }
}
