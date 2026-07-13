// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { StoryList } from "@zhuangsheng/domain-ui";

describe("StoryList", () => {
  it("keeps creation explicit and passes the entered title to the command owner", async () => {
    const onCreate = vi.fn(async () => undefined);
    render(
      <StoryList
        stories={[]}
        templates={[template]}
        templateSettings={{ graphrev_1: settings }}
        secretStatus={{ initialized: true, storeId: "store_1", formatVersion: 1, locked: false }}
        loading={false}
        pending={false}
        error={null}
        onReload={() => undefined}
        onCreate={onCreate}
        onUnlockSecretStore={async () => undefined}
        onOpen={() => undefined}
        onConfigure={() => undefined}
      />,
    );

    expect(screen.getByRole("heading", { name: "最近的故事" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "新建故事" }));
    expect(screen.getByRole("heading", { name: "选择角色与 Agent 模板" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "下一步" }));
    expect(screen.getByRole("heading", { name: "Persona 与世界来源" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "下一步" }));
    expect(screen.getByRole("heading", { name: "模型与能力" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "下一步" }));
    expect(screen.getByRole("heading", { name: "开场检查" })).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText(/故事名称/), { target: { value: "月下档案馆" } });
    fireEvent.change(screen.getByLabelText("首条消息"), { target: { value: "推开档案馆的门。" } });
    fireEvent.click(screen.getByRole("button", { name: "创建故事并开始" }));

    await waitFor(() => expect(onCreate).toHaveBeenCalledWith("月下档案馆", {
      graphRevisionId: "graphrev_1",
      replyOutputKey: "reply",
      inputShape: "conversation_message_v1",
    }, "推开档案馆的门。"));
  });

  it("routes an unconfigured user into the guided Agent setup", () => {
    const onConfigure = vi.fn();
    render(<StoryList
      stories={[]}
      templates={[]}
      templateSettings={{}}
      secretStatus={null}
      loading={false}
      pending={false}
      error={null}
      onReload={() => undefined}
      onCreate={async () => undefined}
      onUnlockSecretStore={async () => undefined}
      onOpen={() => undefined}
      onConfigure={onConfigure}
    />);

    fireEvent.click(screen.getByRole("button", { name: "配置首个 Agent" }));
    expect(onConfigure).toHaveBeenCalledOnce();
  });
});

const template = {
  graphId: "graph_1",
  graphName: "Alice Agent",
  revisionId: "graphrev_1",
  revisionNo: 1,
  replyOutputKeys: ["reply"],
  primaryLlmNodeId: "reply",
  compatibility: {
    mode: "editable" as const,
    profileVersion: 1 as const,
    editableFields: ["model", "context.character"],
  },
};

const settings = {
  profileVersion: 1 as const,
  revisionId: "graphrev_1",
  primaryLlmNodeId: "reply",
  compatibility: template.compatibility,
  model: { channelId: "channel_1", modelId: "model_1", modelName: "Role Model", operationKey: {} },
  generation: {},
  streaming: { enabled: true, audience: "user" as const, persistChunks: false },
  contextPresetId: "preset_1",
};
