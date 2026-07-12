import { lazy, StrictMode, Suspense, useCallback, useState } from "react";
import { createRoot } from "react-dom/client";

import { type UiExperienceMode } from "@zhuangsheng/api-client";
import { AppShell, SurfacePlaceholder } from "@zhuangsheng/domain-ui";

import { LocalStories } from "./local-stories";
import { LocalSettings } from "./local-settings";
import { LocalMemory } from "./local-memory";
import { LocalLibrary } from "./local-library";
import "../../web/src/styles.css";

const LocalRuns = lazy(async () => ({ default: (await import("./local-runs")).LocalRuns }));

function DesktopApp() {
  const [mode, setMode] = useState<UiExperienceMode>(() =>
    localStorage.getItem("zhuangsheng.uiMode") === "expert" ? "expert" : "user");
  const [section, setSection] = useState<"stories" | "library" | "memory" | "settings" | "studio" | "runs" | "contexts" | "artifacts">("stories");
  const [inspectRunId, setInspectRunId] = useState<string | null>(null);
  const [resumeStoryId, setResumeStoryId] = useState<string | null>(null);
  const clearInspectRun = useCallback(() => setInspectRunId(null), []);
  const clearResumeStory = useCallback(() => setResumeStoryId(null), []);
  const content = section === "stories"
    ? <LocalStories initialStoryId={resumeStoryId} onStoryOpened={clearResumeStory} onInspectRun={(runId, storyId) => { setInspectRunId(runId); setResumeStoryId(storyId); setSection("runs"); }} onConfigure={() => setSection("settings")} />
    : section === "runs"
      ? <Suspense fallback={<SurfacePlaceholder label="本地 Run" title="正在加载运行诊断" description="正在读取固定 Graph 与 durable trace。" />}><LocalRuns initialRunId={inspectRunId} onRunOpened={clearInspectRun} onOpenContext={() => setSection("contexts")} onReturnToStory={() => setSection("stories")} /></Suspense>
      : section === "settings"
        ? <LocalSettings />
        : section === "memory"
          ? <LocalMemory />
          : section === "library"
            ? <LocalLibrary onOpenSettings={() => setSection("settings")} onOpenArtifacts={() => setSection("artifacts")} />
      : <SurfacePlaceholder label="本地 surface" title="此区域正在接入本地 transport" description="数据仍保存在本机 SQLite；当前可使用完整故事对话与运行列表。" />;
  const changeMode = (next: UiExperienceMode) => {
    localStorage.setItem("zhuangsheng.uiMode", next); setMode(next);
  };
  return <AppShell mode={mode} section={section} onModeChange={changeMode} onSectionChange={setSection}>{content}</AppShell>;
}

const root = document.getElementById("root");
if (!root) throw new Error("Application root is missing");
createRoot(root).render(<StrictMode><DesktopApp /></StrictMode>);
