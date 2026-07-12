import {
  decodeMemoryProposal,
  decodeMemoryProposalList,
  decodeMemoryRecord,
  decodeMemorySearch,
} from "./decode-memory";
import { DecodeError } from "./decode-error";
import { createIdempotencyKey } from "./idempotency";
import type {
  MemoryProposalCursor,
  MemoryProposalListView,
  MemoryProposalStatus,
  MemoryProposalView,
  MemoryRecordStatus,
  MemoryRecordView,
  MemorySearchView,
  ProposeMemoryInput,
} from "./memory-types";
import type { TauriBridge } from "./transport";

export class TauriMemoryClient {
  constructor(private readonly bridge: TauriBridge) {}

  async listProposals(
    scopeId: string,
    status?: MemoryProposalStatus,
    cursor?: MemoryProposalCursor,
  ): Promise<MemoryProposalListView> {
    return decodeMemoryProposalList(await this.bridge.invoke("list_memory_proposals", { command: {
      scopeId, status: status ?? null, limit: 50, cursor: cursor ?? null,
    } }));
  }

  async search(
    scopeId: string,
    status: Extract<MemoryRecordStatus, "active" | "obsolete">,
  ): Promise<MemorySearchView> {
    return decodeMemorySearch(await this.bridge.invoke("search_memory", { command: {
      scopeId, text: null, tags: [], status, limit: 100,
    } }));
  }

  async get(memoryId: string): Promise<MemoryRecordView> {
    const value = decodeMemoryRecord(await this.bridge.invoke("get_memory_record", { memoryId }));
    if (value.id !== memoryId) throw new DecodeError("memoryRecord.id");
    return value;
  }

  async propose(input: ProposeMemoryInput): Promise<MemoryProposalView> {
    return decodeMemoryProposal(await this.bridge.invoke("propose_memory_change", { input: {
      ...input,
      idempotencyKey: input.idempotencyKey ?? createIdempotencyKey(),
    } }));
  }

  async decide(
    proposalId: string,
    expectedStatus: MemoryProposalStatus,
    decision: "approve" | "reject",
    idempotencyKey = createIdempotencyKey(),
  ): Promise<MemoryProposalView> {
    return decodeMemoryProposal(await this.bridge.invoke("decide_memory_proposal", { input: {
      proposalId, expectedStatus, decision, idempotencyKey,
    } }));
  }

  async apply(
    proposalId: string,
    idempotencyKey = createIdempotencyKey(),
  ): Promise<MemoryProposalView> {
    return decodeMemoryProposal(await this.bridge.invoke("apply_memory_proposal", { command: {
      proposalId, expectedStatus: "approved", idempotencyKey,
    } }));
  }
}
