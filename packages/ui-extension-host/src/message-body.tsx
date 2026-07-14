import { useEffect, useState, type ReactNode } from "react";

import type { PluginMessageRole } from "@zhuangsheng/api-client";

import { usePluginHost } from "./context";
import { RenderPluginNodes } from "./render-nodes";
import type { UiNode } from "./types";

export function PluginMessageBody({
  messageId, role, source, text, streaming = false, fallback,
}: {
  messageId: string;
  role: PluginMessageRole;
  source: string;
  text: string;
  streaming?: boolean;
  fallback: ReactNode;
}) {
  const host = usePluginHost();
  const [result, setResult] = useState<{ applied: boolean; nodes: UiNode[] | null }>({ applied: false, nodes: null });
  useEffect(() => {
    let stale = false;
    setResult({ applied: false, nodes: null });
    void host.render({
      slot: "conversation_message_body",
      message: { id: messageId, role, source, text, reasoning: null, streaming },
    }).then((nodes) => { if (!stale) setResult({ applied: true, nodes }); });
    return () => { stale = true; };
  }, [host.render, messageId, role, source, streaming, text]);
  if (!result.applied || result.nodes === null) return fallback;
  return <RenderPluginNodes nodes={result.nodes} />;
}
