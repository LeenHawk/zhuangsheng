import { lazy, Suspense, useEffect, useState } from "react";
import { Navigate, Route, Routes, useLocation, useNavigate } from "react-router-dom";

import type { UiExperienceMode } from "@zhuangsheng/api-client";
import { AppShell, SurfacePlaceholder } from "@zhuangsheng/domain-ui";

import { StoriesRoute } from "./stories-route";
import { StoryRoute } from "./story-route";
import { RunRoute, RunsRoute } from "./run-routes";
import { SettingsRoute } from "./settings-route";
import { MemoryRoute } from "./memory-route";
import { ArtifactsRoute } from "./artifacts-route";
import { loadUiPreferences } from "./ui-preferences";

const GraphStudioRoute = lazy(async () => {
  const module = await import("./graph-routes");
  return { default: module.GraphStudioRoute };
});
const ContextsRoute = lazy(async () => ({ default: (await import("./context-routes")).ContextsRoute }));
const ContextRoute = lazy(async () => ({ default: (await import("./context-routes")).ContextRoute }));
const LibraryRoute = lazy(async () => ({ default: (await import("./library-route")).LibraryRoute }));

export function App() {
  const navigate = useNavigate();
  const location = useLocation();
  const [mode, setMode] = useModePreference();
  const [startPage, setStartPage] = useState(loadUiPreferences().startPage);
  useEffect(() => {
    const update = (event: Event) => {
      const value = (event as CustomEvent<{ defaultMode: UiExperienceMode; startPage: "stories" | "library" }>).detail;
      setMode(value.defaultMode); setStartPage(value.startPage);
    };
    window.addEventListener("zhuangsheng:preferences", update);
    return () => window.removeEventListener("zhuangsheng:preferences", update);
  }, [setMode]);
  const section = location.pathname.startsWith("/expert/studio")
    ? "studio"
    : location.pathname.startsWith("/expert/runs")
      ? "runs"
      : location.pathname.startsWith("/expert/contexts")
        ? "contexts"
      : location.pathname.startsWith("/expert/artifacts")
        ? "artifacts"
      : location.pathname.startsWith("/memory")
        ? "memory"
        : location.pathname.startsWith("/library")
          ? "library"
        : location.pathname.startsWith("/settings")
          ? "settings"
          : "stories";
  const changeSection = (next: typeof section) =>
    navigate(next === "stories" ? "/stories" : next === "studio" ? "/expert/studio" : next === "runs" ? "/expert/runs" : next === "contexts" ? "/expert/contexts" : next === "artifacts" ? "/expert/artifacts" : `/${next}`);
  return (
    <AppShell mode={mode} section={section} onModeChange={setMode} onSectionChange={changeSection}>
      <Routes>
        <Route path="/" element={<Navigate to={`/${startPage}`} replace />} />
        <Route path="/stories" element={<StoriesRoute />} />
        <Route path="/stories/:conversationId" element={<StoryRoute />} />
        <Route path="/memory" element={<MemoryRoute />} />
        <Route path="/library" element={<Suspense fallback={<SurfacePlaceholder label="资料库" title="正在读取资料" description="正在加载版本化资源投影。" />}><LibraryRoute /></Suspense>} />
        <Route path="/settings" element={<SettingsRoute />} />
        <Route path="/expert/studio" element={<Suspense fallback={<SurfacePlaceholder label="专家 surface" title="正在加载 Agent Studio" description="正在加载 Graph 编辑能力。" />}><GraphStudioRoute /></Suspense>} />
        <Route path="/expert/runs" element={<RunsRoute />} />
        <Route path="/expert/runs/:runId" element={<RunRoute />} />
        <Route path="/expert/contexts" element={<Suspense fallback={<SurfacePlaceholder label="Expert Context" title="正在读取 Context" description="正在加载 branch 与 commit projection。" />}><ContextsRoute /></Suspense>} />
        <Route path="/expert/contexts/:contextId" element={<Suspense fallback={<SurfacePlaceholder label="Expert Context" title="正在读取 Context" description="正在加载 branch 与 commit projection。" />}><ContextRoute /></Suspense>} />
        <Route path="/expert/artifacts" element={<ArtifactsRoute />} />
        <Route path="*" element={<Navigate to="/stories" replace />} />
      </Routes>
    </AppShell>
  );
}

function useModePreference(): [UiExperienceMode, (mode: UiExperienceMode) => void] {
  const [mode, setMode] = useState<UiExperienceMode>(() =>
    localStorage.getItem("zhuangsheng.uiMode") === "expert" ? "expert" : "user",
  );
  return [mode, (next) => {
    localStorage.setItem("zhuangsheng.uiMode", next);
    setMode(next);
  }];
}
