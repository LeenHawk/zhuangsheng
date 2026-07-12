import { useCallback, useEffect, useState } from "react";
import { useNavigate, useParams } from "react-router-dom";

import type { RunView } from "@zhuangsheng/api-client";
import { RunDetail, RunList } from "@zhuangsheng/domain-ui";

import { client, messageFor } from "./api";
import { useRunMonitor } from "./use-run-monitor";

export function RunsRoute() {
  const navigate = useNavigate();
  const [runs, setRuns] = useState<RunView[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const reload = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setRuns((await client.runtime.listRecentRuns()).items);
    } catch (cause) {
      setError(messageFor(cause));
    } finally {
      setLoading(false);
    }
  }, []);
  useEffect(() => { void reload(); }, [reload]);
  return <RunList runs={runs} loading={loading} error={error} onReload={() => void reload()} onOpen={(id) => navigate(`/expert/runs/${id}`)} />;
}

export function RunRoute() {
  const { runId = "" } = useParams();
  const navigate = useNavigate();
  const monitor = useRunMonitor(runId);
  return (
    <RunDetail
      {...monitor}
      onBack={() => navigate("/expert/runs")}
      onControl={monitor.control}
      onOpenContext={(contextId, branchId) => navigate(`/expert/contexts/${encodeURIComponent(contextId)}?branch=${encodeURIComponent(branchId)}`)}
    />
  );
}
