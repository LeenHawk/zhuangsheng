import { lazy, Suspense, useEffect, useState } from "react";
import { Navigate, Route, Routes, useLocation, useNavigate } from "react-router-dom";

import { webPlatformCapabilities, type UiExperienceMode } from "@zhuangsheng/api-client";
import { AppShell, PlatformCapabilitiesProvider, SurfacePlaceholder, useAppShellStatus } from "@zhuangsheng/domain-ui";

import { loadUiPreferences } from "./ui-preferences";
import { client } from "./api";

const loadWebSecretStore = () => client.secrets.status();

const StoriesRoute = lazy(async () => ({ default: (await import("./stories-route")).StoriesRoute }));
const StoryRoute = lazy(async () => ({ default: (await import("./story-route")).StoryRoute }));
const RunsRoute = lazy(async () => ({ default: (await import("./run-routes")).RunsRoute }));
const RunRoute = lazy(async () => ({ default: (await import("./run-routes")).RunRoute }));
const SettingsRoute = lazy(async () => ({ default: (await import("./settings-route")).SettingsRoute }));
const MemoryRoute = lazy(async () => ({ default: (await import("./memory-route")).MemoryRoute }));
const ArtifactsRoute = lazy(async () => ({ default: (await import("./artifacts-route")).ArtifactsRoute }));
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
  const shellStatus = useAppShellStatus(loadWebSecretStore, false);
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
    <PlatformCapabilitiesProvider value={webPlatformCapabilities}><AppShell mode={mode} section={section} status={shellStatus} onModeChange={setMode} onSectionChange={changeSection}>
      <Suspense fallback={<SurfacePlaceholder label="页面" title="正在读取权威状态" description="正在加载领域投影与可用操作。" />}><Routes>
        <Route path="/" element={<Navigate to={`/${startPage}`} replace />} />
        <Route path="/stories" element={<StoriesRoute />} />
        <Route path="/stories/:conversationId" element={<StoryRoute />} />
        <Route path="/memory" element={<MemoryRoute />} />
        <Route path="/library" element={<LibraryRoute />} />
        <Route path="/settings" element={<SettingsRoute />} />
        <Route path="/expert/studio" element={<GraphStudioRoute />} />
        <Route path="/expert/runs" element={<RunsRoute />} />
        <Route path="/expert/runs/:runId" element={<RunRoute />} />
        <Route path="/expert/contexts" element={<ContextsRoute />} />
        <Route path="/expert/contexts/:contextId" element={<ContextRoute />} />
        <Route path="/expert/artifacts" element={<ArtifactsRoute />} />
        <Route path="*" element={<Navigate to="/stories" replace />} />
      </Routes></Suspense>
    </AppShell></PlatformCapabilitiesProvider>
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
