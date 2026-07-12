import { decodeContextBranches, decodeContextCommits, decodeContextDiff } from "./decode-context";
import type { ContextBranchView, ContextCommitView, ContextDiffView } from "./context-types";
import { requestJson } from "./http-json";

export class HttpContextClient {
  constructor(private readonly baseUrl = "") {}

  async listBranches(contextId: string, signal?: AbortSignal): Promise<ContextBranchView[]> {
    return decodeContextBranches(await requestJson(
      this.baseUrl,
      `/v1/contexts/${encodeURIComponent(contextId)}/branches`,
      { signal },
    ));
  }

  async listCommits(contextId: string, signal?: AbortSignal): Promise<ContextCommitView[]> {
    return decodeContextCommits(await requestJson(
      this.baseUrl,
      `/v1/contexts/${encodeURIComponent(contextId)}/commits`,
      { signal },
    ));
  }

  async diff(
    contextId: string,
    fromCommitId: string,
    toCommitId: string,
    signal?: AbortSignal,
  ): Promise<ContextDiffView> {
    const query = new URLSearchParams({ from: fromCommitId, to: toCommitId });
    return decodeContextDiff(await requestJson(
      this.baseUrl,
      `/v1/contexts/${encodeURIComponent(contextId)}/diff?${query.toString()}`,
      { signal },
    ));
  }
}
