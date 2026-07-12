// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, renderHook, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { SettingsSetup } from "@zhuangsheng/domain-ui";

import { client } from "./api";
import { buildRolePresetSpec, useInitialSetup } from "./use-initial-setup";

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
      loading={false}
      pending={null}
      error={null}
      onReload={() => undefined}
      onStoreSecret={onStoreSecret}
      onPublishChannel={async () => undefined}
      onPublishPreset={async () => undefined}
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

  it("maps friendly connection and character fields to canonical commands", async () => {
    const onPublishChannel = vi.fn(async () => undefined);
    const onPublishPreset = vi.fn(async () => undefined);
    render(<SettingsSetup
      status={{ initialized: true, storeId: "store_1", formatVersion: 1, locked: false }}
      secrets={[secret]}
      channels={[]}
      presets={[]}
      loading={false}
      pending={null}
      error={null}
      onReload={() => undefined}
      onStoreSecret={async () => undefined}
      onPublishChannel={onPublishChannel}
      onPublishPreset={onPublishPreset}
    />);
    fireEvent.change(screen.getByLabelText("Model ID"), { target: { value: "roleplay-model" } });
    fireEvent.click(screen.getByRole("button", { name: "发布 Channel" }));
    await waitFor(() => expect(onPublishChannel).toHaveBeenCalledWith(expect.objectContaining({ providerKind: "open_ai_responses", modelId: "roleplay-model", credentialSecretId: "provider-key" })));

    fireEvent.change(screen.getByLabelText("角色名称"), { target: { value: "Alice" } });
    fireEvent.change(screen.getByLabelText("身份与背景"), { target: { value: "月下档案馆的守护者" } });
    fireEvent.click(screen.getByRole("button", { name: "发布角色模板" }));
    await waitFor(() => expect(onPublishPreset).toHaveBeenCalledWith(expect.objectContaining({ characterName: "Alice", identity: "月下档案馆的守护者" })));
  });

  it("compiles role fields into one canonical required ContextPreset item", () => {
    const spec = buildRolePresetSpec({ name: "Template", characterName: "Alice", identity: "守护者", personality: "克制", speakingStyle: "简洁", boundaries: "不泄露秘密" });
    expect(spec).toMatchObject({
      mode: "chat",
      items: [{ id: "character", enabled: true, requestedRole: "system", source: { type: "literal", text: expect.stringContaining("角色：Alice") }, budget: { required: true } }],
      preview: { content: "metadata_only", count: "local" },
    });
  });
});
