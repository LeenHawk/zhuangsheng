import { MemoryPage } from "@zhuangsheng/domain-ui";

import { useMemory } from "./use-memory";

export function MemoryRoute() {
  const memory = useMemory();
  return <MemoryPage
    scopeId={memory.scopeId}
    records={memory.records}
    proposals={memory.proposals}
    hasMore={memory.hasMore}
    loading={memory.loading}
    pending={memory.pending !== null}
    error={memory.error}
    onScopeChange={memory.setScopeId}
    onReload={memory.reload}
    onLoadMore={() => void memory.loadMore()}
    onPropose={memory.propose}
    onDecide={memory.decide}
    onApply={memory.apply}
  />;
}
