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
        loading={false}
        pending={false}
        error={null}
        onReload={() => undefined}
        onCreate={onCreate}
        onOpen={() => undefined}
        onConfigure={() => undefined}
      />,
    );

    expect(screen.getByRole("heading", { name: "最近的故事" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "新建故事" }));
    fireEvent.change(screen.getByLabelText(/故事名称/), { target: { value: "月下档案馆" } });
    fireEvent.click(screen.getByRole("button", { name: "创建" }));

    await waitFor(() => expect(onCreate).toHaveBeenCalledWith("月下档案馆", {
      graphRevisionId: "graphrev_1",
      replyOutputKey: "reply",
      inputShape: "conversation_message_v1",
    }));
  });

  it("routes an unconfigured user into the guided Agent setup", () => {
    const onConfigure = vi.fn();
    render(<StoryList
      stories={[]}
      templates={[]}
      loading={false}
      pending={false}
      error={null}
      onReload={() => undefined}
      onCreate={async () => undefined}
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
