import { lazy, Suspense, useState } from "react";
import { Navigate, Route, Routes, useLocation, useNavigate } from "react-router-dom";

import type { UiExperienceMode } from "@zhuangsheng/api-client";
import { AppShell, SurfacePlaceholder } from "@zhuangsheng/domain-ui";

import { StoriesRoute } from "./stories-route";
import { StoryRoute } from "./story-route";
import { RunRoute, RunsRoute } from "./run-routes";
import { SettingsRoute } from "./settings-route";
import { MemoryRoute } from "./memory-route";
import { ArtifactsRoute } from "./artifacts-route";

const GraphStudioRoute = lazy(async () => {
  const module = await import("./graph-routes");
  return { default: module.GraphStudioRoute };
});

export function App() {
  const navigate = useNavigate();
  const location = useLocation();
  const [mode, setMode] = useModePreference();
  const section = location.pathname.startsWith("/expert/studio")
    ? "studio"
    : location.pathname.startsWith("/expert/runs")
      ? "runs"
      : location.pathname.startsWith("/expert/artifacts")
        ? "artifacts"
      : location.pathname.startsWith("/memory")
        ? "memory"
        : location.pathname.startsWith("/settings")
          ? "settings"
          : "stories";
  const changeSection = (next: typeof section) =>
    navigate(next === "stories" ? "/stories" : next === "studio" ? "/expert/studio" : next === "runs" ? "/expert/runs" : next === "artifacts" ? "/expert/artifacts" : `/${next}`);
  return (
    <AppShell mode={mode} section={section} onModeChange={setMode} onSectionChange={changeSection}>
      <Routes>
        <Route path="/" element={<Navigate to="/stories" replace />} />
        <Route path="/stories" element={<StoriesRoute />} />
        <Route path="/stories/:conversationId" element={<StoryRoute />} />
        <Route path="/memory" element={<MemoryRoute />} />
        <Route path="/settings" element={<SettingsRoute />} />
        <Route path="/expert/studio" element={<Suspense fallback={<SurfacePlaceholder label="专家 surface" title="正在加载 Agent Studio" description="正在加载 Graph 编辑能力。" />}><GraphStudioRoute /></Suspense>} />
        <Route path="/expert/runs" element={<RunsRoute />} />
        <Route path="/expert/runs/:runId" element={<RunRoute />} />
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
