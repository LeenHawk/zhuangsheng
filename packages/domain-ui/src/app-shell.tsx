import type { ReactNode } from "react";
import { BookOpen, Brain, FileArchive, GitBranch, Orbit, Settings, Sparkles, Workflow } from "lucide-react";

import type { UiExperienceMode } from "@zhuangsheng/api-client";
import { Badge, cn } from "@zhuangsheng/ui";

type Section = "stories" | "memory" | "artifacts" | "studio" | "runs" | "contexts" | "settings";

interface AppShellProps {
  mode: UiExperienceMode;
  section: Section;
  onModeChange: (mode: UiExperienceMode) => void;
  onSectionChange: (section: Section) => void;
  children: ReactNode;
}

const userNavigation = [
  { id: "stories" as const, label: "故事", icon: BookOpen },
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

export function AppShell({ mode, section, onModeChange, onSectionChange, children }: AppShellProps) {
  const navigation = mode === "expert" ? expertNavigation : userNavigation;
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

export function SurfacePlaceholder({ label, title, description }: { label: string; title: string; description: string }) {
  return <div className="mx-auto max-w-3xl py-16 text-center"><Badge tone="info">{label}</Badge><h1 className="mt-5 font-display text-3xl font-bold">{title}</h1><p className="mx-auto mt-3 max-w-xl text-secondary">{description}</p></div>;
}
