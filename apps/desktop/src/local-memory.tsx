import { useCallback, useEffect, useRef, useState } from "react";

import {
  createIdempotencyKey,
  type MemoryProposalCursor,
  type MemoryProposalView,
  type MemoryRecordView,
  type ProposeMemoryInput,
} from "@zhuangsheng/api-client";
import { MemoryPage } from "@zhuangsheng/domain-ui";

import { localErrorMessage, memory } from "./bridge";

export function LocalMemory() {
  const [scopeId, setScopeId] = useState("roleplay");
  const [records, setRecords] = useState<MemoryRecordView[]>([]);
  const [proposals, setProposals] = useState<MemoryProposalView[]>([]);
  const [nextCursor, setNextCursor] = useState<MemoryProposalCursor | null>(null);
  const [loading, setLoading] = useState(true);
  const [pending, setPending] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const keys = useRef(new Map<string, string>());
  const keyFor = (signature: string) => {
    const value = keys.current.get(signature) ?? createIdempotencyKey();
    keys.current.set(signature, value); return value;
  };
  const load = useCallback(async () => {
    setLoading(true); setError(null);
    try {
      const page = await memory.listProposals(scopeId);
      const [active, obsolete] = await Promise.all([
        searchOrEmpty(scopeId, "active"), searchOrEmpty(scopeId, "obsolete"),
      ]);
      setProposals(page.proposals); setNextCursor(page.nextCursor);
      setRecords([...active, ...obsolete]);
    } catch (cause) { setError(localErrorMessage(cause)); }
    finally { setLoading(false); }
  }, [scopeId]);
  useEffect(() => { void load(); }, [load]);
  const propose = async (input: Omit<ProposeMemoryInput, "scopeId" | "idempotencyKey">) => {
    const signature = `propose:${scopeId}:${JSON.stringify(input)}`;
    setPending(signature); setError(null);
    try {
      await memory.propose({ ...input, scopeId, idempotencyKey: keyFor(signature) });
      keys.current.delete(signature); await load();
    } catch (cause) { setError(localErrorMessage(cause)); throw cause; }
    finally { setPending(null); }
  };
  const decide = async (proposal: MemoryProposalView, decision: "approve" | "reject") => {
    const signature = `decide:${proposal.id}:${proposal.status}:${decision}`;
    setPending(signature); setError(null);
    try {
      await memory.decide(proposal.id, proposal.status, decision, keyFor(signature));
      keys.current.delete(signature); await load();
    } catch (cause) { setError(localErrorMessage(cause)); }
    finally { setPending(null); }
  };
  const apply = async (proposal: MemoryProposalView) => {
    const signature = `apply:${proposal.id}`;
    setPending(signature); setError(null);
    try {
      await memory.apply(proposal.id, keyFor(signature));
      keys.current.delete(signature); await load();
    } catch (cause) { setError(localErrorMessage(cause)); }
    finally { setPending(null); }
  };
  const loadMore = async () => {
    if (!nextCursor) return;
    setPending("load-more");
    try {
      const page = await memory.listProposals(scopeId, undefined, nextCursor);
      setProposals((items) => [...items, ...page.proposals.filter((item) => !items.some((old) => old.id === item.id))]);
      setNextCursor(page.nextCursor);
    } catch (cause) { setError(localErrorMessage(cause)); }
    finally { setPending(null); }
  };
  return <MemoryPage scopeId={scopeId} records={records} proposals={proposals} hasMore={nextCursor !== null} loading={loading} pending={pending !== null} error={error} onScopeChange={setScopeId} onReload={() => void load()} onLoadMore={() => void loadMore()} onPropose={propose} onDecide={(item, decision) => void decide(item, decision)} onApply={(item) => void apply(item)} />;
}

async function searchOrEmpty(scopeId: string, status: "active" | "obsolete") {
  try { return (await memory.search(scopeId, status)).records; }
  catch (cause) {
    if (cause && typeof cause === "object" && (cause as { code?: unknown }).code === "not_found") return [];
    throw cause;
  }
}
