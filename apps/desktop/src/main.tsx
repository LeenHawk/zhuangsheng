import { lazy, StrictMode, Suspense, useCallback, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";

import { type UiExperienceMode } from "@zhuangsheng/api-client";
import {
  applyApplicationPreferences,
  AppShell,
  loadApplicationPreferences,
  SurfacePlaceholder,
} from "@zhuangsheng/domain-ui";

import { LocalStories } from "./local-stories";
import { LocalSettings } from "./local-settings";
import { LocalMemory } from "./local-memory";
import { LocalLibrary } from "./local-library";
import { LocalArtifacts } from "./local-artifacts";
import { LocalContexts } from "./local-contexts";
import "../../web/src/styles.css";

const LocalRuns = lazy(async () => ({ default: (await import("./local-runs")).LocalRuns }));

function DesktopApp() {
  const [mode, setMode] = useState<UiExperienceMode>(() =>
    localStorage.getItem("zhuangsheng.uiMode") === "expert" ? "expert" : "user");
  const [section, setSection] = useState<"stories" | "library" | "memory" | "settings" | "studio" | "runs" | "contexts" | "artifacts">(
    () => loadApplicationPreferences().startPage,
  );
  const [inspectRunId, setInspectRunId] = useState<string | null>(null);
  const [resumeStoryId, setResumeStoryId] = useState<string | null>(null);
  const [inspectContext, setInspectContext] = useState<{ contextId: string; branchId: string } | null>(null);
  const clearInspectRun = useCallback(() => setInspectRunId(null), []);
  const clearResumeStory = useCallback(() => setResumeStoryId(null), []);
  const clearInspectContext = useCallback(() => setInspectContext(null), []);
  useEffect(() => {
    const update = (event: Event) => setMode((event as CustomEvent<{ defaultMode: UiExperienceMode }>).detail.defaultMode);
    window.addEventListener("zhuangsheng:preferences", update);
    return () => window.removeEventListener("zhuangsheng:preferences", update);
  }, []);
  const content = section === "stories"
    ? <LocalStories initialStoryId={resumeStoryId} onStoryOpened={clearResumeStory} onInspectRun={(runId, storyId) => { setInspectRunId(runId); setResumeStoryId(storyId); setSection("runs"); }} onConfigure={() => setSection("settings")} />
    : section === "runs"
      ? <Suspense fallback={<SurfacePlaceholder label="本地 Run" title="正在加载运行诊断" description="正在读取固定 Graph 与 durable trace。" />}><LocalRuns initialRunId={inspectRunId} onRunOpened={clearInspectRun} onOpenContext={(contextId, branchId) => { setInspectContext({ contextId, branchId }); setSection("contexts"); }} onReturnToStory={() => setSection("stories")} /></Suspense>
      : section === "settings"
        ? <LocalSettings />
        : section === "memory"
          ? <LocalMemory />
          : section === "library"
            ? <LocalLibrary onOpenSettings={() => setSection("settings")} onOpenArtifacts={() => setSection("artifacts")} />
            : section === "artifacts"
              ? <LocalArtifacts />
              : section === "contexts"
                ? <LocalContexts initial={inspectContext} onOpened={clearInspectContext} />
      : <SurfacePlaceholder label="本地 surface" title="此区域正在接入本地 transport" description="数据仍保存在本机 SQLite；当前可使用完整故事对话与运行列表。" />;
  const changeMode = (next: UiExperienceMode) => {
    localStorage.setItem("zhuangsheng.uiMode", next); setMode(next);
  };
  return <AppShell mode={mode} section={section} onModeChange={changeMode} onSectionChange={setSection}>{content}</AppShell>;
}

const root = document.getElementById("root");
if (!root) throw new Error("Application root is missing");
applyApplicationPreferences(loadApplicationPreferences());
createRoot(root).render(<StrictMode><DesktopApp /></StrictMode>);
