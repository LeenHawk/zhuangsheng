import { decodeMemoryProposal, decodeMemoryProposalList, decodeMemoryRecord, decodeMemorySearch } from "./decode-memory";
import { DecodeError } from "./decode-error";
import { stringifyJsonExact } from "./exact-json";
import { requestJson } from "./http-json";
import { createIdempotencyKey } from "./idempotency";
import type { MemoryProposalCursor, MemoryProposalListView, MemoryProposalStatus, MemoryProposalView, MemoryRecordStatus, MemoryRecordView, MemorySearchView, ProposeMemoryInput } from "./memory-types";

export class HttpMemoryClient {
  constructor(private readonly baseUrl = "") {}

  async listProposals(scopeId: string, status?: MemoryProposalStatus, cursor?: MemoryProposalCursor, signal?: AbortSignal): Promise<MemoryProposalListView> {
    const query = new URLSearchParams({ scopeId, limit: "50" });
    if (status) query.set("status", status);
    if (cursor) { query.set("beforeUpdatedAt", String(cursor.updatedAt)); query.set("beforeId", cursor.id); }
    return decodeMemoryProposalList(await requestJson(this.baseUrl, `/v1/memory-proposals?${query}`, { signal }));
  }

  async search(scopeId: string, status: Extract<MemoryRecordStatus, "active" | "obsolete">, signal?: AbortSignal): Promise<MemorySearchView> {
    return decodeMemorySearch(await requestJson(this.baseUrl, "/v1/memory-search", { method: "POST", headers: { "content-type": "application/json" }, body: stringifyJsonExact({ scopeId, text: null, tags: [], status, limit: 100 }), signal }));
  }

  async get(memoryId: string, signal?: AbortSignal): Promise<MemoryRecordView> {
    const record = decodeMemoryRecord(await requestJson(
      this.baseUrl,
      `/v1/memories/${encodeURIComponent(memoryId)}`,
      { signal },
    ));
    if (record.id !== memoryId) throw new DecodeError("memoryRecord.id");
    return record;
  }

  async propose(input: ProposeMemoryInput): Promise<MemoryProposalView> {
    const { idempotencyKey, ...proposal } = input;
    return decodeMemoryProposal(await this.command("/v1/memory-proposals", { ...proposal, requestedBy: { kind: "user", id: "local-user" }, schemaVersion: 1, policyVersion: 1, originRunId: null, originNodeInstanceId: null }, idempotencyKey));
  }

  async decide(proposalId: string, expectedStatus: MemoryProposalStatus, decision: "approve" | "reject", idempotencyKey = createIdempotencyKey()): Promise<MemoryProposalView> {
    return decodeMemoryProposal(await this.command(`/v1/memory-proposals/${encodeURIComponent(proposalId)}/decision`, { expectedStatus, decision, actor: { kind: "user", id: "local-user" } }, idempotencyKey));
  }

  async apply(proposalId: string, idempotencyKey = createIdempotencyKey()): Promise<MemoryProposalView> {
    return decodeMemoryProposal(await this.command(`/v1/memory-proposals/${encodeURIComponent(proposalId)}/apply`, { expectedStatus: "approved" }, idempotencyKey));
  }

  private command(path: string, body: unknown, key = createIdempotencyKey()): Promise<unknown> {
    return requestJson(this.baseUrl, path, { method: "POST", headers: { "content-type": "application/json", "idempotency-key": key }, body: stringifyJsonExact(body) });
  }
}
