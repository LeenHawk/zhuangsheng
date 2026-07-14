import type { ReactNode } from "react";

import type { UiInlineNode, UiNode, UiTone } from "./types";

export function RenderPluginNodes({ nodes }: { nodes: UiNode[] }) {
  return <>{nodes.map((node, index) => <Node key={index} node={node} />)}</>;
}

function Node({ node }: { node: UiNode }): ReactNode {
  if (node.type === "text" || node.type === "badge" || node.type === "link") {
    return <Inline node={node} />;
  }
  if (node.type === "paragraph") {
    const align = node.align === "center" ? "text-center" : node.align === "end" ? "text-right" : "text-left";
    return <p className={`whitespace-pre-wrap leading-7 ${align}`}>{inlineChildren(node.children)}</p>;
  }
  if (node.type === "heading") {
    const className = node.level === 1 ? "text-xl font-bold" : node.level === 2 ? "text-lg font-semibold" : "font-semibold";
    if (node.level === 1) return <h2 className={className}>{inlineChildren(node.children)}</h2>;
    if (node.level === 2) return <h3 className={className}>{inlineChildren(node.children)}</h3>;
    return <h4 className={className}>{inlineChildren(node.children)}</h4>;
  }
  if (node.type === "quote") {
    return <blockquote className="border-l-2 border-accent/40 pl-3 text-secondary"><RenderPluginNodes nodes={node.children} /></blockquote>;
  }
  if (node.type === "code") {
    return <pre className="overflow-x-auto rounded-xl bg-elevated p-3 font-mono text-xs"><code>{node.text}</code></pre>;
  }
  if (node.type === "divider") return <hr className="border-default" />;
  const gap = node.gap === "large" ? "space-y-4" : node.gap === "small" ? "space-y-1" : "space-y-2";
  return <div className={gap}><RenderPluginNodes nodes={node.children} /></div>;
}

function Inline({ node }: { node: UiInlineNode }) {
  if (node.type === "link") {
    return <a className="text-accent underline underline-offset-2" href={node.href} target="_blank" rel="noreferrer">{node.text}</a>;
  }
  if (node.type === "badge") {
    return <span className={`mx-0.5 inline-flex rounded-full px-2 py-0.5 text-xs ${tone(node.tone)}`}>{node.text}</span>;
  }
  const emphasis = node.emphasis === "strong" ? "font-semibold" : node.emphasis === "italic" ? "italic" : "";
  return <span className={`${tone(node.tone)} ${emphasis}`}>{node.text}</span>;
}

const inlineChildren = (children: UiInlineNode[]) => children.map((node, index) => <Inline key={index} node={node} />);

const tone = (value?: UiTone) => ({
  default: "text-primary", muted: "text-muted", accent: "text-accent",
  warning: "text-warning", danger: "text-danger",
}[value ?? "default"]);
