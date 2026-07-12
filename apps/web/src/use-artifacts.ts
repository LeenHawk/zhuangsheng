import { useCallback, useEffect, useRef, useState } from "react";

import { createIdempotencyKey, type ArtifactStagingView, type ArtifactView, type UploadArtifactInput } from "@zhuangsheng/api-client";

import { client, messageFor } from "./api";

export function useArtifacts() {
  const [items, setItems] = useState<ArtifactView[]>([]);
  const [loading, setLoading] = useState(true);
  const [pending, setPending] = useState(false);
  const [pendingCommit, setPendingCommit] = useState<ArtifactStagingView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const commitKeys = useRef(new Map<string, string>());

  const load = useCallback(async (signal?: AbortSignal) => {
    setLoading(true); setError(null);
    try { setItems((await client.artifacts.list(100, signal)).items); }
    catch (cause) { if (!signal?.aborted) setError(messageFor(cause)); }
    finally { if (!signal?.aborted) setLoading(false); }
  }, []);

  useEffect(() => {
    const controller = new AbortController(); void load(controller.signal);
    return () => controller.abort();
  }, [load]);

  const commit = async (staging: ArtifactStagingView) => {
    const key = commitKeys.current.get(staging.stagingId) ?? createIdempotencyKey();
    commitKeys.current.set(staging.stagingId, key);
    await client.artifacts.commit(staging, key);
    commitKeys.current.delete(staging.stagingId);
    setPendingCommit(null);
    await load();
  };

  const upload = async (input: UploadArtifactInput) => {
    if (pendingCommit) {
      setError("请先完成当前 staging 的 commit，再上传新 Artifact。");
      return;
    }
    setPending(true); setError(null);
    try {
      const staging = await client.artifacts.upload(input);
      setPendingCommit(staging);
      await commit(staging);
    } catch (cause) { setError(messageFor(cause)); throw cause; }
    finally { setPending(false); }
  };

  const retryCommit = async () => {
    if (!pendingCommit) return;
    setPending(true); setError(null);
    try { await commit(pendingCommit); }
    catch (cause) { setError(messageFor(cause)); }
    finally { setPending(false); }
  };

  return {
    items, loading, pending, pendingCommit, upload,
    retryCommit: () => void retryCommit(),
    reload: () => void load(),
    contentUrl: (artifactId: string) => client.artifacts.contentUrl(artifactId),
    error,
  };
}
