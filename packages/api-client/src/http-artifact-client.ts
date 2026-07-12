import type { ArtifactListView, ArtifactStagingView, ArtifactView, UploadArtifactInput } from "./artifact-types";
import { decodeArtifact, decodeArtifactList, decodeArtifactStaging } from "./decode-artifacts";
import { DecodeError } from "./decode-error";
import { requestJson } from "./http-json";

export class HttpArtifactClient {
  constructor(private readonly baseUrl: string) {}

  async list(limit = 50, signal?: AbortSignal): Promise<ArtifactListView> {
    const bounded = Math.max(1, Math.min(100, Math.trunc(limit)));
    return decodeArtifactList(await requestJson(this.baseUrl, `/v1/artifacts?limit=${bounded}`, { signal }));
  }

  async get(artifactId: string, signal?: AbortSignal): Promise<ArtifactView> {
    return decodeArtifact(await requestJson(
      this.baseUrl,
      `/v1/artifacts/${encodeURIComponent(artifactId)}`,
      { signal },
    ));
  }

  async upload(input: UploadArtifactInput, signal?: AbortSignal): Promise<ArtifactStagingView> {
    const form = new FormData();
    const metadata = {
      contextId: input.contextId ?? null,
      metadataDraft: {
        name: input.name?.trim() || null,
        classification: input.classification,
        retention: input.retention,
      },
      declaredMediaType: input.declaredMediaType?.trim() || input.object.type || null,
    };
    form.append("metadata", new Blob([JSON.stringify(metadata)], { type: "application/json" }));
    form.append("object", input.object, input.name?.trim() || "upload");
    const staging = decodeArtifactStaging(await requestJson(this.baseUrl, "/v1/artifacts/staging", {
      method: "POST",
      body: form,
      signal,
    }));
    if (staging.status !== "validated") throw new DecodeError("artifactStaging.status");
    return staging;
  }

  async commit(
    staging: Pick<ArtifactStagingView, "stagingId" | "lifecycleGeneration">,
    idempotencyKey: string,
    signal?: AbortSignal,
  ): Promise<ArtifactView> {
    return decodeArtifact(await requestJson(
      this.baseUrl,
      `/v1/artifacts/staging/${encodeURIComponent(staging.stagingId)}/commit`,
      {
        method: "POST",
        headers: { "content-type": "application/json", "idempotency-key": idempotencyKey },
        body: JSON.stringify({ expectedLifecycleGeneration: staging.lifecycleGeneration }),
        signal,
      },
    ));
  }

  contentUrl(artifactId: string): string {
    return `${this.baseUrl}/v1/artifacts/${encodeURIComponent(artifactId)}/content`;
  }
}
