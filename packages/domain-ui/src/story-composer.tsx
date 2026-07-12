import { useState, type FormEvent } from "react";
import { Send } from "lucide-react";

import { Button, Card, Textarea } from "@zhuangsheng/ui";

interface StoryComposerProps {
  enabled: boolean;
  pending: boolean;
  error: string | null;
  onSubmit: (text: string) => Promise<void>;
}

export function StoryComposer({ enabled, pending, error, onSubmit }: StoryComposerProps) {
  const [text, setText] = useState("");
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    const content = text.trim();
    if (!content || !enabled || pending) return;
    try {
      await onSubmit(content);
      setText("");
    } catch {
      // The command owner renders the typed API error and keeps the draft for retry.
    }
  };
  return (
    <Card className="sticky bottom-20 z-20 mx-auto mt-8 max-w-3xl p-3 shadow-panel md:static">
      <form onSubmit={submit}>
        <label className="sr-only" htmlFor="story-message">继续故事</label>
        <Textarea
          id="story-message"
          className="min-h-24 resize-none border-0 bg-transparent focus:ring-0"
          value={text}
          onChange={(event) => setText(event.target.value)}
          placeholder={enabled ? "写下你的行动、对话或想法…" : "先在右侧选择故事运行模板"}
          maxLength={20_000}
          disabled={!enabled || pending}
        />
        <div className="flex items-center justify-between gap-3 border-t border-default px-1 pt-3">
          <p className="text-xs text-muted">消息会先持久化，再启动角色回复。</p>
          <Button type="submit" disabled={!enabled || pending || !text.trim()}>
            <Send className="size-4" />{pending ? "已提交，正在准备…" : "发送"}
          </Button>
        </div>
      </form>
      {error && <p className="mt-3 border-t border-danger/20 px-1 pt-3 text-sm text-danger">{error}</p>}
    </Card>
  );
}
