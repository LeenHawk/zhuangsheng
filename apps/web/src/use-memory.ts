import { useCallback, useEffect, useRef, useState } from "react";

import { ApiError, createIdempotencyKey, type MemoryProposalCursor, type MemoryProposalView, type MemoryRecordView, type ProposeMemoryInput } from "@zhuangsheng/api-client";

import { client, messageFor } from "./api";

export function useMemory() {
  const [scopeId, setScopeId] = useState("roleplay");
  const [records, setRecords] = useState<MemoryRecordView[]>([]);
  const [proposals, setProposals] = useState<MemoryProposalView[]>([]);
  const [nextCursor, setNextCursor] = useState<MemoryProposalCursor | null>(null);
  const [loading, setLoading] = useState(true);
  const [pending, setPending] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const keys = useRef(new Map<string, string>());

  const load = useCallback(async (signal?: AbortSignal) => {
    setLoading(true); setError(null);
    try {
      const proposalPage = await client.memory.listProposals(scopeId, undefined, undefined, signal);
      const [active, obsolete] = await Promise.all([
        searchOrEmpty(scopeId, "active", signal), searchOrEmpty(scopeId, "obsolete", signal),
      ]);
      setProposals(proposalPage.proposals); setNextCursor(proposalPage.nextCursor); setRecords([...active, ...obsolete]);
    } catch (cause) { if (!signal?.aborted) setError(messageFor(cause)); }
    finally { if (!signal?.aborted) setLoading(false); }
  }, [scopeId]);

  useEffect(() => { const controller = new AbortController(); void load(controller.signal); return () => controller.abort(); }, [load]);
  const keyFor = (signature: string) => {
    const existing = keys.current.get(signature); if (existing) return existing;
    const key = createIdempotencyKey(); keys.current.set(signature, key); return key;
  };
  const complete = (signature: string) => keys.current.delete(signature);

  const propose = async (input: Omit<ProposeMemoryInput, "scopeId" | "idempotencyKey">) => {
    const signature = `propose:${scopeId}:${JSON.stringify(input)}`; setPending(signature); setError(null);
    try { await client.memory.propose({ ...input, scopeId, idempotencyKey: keyFor(signature) }); complete(signature); await load(); }
    catch (cause) { setError(memoryError(cause)); throw cause; }
    finally { setPending(null); }
  };

  const decide = async (proposal: MemoryProposalView, decision: "approve" | "reject") => {
    const signature = `decide:${proposal.id}:${proposal.status}:${decision}`; setPending(signature); setError(null);
    try { await client.memory.decide(proposal.id, proposal.status, decision, keyFor(signature)); complete(signature); await load(); }
    catch (cause) { setError(memoryError(cause)); }
    finally { setPending(null); }
  };

  const apply = async (proposal: MemoryProposalView) => {
    const signature = `apply:${proposal.id}`; setPending(signature); setError(null);
    try { await client.memory.apply(proposal.id, keyFor(signature)); complete(signature); await load(); }
    catch (cause) { setError(memoryError(cause)); }
    finally { setPending(null); }
  };

  const loadMore = async () => {
    if (!nextCursor) return;
    setPending("load-more"); setError(null);
    try {
      const page = await client.memory.listProposals(scopeId, undefined, nextCursor);
      setProposals((items) => [...items, ...page.proposals.filter((proposal) => !items.some((item) => item.id === proposal.id))]);
      setNextCursor(page.nextCursor);
    } catch (cause) { setError(messageFor(cause)); }
    finally { setPending(null); }
  };

  return { scopeId, setScopeId, records, proposals, hasMore: nextCursor !== null, loading, pending, error, reload: () => void load(), loadMore, propose, decide, apply };
}

async function searchOrEmpty(scopeId: string, status: "active" | "obsolete", signal?: AbortSignal) {
  try { return (await client.memory.search(scopeId, status, signal)).records; }
  catch (cause) { if (cause instanceof ApiError && cause.status === 404) return []; throw cause; }
}

function memoryError(cause: unknown) {
  if (cause instanceof ApiError && ["memory_proposal_status", "memory_head"].includes(cause.body.code)) {
    return "记忆或提案状态已变化，请刷新后重新检查；不会覆盖较新的版本。";
  }
  return messageFor(cause);
}
