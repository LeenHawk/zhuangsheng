import type { ReactNode } from "react";
import { BookOpen, Brain, FileArchive, GitBranch, LockKeyhole, LockKeyholeOpen, Orbit, Settings, Sparkles, Wifi, WifiOff, Workflow } from "lucide-react";

import type { SecretStoreStatusView, UiExperienceMode } from "@zhuangsheng/api-client";
import { Badge, cn } from "@zhuangsheng/ui";
import { CommandPalette } from "./command-palette";
import { usePlatformCapabilities } from "./platform-capabilities";

type Section = "stories" | "library" | "memory" | "artifacts" | "studio" | "runs" | "contexts" | "settings";

interface AppShellProps {
  mode: UiExperienceMode;
  section: Section;
  status: AppShellStatus;
  onModeChange: (mode: UiExperienceMode) => void;
  onSectionChange: (section: Section) => void;
  children: ReactNode;
}

export interface AppShellStatus {
  connection: "online" | "offline" | "unknown";
  secretStore: SecretStoreStatusView | null;
}

const userNavigation = [
  { id: "stories" as const, label: "故事", icon: BookOpen },
  { id: "library" as const, label: "资料库", icon: FileArchive },
  { id: "memory" as const, label: "记忆", icon: Brain },
  { id: "settings" as const, label: "设置", icon: Settings },
];

const expertNavigation = [
  ...userNavigation,
  { id: "contexts" as const, label: "Context", icon: GitBranch },
  { id: "studio" as const, label: "Agent Studio", icon: Workflow },
  { id: "runs" as const, label: "运行与 Trace", icon: GitBranch },
  { id: "artifacts" as const, label: "Artifacts", icon: FileArchive },
];

export function AppShell({ mode, section, status, onModeChange, onSectionChange, children }: AppShellProps) {
  const navigation = mode === "expert" ? expertNavigation : userNavigation;
  const platform = usePlatformCapabilities();
  return (
    <div className="min-h-screen bg-canvas text-primary">
      <header className="sticky top-0 z-30 border-b border-default/80 bg-canvas/90 backdrop-blur-xl">
        <div className="mx-auto flex h-16 max-w-[1600px] items-center gap-3 px-4 lg:px-6">
          <div className="flex min-w-0 items-center gap-3">
            <div className="grid size-9 place-items-center rounded-xl bg-accent text-accent-contrast shadow-glow" aria-hidden="true">
              <Orbit className="size-5" />
            </div>
            <div className="min-w-0">
              <div className="truncate font-display text-base font-bold tracking-tight">庄生</div>
              <div className="hidden text-[11px] text-muted sm:block">Agentic Role Play</div>
            </div>
          </div>
          <nav className="ml-3 hidden flex-1 items-center gap-1 md:flex" aria-label="主导航">
            {navigation.map(({ id, label, icon: Icon }) => (
              <button key={id} onClick={() => onSectionChange(id)} className={cn("flex min-h-10 items-center gap-2 rounded-xl px-3 text-sm font-medium text-secondary transition-colors hover:bg-elevated hover:text-primary", section === id && "bg-elevated text-primary")} aria-current={section === id ? "page" : undefined}>
                <Icon className="size-4" aria-hidden="true" />{label}
              </button>
            ))}
          </nav>
          <CommandPalette items={navigation.map(({ id, label }) => ({ id, label }))} onSelect={onSectionChange} />
          <Badge className="hidden lg:inline-flex" tone={platform.localFirst ? "success" : "info"}>
            {platform.localFirst ? "本地 SQLite" : "Web 服务"}
          </Badge>
          <ConnectionBadge status={status.connection} localFirst={platform.localFirst} />
          <SecretBadge status={status.secretStore} />
          <div className="ml-auto flex items-center gap-2 rounded-xl border border-default bg-surface p-1" aria-label="界面模式">
            {(["user", "expert"] as const).map((value) => (
              <button key={value} onClick={() => onModeChange(value)} className={cn("min-h-8 rounded-lg px-3 text-xs font-semibold text-muted transition-colors", mode === value && "bg-elevated text-primary shadow-sm")} aria-pressed={mode === value}>
                {value === "user" ? "用户模式" : "专家模式"}
              </button>
            ))}
          </div>
        </div>
      </header>
      {mode === "expert" && (
        <div className="border-b border-info/20 bg-info/5 px-4 py-2 text-center text-xs text-info">
          <span className="inline-flex items-center gap-1.5"><Sparkles className="size-3.5" />专家模式只改变信息投影，不授予额外权限。</span>
        </div>
      )}
      <main className="mx-auto max-w-[1600px] px-4 py-6 lg:px-6 lg:py-8">{children}</main>
      <nav className="fixed inset-x-3 bottom-3 z-40 flex justify-around rounded-2xl border border-default bg-elevated/95 p-1.5 shadow-panel backdrop-blur-xl md:hidden" aria-label="移动导航">
        {navigation.slice(0, 4).map(({ id, label, icon: Icon }) => (
          <button key={id} onClick={() => onSectionChange(id)} className={cn("flex min-h-11 min-w-16 flex-col items-center justify-center gap-0.5 rounded-xl text-[11px] text-muted", section === id && "bg-surface text-primary")}><Icon className="size-4" />{label}</button>
        ))}
      </nav>
    </div>
  );
}

function ConnectionBadge({ status, localFirst }: { status: AppShellStatus["connection"]; localFirst: boolean }) {
  const label = status === "online" ? (localFirst ? "本地存储可用" : "服务已连接") : status === "offline" ? "当前离线" : "连接状态未知";
  const Icon = status === "online" ? Wifi : WifiOff;
  return <Badge aria-label={label} title={label} className="gap-1 px-2" tone={status === "online" ? "success" : status === "offline" ? "warning" : "neutral"}><Icon className="size-3.5" /><span className="hidden xl:inline">{label}</span></Badge>;
}

function SecretBadge({ status }: { status: SecretStoreStatusView | null }) {
  const unlocked = status?.initialized === true && !status.locked;
  const label = status === null ? "Secret 状态未知" : !status.initialized ? "Secret 未初始化" : status.locked ? "Secret 已锁定" : "Secret 已解锁";
  const Icon = unlocked ? LockKeyholeOpen : LockKeyhole;
  return <Badge aria-label={label} title={label} className="gap-1 px-2" tone={unlocked ? "success" : status?.locked ? "warning" : "neutral"}><Icon className="size-3.5" /><span className="hidden xl:inline">{label}</span></Badge>;
}

export function SurfacePlaceholder({ label, title, description }: { label: string; title: string; description: string }) {
  return <div className="mx-auto max-w-3xl py-16 text-center"><Badge tone="info">{label}</Badge><h1 className="mt-5 font-display text-3xl font-bold">{title}</h1><p className="mx-auto mt-3 max-w-xl text-secondary">{description}</p></div>;
}
