import { afterEach, describe, expect, it, vi } from "vitest";

import { DecodeError } from "./decode-error";
import { decodeArtifact } from "./decode-artifacts";
import { HttpArtifactClient } from "./http-artifact-client";

describe("HttpArtifactClient", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("uploads metadata before bytes, commits by generation and builds a safe content URL", async () => {
    const calls: Array<{ input: RequestInfo | URL; init?: RequestInit }> = [];
    vi.stubGlobal("fetch", async (input: RequestInfo | URL, init?: RequestInit) => {
      calls.push({ input, init });
      return Response.json(calls.length === 1 ? staging() : artifact());
    });
    const client = new HttpArtifactClient("https://role.example");
    const staged = await client.upload({
      object: new Blob(["story note"], { type: "text/plain" }),
      name: "note.txt",
      classification: "private",
      retention: { type: "pinned" },
    });
    await client.commit(staged, "artifact-commit-1");

    const form = calls[0]?.init?.body as FormData;
    const metadata = JSON.parse(await (form.get("metadata") as Blob).text());
    expect(metadata).toEqual({
      contextId: null,
      metadataDraft: { name: "note.txt", classification: "private", retention: { type: "pinned" } },
      declaredMediaType: "text/plain",
    });
    expect((form.get("object") as Blob).size).toBe(10);
    expect(calls[1]?.input).toBe("https://role.example/v1/artifacts/staging/staging_1/commit");
    expect(calls[1]?.init?.headers).toMatchObject({ "idempotency-key": "artifact-commit-1" });
    expect(JSON.parse(calls[1]?.init?.body as string)).toEqual({ expectedLifecycleGeneration: 2 });
    expect(client.contentUrl("artifact/1")).toBe("https://role.example/v1/artifacts/artifact%2F1/content");
  });

  it("fails closed when metadata identity or enums drift", () => {
    expect(() => decodeArtifact({
      ...artifact(),
      metadata: { ...artifact().metadata, classification: "future" },
    })).toThrow(DecodeError);
    expect(() => decodeArtifact({
      ...artifact(),
      metadata: {
        ...artifact().metadata,
        content: { ...artifact().metadata.content, artifactId: "artifact_other" },
      },
    })).toThrow(DecodeError);
  });

  it("lists bounded metadata projections without loading content", async () => {
    const calls: string[] = [];
    vi.stubGlobal("fetch", async (input: RequestInfo | URL) => {
      calls.push(String(input));
      return Response.json({ items: [artifact()] });
    });
    const page = await new HttpArtifactClient("https://role.example").list(999);
    expect(page.items[0]?.metadata.name).toBe("note.txt");
    expect(calls).toEqual(["https://role.example/v1/artifacts?limit=100"]);
  });

  it("reads the exact staging resource before retrying a lifecycle command", async () => {
    let requested: RequestInfo | URL | null = null;
    vi.stubGlobal("fetch", async (input: RequestInfo | URL) => {
      requested = input;
      return Response.json({ ...staging(), stagingId: "staging/1" });
    });
    const result = await new HttpArtifactClient("https://role.example").getStaging("staging/1");
    expect(requested).toBe("https://role.example/v1/artifacts/staging/staging%2F1");
    expect(result.lifecycleGeneration).toBe(2);
  });
});

const staging = () => ({
  stagingId: "staging_1",
  status: "validated",
  lifecycleGeneration: 2,
  byteSize: 10,
  contentHash: `sha256:${"a".repeat(64)}`,
  validatedMediaType: "text/plain",
});

const artifact = () => ({
  metadata: {
    artifactId: "artifact_1",
    content: {
      artifactId: "artifact_1",
      contentHash: `sha256:${"a".repeat(64)}`,
      byteSize: 10,
      mediaType: "text/plain",
    },
    name: "note.txt",
    classification: "private",
    status: "active",
    originRunId: null,
    originNodeInstanceId: null,
    originToolCallId: null,
    retention: { type: "pinned" },
    createdAt: 1,
  },
  metadataHeadCommitId: "commit_1",
});
