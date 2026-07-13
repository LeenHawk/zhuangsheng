// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { ArtifactPage, PlatformCapabilitiesProvider } from "@zhuangsheng/domain-ui";
import { webPlatformCapabilities, type ArtifactStagingView, type ArtifactView } from "@zhuangsheng/api-client";

describe("ArtifactPage", () => {
  afterEach(cleanup);
  it("uploads through staging metadata and never embeds active content", async () => {
    const onUpload = vi.fn(async () => undefined);
    renderPage({ items: [artifact], onUpload });
    const file = new File(["story note"], "note.txt", { type: "text/plain" });
    fireEvent.change(screen.getByLabelText("Artifact 文件"), { target: { files: [file] } });
    fireEvent.click(screen.getByRole("button", { name: "上传并 commit" }));

    await waitFor(() => expect(onUpload).toHaveBeenCalledWith(expect.objectContaining({
      object: file,
      name: "note.txt",
      declaredMediaType: "text/plain",
      classification: "private",
      retention: { type: "pinned" },
    })));
    expect(screen.getByRole("link", { name: "下载" })).toHaveAttribute("href", "/download/artifact_1");
    expect(document.querySelector("iframe, img, object, embed")).toBeNull();
  });

  it("keeps a validated staging generation visible for idempotent commit retry", () => {
    const onRetryCommit = vi.fn();
    renderPage({ pendingCommit: staging, onRetryCommit });
    expect(screen.getByText(/staging generation/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "重试 commit" }));
    expect(onRetryCommit).toHaveBeenCalledOnce();
  });

  it("disables file input when the injected platform denies picker capability", () => {
    render(<PlatformCapabilitiesProvider value={{
      ...webPlatformCapabilities,
      filePicker: false,
    }}><ArtifactPage
      items={[]} loading={false} pending={false} pendingCommit={null} error={null}
      onReload={() => undefined} onUpload={async () => undefined}
      onRetryCommit={() => undefined} contentUrl={() => "#"}
    /></PlatformCapabilitiesProvider>);
    expect(screen.getByLabelText("Artifact 文件")).toBeDisabled();
    expect(screen.getByText("当前平台未授予文件选择能力。")).toBeInTheDocument();
  });
});

const artifact: ArtifactView = {
  metadata: {
    artifactId: "artifact_1",
    content: { artifactId: "artifact_1", contentHash: `sha256:${"a".repeat(64)}`, byteSize: 10, mediaType: "text/plain" },
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
};

const staging: ArtifactStagingView = {
  stagingId: "staging_1",
  status: "validated",
  lifecycleGeneration: 2,
  byteSize: 10,
  contentHash: `sha256:${"a".repeat(64)}`,
  validatedMediaType: "text/plain",
};

function renderPage(overrides: Partial<React.ComponentProps<typeof ArtifactPage>> = {}) {
  render(<PlatformCapabilitiesProvider value={webPlatformCapabilities}><ArtifactPage
    items={[]}
    loading={false}
    pending={false}
    pendingCommit={null}
    error={null}
    onReload={() => undefined}
    onUpload={async () => undefined}
    onRetryCommit={() => undefined}
    contentUrl={(id) => `/download/${id}`}
    {...overrides}
  /></PlatformCapabilitiesProvider>);
}
