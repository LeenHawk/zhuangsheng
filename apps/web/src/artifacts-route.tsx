import { ArtifactPage } from "@zhuangsheng/domain-ui";

import { useArtifacts } from "./use-artifacts";

export function ArtifactsRoute() {
  const artifacts = useArtifacts();
  return <ArtifactPage {...artifacts} onReload={artifacts.reload} onUpload={artifacts.upload} onRetryCommit={artifacts.retryCommit} />;
}
