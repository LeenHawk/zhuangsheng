// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { SettingsSetup } from "@zhuangsheng/domain-ui";

afterEach(cleanup);

const common = {
  status: { initialized: true as const, storeId: "store_1", formatVersion: 1 as const, locked: false },
  secrets: [], presets: [], preview: null, loading: false, pending: null, error: null,
  onReload: () => undefined,
  onStoreSecret: async () => undefined,
  onUnlockSecretStore: async () => undefined,
  onLockSecretStore: async () => undefined,
  onChangeSecretStorePassword: async () => undefined,
  onPublishChannel: async () => undefined,
  onPublishPreset: async () => undefined,
  onPreviewPreset: () => undefined,
  onCreateTemplate: async () => undefined,
  onDiscoverModels: () => undefined,
  onPublishDiscoveredModel: async () => undefined,
  onInspectTemplate: () => undefined,
};

describe("settings projections", () => {
  it("keeps discovered models temporary until explicit publication", async () => {
    const onDiscoverModels = vi.fn();
    const onPublishDiscoveredModel = vi.fn(async () => undefined);
    render(<SettingsSetup
      {...common}
      channels={[{ id: "channel_1", name: "Primary", headRevisionId: "channelrev_1", createdAt: 1, updatedAt: 1 }]}
      templates={[]}
      discovery={{
        channelId: "channel_1",
        channelRevisionId: "channelrev_1",
        operationKey: { operation: "list_models", kind: "open_ai" },
        models: [
          { id: "model-a", name: "Model A", contextWindow: null, maxOutputTokens: null },
          { id: "model-b", name: null, contextWindow: 128000, maxOutputTokens: 4096 },
        ],
      }}
      rolePlaySettings={null}
      onDiscoverModels={onDiscoverModels}
      onPublishDiscoveredModel={onPublishDiscoveredModel}
    />);

    fireEvent.click(screen.getByRole("button", { name: "发现模型" }));
    expect(onDiscoverModels).toHaveBeenCalledWith(expect.objectContaining({ id: "channel_1" }));
    expect(screen.getByRole("button", { name: "发布所选模型" })).toBeDisabled();
    fireEvent.change(screen.getByLabelText("可用模型（2）"), { target: { value: "model-b" } });
    fireEvent.click(screen.getByLabelText("我确认所选模型支持结构化 JSON 输出"));
    fireEvent.click(screen.getByRole("button", { name: "发布所选模型" }));
    await waitFor(() => expect(onPublishDiscoveredModel).toHaveBeenCalledWith(
      expect.objectContaining({ id: "model-b" }), true,
    ));
  });

  it("renders only the server-projected role play settings", () => {
    const template = {
      graphId: "graph_1", graphName: "Alice", revisionId: "graphrev_1", revisionNo: 1,
      replyOutputKeys: ["reply"], primaryLlmNodeId: "reply",
      compatibility: { mode: "partial" as const, profileVersion: 1 as const, editableFields: ["model"], lockedReasons: ["tool_permissions_require_expert"] },
    };
    render(<SettingsSetup
      {...common}
      channels={[]}
      templates={[template]}
      discovery={null}
      rolePlaySettings={{
        profileVersion: 1, revisionId: "graphrev_1", primaryLlmNodeId: "reply",
        compatibility: template.compatibility,
        model: { channelId: "channel_1", modelId: "model_1", modelName: "Role Model", operationKey: {} },
        generation: { temperature: 0.7 },
        streaming: { enabled: false, audience: "user", persistChunks: false },
        contextPresetId: "preset_1",
      }}
    />);

    expect(screen.getByText("Role Model")).toBeInTheDocument();
    expect(screen.getByText("工具权限")).toBeInTheDocument();
    expect(screen.getByText(/temperature/)).toBeInTheDocument();
  });
});
