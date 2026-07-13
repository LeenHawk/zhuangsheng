import { useCallback, useEffect, useRef, useState } from "react";

import {
  createIdempotencyKey,
  type ArtifactStagingView,
  type ArtifactView,
  type UploadArtifactInput,
} from "@zhuangsheng/api-client";
import { ArtifactPage } from "@zhuangsheng/domain-ui";

import { artifacts, localErrorMessage } from "./bridge";

export function LocalArtifacts() {
  const [items, setItems] = useState<ArtifactView[]>([]);
  const [loading, setLoading] = useState(true);
  const [pending, setPending] = useState(false);
  const [pendingCommit, setPendingCommit] = useState<ArtifactStagingView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const keys = useRef(new Map<string, string>());
  const load = useCallback(async () => {
    setLoading(true); setError(null);
    try { setItems((await artifacts.list(100)).items); }
    catch (cause) { setError(localErrorMessage(cause)); }
    finally { setLoading(false); }
  }, []);
  useEffect(() => { void load(); }, [load]);
  const commit = async (staging: ArtifactStagingView) => {
    const key = keys.current.get(staging.stagingId) ?? createIdempotencyKey();
    keys.current.set(staging.stagingId, key);
    await artifacts.commit(staging, key);
    keys.current.delete(staging.stagingId); setPendingCommit(null); await load();
  };
  const upload = async (input: UploadArtifactInput) => {
    if (pendingCommit) { setError("请先完成当前 staging 的 commit。"); return; }
    setPending(true); setError(null);
    try {
      const staging = await artifacts.upload(input);
      setPendingCommit(staging); await commit(staging);
    } catch (cause) { setError(localErrorMessage(cause)); throw cause; }
    finally { setPending(false); }
  };
  const retryCommit = async () => {
    if (!pendingCommit) return;
    setPending(true); setError(null);
    try { await commit(pendingCommit); }
    catch (cause) { setError(localErrorMessage(cause)); }
    finally { setPending(false); }
  };
  return <ArtifactPage items={items} loading={loading} pending={pending} pendingCommit={pendingCommit} error={error} onReload={() => void load()} onUpload={upload} onRetryCommit={() => void retryCommit()} contentUrl={() => "#"} onDownload={(id) => artifacts.downloadToBrowser(id)} />;
}
