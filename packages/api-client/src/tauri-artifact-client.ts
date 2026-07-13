import type {
  ArtifactListView,
  ArtifactStagingView,
  ArtifactView,
  UploadArtifactInput,
} from "./artifact-types";
import { decodeArtifact, decodeArtifactList, decodeArtifactStaging } from "./decode-artifacts";
import { DecodeError } from "./decode-error";
import type { TauriBridge } from "./transport";

export class TauriArtifactClient {
  constructor(private readonly bridge: TauriBridge) {}

  async list(limit = 50): Promise<ArtifactListView> {
    return decodeArtifactList(await this.bridge.invoke("list_artifacts", {
      limit: Math.max(1, Math.min(100, Math.trunc(limit))),
    }));
  }

  async get(artifactId: string): Promise<ArtifactView> {
    const value = decodeArtifact(await this.bridge.invoke("get_artifact", { artifactId }));
    if (value.metadata.artifactId !== artifactId) throw new DecodeError("artifact.id");
    return value;
  }

  async getStaging(stagingId: string): Promise<ArtifactStagingView> {
    const value = decodeArtifactStaging(await this.bridge.invoke("get_artifact_staging", { stagingId }));
    if (value.stagingId !== stagingId) throw new DecodeError("artifactStaging.stagingId");
    return value;
  }

  async upload(input: UploadArtifactInput): Promise<ArtifactStagingView> {
    if (input.object.size > 16 * 1024 * 1024) {
      throw new DecodeError("artifactUpload.size");
    }
    const created = decodeArtifactStaging(await this.bridge.invoke("create_artifact_staging", { command: {
      contextId: input.contextId ?? null,
      nodeAttemptId: null,
      toolCallId: null,
      metadataDraft: {
        name: input.name?.trim() || null,
        classification: input.classification,
        retention: input.retention,
      },
      declaredMediaType: input.declaredMediaType?.trim() || input.object.type || null,
    } }));
    const bytes = [...new Uint8Array(await input.object.arrayBuffer())];
    const staged = decodeArtifactStaging(await this.bridge.invoke("complete_artifact_staging", { input: {
      stagingId: created.stagingId,
      expectedLifecycleGeneration: created.lifecycleGeneration,
      bytes,
    } }));
    if (staged.status !== "validated") throw new DecodeError("artifactStaging.status");
    return staged;
  }

  async commit(
    staging: Pick<ArtifactStagingView, "stagingId" | "lifecycleGeneration">,
    idempotencyKey: string,
  ): Promise<ArtifactView> {
    return decodeArtifact(await this.bridge.invoke("commit_artifact_staging", { command: {
      stagingId: staging.stagingId,
      expectedLifecycleGeneration: staging.lifecycleGeneration,
      idempotencyKey,
    } }));
  }

  async download(artifactId: string): Promise<{ artifact: ArtifactView; bytes: Uint8Array }> {
    const wire = await this.bridge.invoke<{ artifact: unknown; bytes: unknown }>("download_artifact", { artifactId });
    const artifact = decodeArtifact(wire.artifact);
    if (artifact.metadata.artifactId !== artifactId || !Array.isArray(wire.bytes)
      || wire.bytes.some((byte) => !Number.isInteger(byte) || byte < 0 || byte > 255)) {
      throw new DecodeError("artifactDownload");
    }
    const bytes = Uint8Array.from(wire.bytes as number[]);
    if (bytes.byteLength !== artifact.metadata.content.byteSize) {
      throw new DecodeError("artifactDownload.bytes");
    }
    return { artifact, bytes };
  }

  async downloadToBrowser(artifactId: string): Promise<void> {
    const { artifact, bytes } = await this.download(artifactId);
    const buffer = new ArrayBuffer(bytes.byteLength);
    new Uint8Array(buffer).set(bytes);
    const url = URL.createObjectURL(new Blob([buffer], { type: artifact.metadata.content.mediaType }));
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = artifact.metadata.name ?? "artifact";
    anchor.hidden = true;
    document.body.append(anchor);
    anchor.click();
    anchor.remove();
    setTimeout(() => URL.revokeObjectURL(url), 0);
  }
}
