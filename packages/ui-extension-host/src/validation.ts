import type { UiInlineNode, UiNode, UiTone } from "./types";

const MAX_DEPTH = 8;
const MAX_NODES = 256;
const MAX_TEXT = 100_000;
const tones: UiTone[] = ["default", "muted", "accent", "warning", "danger"];

interface Budget { nodes: number; text: number }

export function validateUiNodes(value: unknown): UiNode[] {
  if (!Array.isArray(value)) throw new Error("plugin renderer must return an array");
  const budget: Budget = { nodes: 0, text: 0 };
  return value.map((node) => decodeNode(node, 0, budget));
}

function decodeNode(value: unknown, depth: number, budget: Budget): UiNode {
  if (depth > MAX_DEPTH || ++budget.nodes > MAX_NODES) throw new Error("plugin UI exceeds node limits");
  const item = object(value);
  switch (item.type) {
    case "text": case "badge": case "link": return decodeInline(item, budget);
    case "paragraph": return {
      type: "paragraph", align: optionalOneOf(item.align, ["start", "center", "end"]),
      children: decodeInlineArray(item.children, budget),
    };
    case "heading": {
      const level = number(item.level);
      if (level !== 1 && level !== 2 && level !== 3) throw new Error("invalid plugin heading level");
      return { type: "heading", level, children: decodeInlineArray(item.children, budget) };
    }
    case "quote": return { type: "quote", children: decodeChildren(item.children, depth, budget) };
    case "code": return { type: "code", text: text(item.text, budget), language: optionalText(item.language, 64) };
    case "divider": return { type: "divider" };
    case "stack": return {
      type: "stack", gap: optionalOneOf(item.gap, ["small", "medium", "large"]),
      children: decodeChildren(item.children, depth, budget),
    };
    default: throw new Error("unsupported plugin UI node");
  }
}

function decodeInline(item: Record<string, unknown>, budget: Budget): UiInlineNode {
  if (item.type === "link") {
    const href = string(item.href);
    if (!href.startsWith("https://")) throw new Error("plugin links must use HTTPS");
    return { type: "link", text: text(item.text, budget), href };
  }
  const type = item.type === "badge" ? "badge" : "text";
  const tone = optionalOneOf(item.tone, tones);
  if (type === "badge") return { type, text: text(item.text, budget), tone };
  return {
    type, text: text(item.text, budget), tone,
    emphasis: optionalOneOf(item.emphasis, ["none", "strong", "italic"]),
  };
}

function decodeInlineArray(value: unknown, budget: Budget): UiInlineNode[] {
  if (!Array.isArray(value)) throw new Error("plugin inline children must be an array");
  return value.map((child) => {
    if (++budget.nodes > MAX_NODES) throw new Error("plugin UI exceeds node limits");
    return decodeInline(object(child), budget);
  });
}

function decodeChildren(value: unknown, depth: number, budget: Budget): UiNode[] {
  if (!Array.isArray(value)) throw new Error("plugin children must be an array");
  return value.map((child) => decodeNode(child, depth + 1, budget));
}

function object(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("plugin UI node must be an object");
  return value as Record<string, unknown>;
}

function string(value: unknown): string {
  if (typeof value !== "string") throw new Error("plugin UI value must be text");
  return value;
}

function text(value: unknown, budget: Budget): string {
  const result = string(value);
  budget.text += result.length;
  if (budget.text > MAX_TEXT) throw new Error("plugin UI exceeds text limits");
  return result;
}

function number(value: unknown): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value)) throw new Error("plugin UI value must be an integer");
  return value;
}

function optionalText(value: unknown, limit: number): string | undefined {
  if (value === undefined) return undefined;
  const result = string(value);
  if (result.length > limit) throw new Error("plugin UI value is too long");
  return result;
}

function optionalOneOf<T extends string>(value: unknown, values: T[]): T | undefined {
  if (value === undefined) return undefined;
  const result = string(value);
  if (!values.includes(result as T)) throw new Error("plugin UI option is invalid");
  return result as T;
}
