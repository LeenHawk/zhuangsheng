import type { JsonObject } from "@zhuangsheng/api-client";

export interface RolePresetInput {
  name: string;
  characterName: string;
  identity: string;
  personality: string;
  speakingStyle: string;
  boundaries: string;
}

export function buildRolePresetSpec(input: RolePresetInput): JsonObject {
  const sections: Array<[string, string]> = [
    ["角色", input.characterName],
    ["身份", input.identity],
    ["性格与目标", input.personality],
    ["说话风格", input.speakingStyle],
    ["内容边界", input.boundaries],
  ];
  const text = sections
    .filter(([, value]) => value.trim())
    .map(([label, value]) => `${label}：${value.trim()}`)
    .join("\n");
  return {
    mode: "chat",
    items: [
      { id: "character", name: input.characterName, enabled: true, requestedRole: "system", source: { type: "literal", text }, position: { type: "start" }, order: 0, priority: 100, insertionDepth: 0, budget: { required: true }, overflow: null },
      { id: "history", name: "Conversation history", enabled: true, requestedRole: "context", source: { type: "history", bindingId: "history", strategy: { type: "all" } }, position: { type: "history" }, order: 0, priority: 90, insertionDepth: 0, budget: { required: false }, overflow: { type: "keep_recent", count: null } },
    ],
    budget: null,
    postProcess: [],
    preview: { content: "metadata_only", count: "local" },
  };
}
