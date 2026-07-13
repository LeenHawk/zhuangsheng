// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, renderHook, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { SettingsSetup } from "@zhuangsheng/domain-ui";

import { client } from "./api";
import { buildRolePresetSpec } from "./role-preset-spec";
import { useInitialSetup } from "./use-initial-setup";

const secret = { secretRef: { scheme: "secret" as const, id: "provider-key" }, name: "Provider key", kind: "api_key" as const, createdAt: 1, updatedAt: 1 };

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("first-run settings", () => {
  it("loads an uninitialized Secret Store without querying unavailable metadata", async () => {
    vi.spyOn(client.secrets, "status").mockResolvedValue({
      initialized: false,
      storeId: null,
      formatVersion: null,
      locked: true,
    });
    const listSecrets = vi.spyOn(client.secrets, "list");
    vi.spyOn(client.config, "listChannels").mockResolvedValue([]);
    vi.spyOn(client.config, "listPresets").mockResolvedValue([]);
    vi.spyOn(client, "listRolePlayGraphOptions").mockResolvedValue([]);

    const { result } = renderHook(() => useInitialSetup());

    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.status?.initialized).toBe(false);
    expect(result.current.error).toBeNull();
    expect(listSecrets).not.toHaveBeenCalled();
  });

  it("uses a write-only secret form and clears plaintext after success", async () => {
    const onStoreSecret = vi.fn(async (_input: unknown) => undefined);
    render(<SettingsSetup
      status={{ initialized: false, storeId: null, formatVersion: null, locked: true }}
      secrets={[]}
      channels={[]}
      presets={[]}
      templates={[]}
      preview={null}
      discovery={null}
      rolePlaySettings={null}
      loading={false}
      pending={null}
      error={null}
      onReload={() => undefined}
      onStoreSecret={onStoreSecret}
      onUnlockSecretStore={async () => undefined}
      onLockSecretStore={async () => undefined}
      onChangeSecretStorePassword={async () => undefined}
      onPublishChannel={async () => undefined}
      onPublishPreset={async () => undefined}
      onPreviewPreset={() => undefined}
      onCreateTemplate={async () => undefined}
      onDiscoverModels={() => undefined}
      onPublishDiscoveredModel={async () => undefined}
      onInspectTemplate={() => undefined}
    />);
    fireEvent.change(screen.getByLabelText("API key"), { target: { value: "secret-provider-value" } });
    fireEvent.change(screen.getByLabelText("设置主密码"), { target: { value: "long-master-password" } });
    fireEvent.change(screen.getByLabelText("确认主密码"), { target: { value: "long-master-password" } });
    fireEvent.click(screen.getByRole("button", { name: "保存凭据" }));

    await waitFor(() => expect(onStoreSecret).toHaveBeenCalledOnce());
    expect(onStoreSecret.mock.calls[0]?.[0]).toMatchObject({ secretId: "provider-api-key", value: "secret-provider-value", masterPassword: "long-master-password", passwordCommandKey: expect.any(String), putCommandKey: expect.any(String) });
    await waitFor(() => expect(screen.getByLabelText("API key")).toHaveValue(""));
    expect(screen.getByLabelText("设置主密码")).toHaveValue("");
  });

  it("uses dedicated dialogs for unlock and password change and clears them on close", async () => {
    const onUnlock = vi.fn(async () => undefined);
    const onChange = vi.fn(async () => undefined);
    render(<SettingsSetup
      status={{ initialized: true, storeId: "store_1", formatVersion: 1, locked: true }}
      secrets={[]} channels={[]} presets={[]} templates={[]} preview={null} discovery={null}
      rolePlaySettings={null} loading={false} pending={null} error={null}
      onReload={() => undefined} onStoreSecret={async () => undefined}
      onUnlockSecretStore={onUnlock} onLockSecretStore={async () => undefined}
      onChangeSecretStorePassword={onChange} onPublishChannel={async () => undefined}
      onPublishPreset={async () => undefined} onPreviewPreset={() => undefined}
      onCreateTemplate={async () => undefined} onDiscoverModels={() => undefined}
      onPublishDiscoveredModel={async () => undefined} onInspectTemplate={() => undefined}
    />);
    fireEvent.click(screen.getByRole("button", { name: "解锁" }));
    let dialog = screen.getByRole("dialog");
    fireEvent.change(within(dialog).getByLabelText("当前主密码"), { target: { value: "current-master-password" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "解锁" }));
    await waitFor(() => expect(onUnlock).toHaveBeenCalledWith("current-master-password", expect.any(String)));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());

    fireEvent.click(screen.getByRole("button", { name: "修改主密码" }));
    dialog = screen.getByRole("dialog");
    expect(within(dialog).getByLabelText("当前主密码")).toHaveValue("");
    fireEvent.change(within(dialog).getByLabelText("当前主密码"), { target: { value: "current-master-password" } });
    fireEvent.change(within(dialog).getByLabelText("新主密码"), { target: { value: "replacement-master-password" } });
    fireEvent.change(within(dialog).getByLabelText("确认新主密码"), { target: { value: "replacement-master-password" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "确认修改" }));
    await waitFor(() => expect(onChange).toHaveBeenCalledWith("current-master-password", "replacement-master-password", expect.any(String), expect.any(String)));
  });

  it("maps friendly connection and character fields to canonical commands", async () => {
    const onPublishChannel = vi.fn(async () => undefined);
    const onPublishPreset = vi.fn(async () => undefined);
    const onCreateTemplate = vi.fn(async () => undefined);
    const onPreviewPreset = vi.fn();
    render(<SettingsSetup
      status={{ initialized: true, storeId: "store_1", formatVersion: 1, locked: false }}
      secrets={[secret]}
      channels={[{ id: "channel_1", name: "Primary", headRevisionId: "channelrev_1", createdAt: 1, updatedAt: 1 }]}
      presets={[{ id: "preset_1", name: "Character", headVersionId: "presetver_1", createdAt: 1, updatedAt: 1 }]}
      templates={[]}
      preview={null}
      discovery={null}
      rolePlaySettings={null}
      loading={false}
      pending={null}
      error={null}
      onReload={() => undefined}
      onStoreSecret={async () => undefined}
      onUnlockSecretStore={async () => undefined}
      onLockSecretStore={async () => undefined}
      onChangeSecretStorePassword={async () => undefined}
      onPublishChannel={onPublishChannel}
      onPublishPreset={onPublishPreset}
      onPreviewPreset={onPreviewPreset}
      onCreateTemplate={onCreateTemplate}
      onDiscoverModels={() => undefined}
      onPublishDiscoveredModel={async () => undefined}
      onInspectTemplate={() => undefined}
    />);
    fireEvent.change(screen.getByLabelText("Model ID"), { target: { value: "roleplay-model" } });
    const structuredOutput = screen.getByLabelText("我确认该模型支持结构化 JSON 输出（角色回复合同需要）");
    expect(structuredOutput).not.toBeChecked();
    expect(screen.getByRole("button", { name: "发布 Channel" })).toBeDisabled();
    fireEvent.click(structuredOutput);
    expect(structuredOutput).toBeChecked();
    fireEvent.click(screen.getByRole("button", { name: "发布 Channel" }));
    await waitFor(() => expect(onPublishChannel).toHaveBeenCalledWith(expect.objectContaining({ providerKind: "open_ai_responses", modelId: "roleplay-model", credentialSecretId: "provider-key", structuredOutput: true })));

    fireEvent.change(screen.getByLabelText("角色名称"), { target: { value: "Alice" } });
    fireEvent.change(screen.getByLabelText("身份与背景"), { target: { value: "月下档案馆的守护者" } });
    fireEvent.click(screen.getByRole("button", { name: "发布角色模板" }));
    await waitFor(() => expect(onPublishPreset).toHaveBeenCalledWith(expect.objectContaining({ characterName: "Alice", identity: "月下档案馆的守护者" })));
    await waitFor(() => expect(screen.getByRole("button", { name: "创建 Agent 模板" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "创建 Agent 模板" }));
    await waitFor(() => expect(onCreateTemplate).toHaveBeenCalledWith({ name: "Role Play Agent", channelId: "channel_1", presetId: "preset_1" }));
    fireEvent.click(screen.getByRole("button", { name: "Preview Character" }));
    expect(onPreviewPreset).toHaveBeenCalledWith(expect.objectContaining({ id: "preset_1", headVersionId: "presetver_1" }));
  });

  it("compiles role fields into one canonical required ContextPreset item", () => {
    const spec = buildRolePresetSpec({ name: "Template", characterName: "Alice", identity: "守护者", personality: "克制", speakingStyle: "简洁", boundaries: "不泄露秘密" });
    expect(spec).toMatchObject({
      mode: "chat",
      items: [
        { id: "character", enabled: true, requestedRole: "system", source: { type: "literal", text: expect.stringContaining("角色：Alice") }, budget: { required: true } },
        { id: "history", source: { type: "history", bindingId: "history", strategy: { type: "all" } }, position: { type: "history" }, overflow: { type: "keep_recent", count: null } },
      ],
      preview: { content: "metadata_only", count: "local" },
    });
  });
});
