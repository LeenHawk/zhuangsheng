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
        loading={false}
        pending={false}
        error={null}
        onReload={() => undefined}
        onCreate={onCreate}
        onOpen={() => undefined}
      />,
    );

    expect(screen.getByRole("heading", { name: "最近的故事" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "新建故事" }));
    fireEvent.change(screen.getByLabelText(/故事名称/), { target: { value: "月下档案馆" } });
    fireEvent.click(screen.getByRole("button", { name: "创建" }));

    await waitFor(() => expect(onCreate).toHaveBeenCalledWith("月下档案馆"));
  });
});
