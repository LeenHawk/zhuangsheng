import { useState } from "react";
import { ArrowRight, BookOpen, Feather, Plus, RefreshCw } from "lucide-react";

import type {
  ConversationRunSpec,
  ConversationView,
  RolePlayGraphOptionView,
  RolePlaySettingsView,
  SecretStoreStatusView,
} from "@zhuangsheng/api-client";
import { Badge, Button, Card } from "@zhuangsheng/ui";
import { NewStoryWizard } from "./new-story-wizard";

interface StoryListProps {
  stories: ConversationView[];
  templates: RolePlayGraphOptionView[];
  templateSettings: Record<string, RolePlaySettingsView | null>;
  secretStatus: SecretStoreStatusView | null;
  loading: boolean;
  pending: boolean;
  error: string | null;
  onReload: () => void;
  onCreate: (title: string | undefined, run: ConversationRunSpec, openingMessage: string) => Promise<void>;
  onUnlockSecretStore: (password: string, idempotencyKey: string) => Promise<void>;
  onOpen: (id: string) => void;
  onConfigure: () => void;
}

export function StoryList({ stories, templates, templateSettings, secretStatus, loading, pending, error, onReload, onCreate, onUnlockSecretStore, onOpen, onConfigure }: StoryListProps) {
  const [showCreate, setShowCreate] = useState(false);
  const available = templates.filter((template) => template.replyOutputKeys.length > 0 && template.compatibility.mode !== "expert_only");
  return (
    <div className="mx-auto max-w-6xl pb-24">
      <section className="relative overflow-hidden rounded-3xl border border-default bg-hero px-6 py-10 shadow-panel sm:px-10 lg:py-14">
        <div className="relative z-10 max-w-2xl">
          <Badge tone="running"><Feather className="mr-1 size-3" />持续的角色、记忆与分支</Badge>
          <h1 className="mt-5 font-display text-4xl font-bold leading-tight tracking-tight sm:text-5xl">从一个故事开始，<br />让角色真正持续存在。</h1>
          <p className="mt-5 max-w-xl text-base leading-7 text-secondary">每次对话都落在可恢复的故事分支上。候选、记忆与行动不会因为刷新页面而消失。</p>
          <div className="mt-7 flex flex-wrap gap-3"><Button onClick={() => available.length > 0 ? setShowCreate(true) : onConfigure()}><Plus className="size-4" />{available.length > 0 ? "新建故事" : "配置首个 Agent"}</Button><Button variant="secondary" onClick={onReload}><RefreshCw className="size-4" />刷新</Button></div>
        </div>
        <div className="story-orb absolute -right-24 -top-28 size-96 rounded-full" aria-hidden="true" />
      </section>
      {showCreate && <NewStoryWizard templates={available} settings={templateSettings} secretStatus={secretStatus} pending={pending} onSubmit={async (...input) => { await onCreate(...input); setShowCreate(false); }} onUnlock={onUnlockSecretStore} onClose={() => setShowCreate(false)} />}
      <div className="mt-10 flex items-end justify-between"><div><p className="text-xs font-bold uppercase tracking-[0.18em] text-muted">Your stories</p><h2 className="mt-2 font-display text-2xl font-bold">最近的故事</h2></div>{stories.length > 0 && <span className="text-sm text-muted">{stories.length} 个</span>}</div>
      {error && <Card className="mt-5 border-danger/30 p-4 text-sm text-danger"><p>{error}</p><Button className="mt-3" size="compact" variant="secondary" onClick={onReload}>重试</Button></Card>}
      {loading ? <div className="mt-5 grid gap-4 sm:grid-cols-2 lg:grid-cols-3" aria-label="正在加载故事">{[0,1,2].map((item) => <div key={item} className="h-44 animate-pulse rounded-2xl bg-elevated" />)}</div> : stories.length === 0 ? <Card className="mt-5 grid min-h-56 place-items-center p-8 text-center"><div><div className="mx-auto grid size-12 place-items-center rounded-2xl bg-elevated text-secondary"><BookOpen className="size-5" /></div><h3 className="mt-4 font-semibold">还没有故事</h3><p className="mt-1 text-sm text-muted">创建故事后，服务端会同时建立 Context、根分支与首个可恢复 head。</p></div></Card> : <div className="mt-5 grid gap-4 sm:grid-cols-2 lg:grid-cols-3">{stories.map((story) => <button key={story.id} onClick={() => onOpen(story.id)} className="group text-left"><Card className="h-full p-5 transition hover:-translate-y-0.5 hover:border-accent/40 hover:shadow-panel"><div className="flex items-start justify-between"><div className="grid size-11 place-items-center rounded-2xl bg-accent-soft text-accent"><BookOpen className="size-5" /></div><ArrowRight className="size-4 text-muted transition group-hover:translate-x-1 group-hover:text-accent" /></div><h3 className="mt-5 truncate font-display text-lg font-bold">{story.title || "未命名故事"}</h3><p className="mt-2 line-clamp-2 text-sm text-secondary">分支 {shortId(story.activeBranchId)} · head {shortId(story.activeHeadCommitId)}</p><p className="mt-5 text-xs text-muted">{new Date(story.updatedAt).toLocaleString()}</p></Card></button>)}</div>}
    </div>
  );
}

const shortId = (value: string) => value.length > 18 ? `${value.slice(0, 10)}…${value.slice(-5)}` : value;
