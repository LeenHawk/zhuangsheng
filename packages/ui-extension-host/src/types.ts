import type { PluginMessageRole, PluginRendererSlot } from "@zhuangsheng/api-client";

export type UiTone = "default" | "muted" | "accent" | "warning" | "danger";

export type UiInlineNode =
  | { type: "text"; text: string; tone?: UiTone; emphasis?: "none" | "strong" | "italic" }
  | { type: "badge"; text: string; tone?: UiTone }
  | { type: "link"; text: string; href: string };

export type UiNode =
  | UiInlineNode
  | { type: "paragraph"; children: UiInlineNode[]; align?: "start" | "center" | "end" }
  | { type: "heading"; level: 1 | 2 | 3; children: UiInlineNode[] }
  | { type: "quote"; children: UiNode[] }
  | { type: "code"; text: string; language?: string }
  | { type: "divider" }
  | { type: "stack"; gap?: "small" | "medium" | "large"; children: UiNode[] };

export interface PluginRenderRequest {
  rendererId: string;
  slot: PluginRendererSlot;
  message: {
    id: string;
    role: PluginMessageRole;
    source: string;
    text: string;
    reasoning: string | null;
    streaming: boolean;
  };
  mode: "user" | "expert";
  platform: "web" | "desktop" | "mobile";
}

export interface AvailablePluginRenderer {
  key: string;
  pluginId: string;
  pluginName: string;
  rendererId: string;
  slot: PluginRendererSlot;
  priority: number;
}
