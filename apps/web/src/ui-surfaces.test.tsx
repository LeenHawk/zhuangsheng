// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { AppShell, ApplicationSettings, CommandPalette, LibraryPage, notifyShellStatusChanged, PlatformCapabilitiesProvider, useAppShellStatus } from "@zhuangsheng/domain-ui";
import { webPlatformCapabilities } from "@zhuangsheng/api-client";
import { defaultUiPreferences } from "./ui-preferences";

describe("global UI surfaces", () => {
  afterEach(cleanup);
  it("receives platform, connection, and secret status from the shell composition root", async () => {
    const loadStatus = vi.fn()
      .mockResolvedValueOnce({ initialized: true, storeId: "store_1", formatVersion: 1, locked: true })
      .mockResolvedValue({ initialized: true, storeId: "store_1", formatVersion: 1, locked: false });
    render(<PlatformCapabilitiesProvider value={webPlatformCapabilities}>
      <ShellHarness loadStatus={loadStatus} />
    </PlatformCapabilitiesProvider>);
    expect(screen.getByText("Web 服务")).toBeInTheDocument();
    await waitFor(() => expect(screen.getByLabelText("服务已连接")).toBeInTheDocument());
    expect(screen.getByLabelText("Secret 已锁定")).toBeInTheDocument();
    const mobileNavigation = screen.getByRole("navigation", { name: "移动导航" });
    expect(mobileNavigation).toHaveClass("md:hidden");
    expect(within(mobileNavigation).getAllByRole("button").map((button) => button.textContent)).toEqual([
      "故事", "资料库", "记忆", "设置",
    ]);
    within(mobileNavigation).getAllByRole("button").forEach((button) => expect(button).toHaveClass("min-h-11"));
    notifyShellStatusChanged();
    await waitFor(() => expect(screen.getByLabelText("Secret 已解锁")).toBeInTheDocument());
  });

  it("saves application preferences separately from runtime configuration", () => {
    const onSave = vi.fn();
    render(<ApplicationSettings value={defaultUiPreferences} onSave={onSave} />);
    fireEvent.change(screen.getByLabelText("主题"), { target: { value: "contrast" } });
    fireEvent.click(screen.getByLabelText("减少动画"));
    fireEvent.click(screen.getByRole("button", { name: "保存应用设置" }));
    expect(onSave).toHaveBeenCalledWith(expect.objectContaining({ theme: "contrast", reducedMotion: true }));
    expect(screen.getByText(/不修改既有 GraphRun/)).toBeInTheDocument();
  });

  it("projects versioned presets and templates in the Library without copying content", () => {
    render(<LibraryPage
      presets={[{ id: "preset_1", name: "守夜人", headVersionId: "version_1", createdAt: 1, updatedAt: 2 }]}
      versions={{ version_1: { id: "version_1", presetId: "preset_1", versionNo: 2, semanticPolicyVersion: 1, spec: {}, contentHash: "sha256:preset", createdAt: 2 } }}
      templates={[{ graphId: "graph_1", graphName: "档案馆模板", revisionId: "revision_1", revisionNo: 3, replyOutputKeys: ["reply"], primaryLlmNodeId: "reply", compatibility: { mode: "editable", profileVersion: 1, editableFields: [] } }]}
      artifacts={[]} loading={false} error={null} onReload={() => undefined}
      onOpenSettings={() => undefined} onOpenArtifacts={() => undefined} contentUrl={() => "#"}
    />);
    expect(screen.getByText("published v2")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("tab", { name: "Agent 模板" }));
    expect(screen.getByText("档案馆模板")).toBeInTheDocument();
    expect(screen.getByText("用户模式可完整编辑")).toBeInTheDocument();
  });

  it("opens Ctrl+K outside text fields, traps focus, and restores the trigger", async () => {
    const onSelect = vi.fn();
    render(<><button>before</button><CommandPalette items={[{ id: "library", label: "资料库" }, { id: "settings", label: "设置" }]} onSelect={onSelect} /></>);
    const trigger = screen.getByRole("button", { name: "打开资源与命令搜索" });
    trigger.focus();
    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    expect(screen.getByRole("dialog", { name: "资源与命令搜索" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "资料库" }));
    expect(onSelect).toHaveBeenCalledWith("library");
    await waitFor(() => expect(trigger).toHaveFocus());
    const input = document.createElement("input");
    document.body.append(input); input.focus();
    fireEvent.keyDown(input, { key: "k", ctrlKey: true });
    expect(screen.queryByRole("dialog", { name: "资源与命令搜索" })).not.toBeInTheDocument();
    input.remove();
  });
});

function ShellHarness({ loadStatus }: { loadStatus: () => Promise<{ initialized: boolean; storeId: string | null; formatVersion: number | null; locked: boolean }> }) {
  const status = useAppShellStatus(loadStatus, false);
  return <AppShell mode="user" section="stories" status={status} onModeChange={() => undefined} onSectionChange={() => undefined}><div>content</div></AppShell>;
}
