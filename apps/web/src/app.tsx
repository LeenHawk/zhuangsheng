import { lazy, Suspense, useState } from "react";
import { Navigate, Route, Routes, useLocation, useNavigate } from "react-router-dom";

import type { UiExperienceMode } from "@zhuangsheng/api-client";
import { AppShell, SurfacePlaceholder } from "@zhuangsheng/domain-ui";

import { StoriesRoute } from "./stories-route";
import { StoryRoute } from "./story-route";
import { RunRoute, RunsRoute } from "./run-routes";

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
      : location.pathname.startsWith("/memory")
        ? "memory"
        : location.pathname.startsWith("/settings")
          ? "settings"
          : "stories";
  const changeSection = (next: typeof section) =>
    navigate(next === "stories" ? "/stories" : next === "studio" ? "/expert/studio" : next === "runs" ? "/expert/runs" : `/${next}`);
  return (
    <AppShell mode={mode} section={section} onModeChange={setMode} onSectionChange={changeSection}>
      <Routes>
        <Route path="/" element={<Navigate to="/stories" replace />} />
        <Route path="/stories" element={<StoriesRoute />} />
        <Route path="/stories/:conversationId" element={<StoryRoute />} />
        <Route path="/memory" element={<SurfacePlaceholder label="用户功能" title="记忆" description="长期记忆与 proposal 将从 MemoryManager 的权威 projection 加载；当前页面不会直接修改数据库记录。" />} />
        <Route path="/settings" element={<SurfacePlaceholder label="用户功能" title="设置" description="模型、Channel、Secret Store 与故事默认配置会按各自的版本和权限边界接入。" />} />
        <Route path="/expert/studio" element={<Suspense fallback={<SurfacePlaceholder label="专家 surface" title="正在加载 Agent Studio" description="正在加载 Graph 编辑能力。" />}><GraphStudioRoute /></Suspense>} />
        <Route path="/expert/runs" element={<RunsRoute />} />
        <Route path="/expert/runs/:runId" element={<RunRoute />} />
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
