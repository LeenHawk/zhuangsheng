import { useEffect, useState } from "react";
import { Navigate, Route, Routes, useLocation, useNavigate, useParams } from "react-router-dom";

import { ApiError, HttpApiClient, type ConversationTimelineView, type ConversationView, type UiExperienceMode } from "@zhuangsheng/api-client";
import { AppShell, SurfacePlaceholder, StoryDetail, StoryList } from "@zhuangsheng/domain-ui";

const client = new HttpApiClient(import.meta.env.VITE_API_BASE_URL ?? "");

export function App() {
  const navigate = useNavigate();
  const location = useLocation();
  const [mode, setMode] = useModePreference();
  const section = location.pathname.startsWith("/expert/studio") ? "studio" : location.pathname.startsWith("/expert/runs") ? "runs" : location.pathname.startsWith("/memory") ? "memory" : location.pathname.startsWith("/settings") ? "settings" : "stories";
  const changeSection = (next: typeof section) => navigate(next === "stories" ? "/stories" : next === "studio" ? "/expert/studio" : next === "runs" ? "/expert/runs" : `/${next}`);
  return <AppShell mode={mode} section={section} onModeChange={setMode} onSectionChange={changeSection}>
    <Routes>
      <Route path="/" element={<Navigate to="/stories" replace />} />
      <Route path="/stories" element={<StoriesRoute />} />
      <Route path="/stories/:conversationId" element={<StoryRoute />} />
      <Route path="/memory" element={<SurfacePlaceholder label="用户功能" title="记忆" description="长期记忆与 proposal 将从 MemoryManager 的权威 projection 加载；当前页面不会直接修改数据库记录。" />} />
      <Route path="/settings" element={<SurfacePlaceholder label="用户功能" title="设置" description="模型、Channel、Secret Store 与故事默认配置会按各自的版本和权限边界接入。" />} />
      <Route path="/expert/studio" element={<SurfacePlaceholder label="专家 surface" title="Agent Studio" description="GraphDraft、Apply diagnostics 与 React Flow 编辑器将在这里消费同一份 canonical Graph API。" />} />
      <Route path="/expert/runs" element={<SurfacePlaceholder label="专家 surface" title="运行与 Trace" description="这里将按 durable sequence 展示 Run、NodeAttempt、ModelCall、ToolCall 与 Effect，不从 live callback 猜测状态。" />} />
      <Route path="*" element={<Navigate to="/stories" replace />} />
    </Routes>
  </AppShell>;
}

function StoriesRoute() {
  const navigate = useNavigate();
  const [stories, setStories] = useState<ConversationView[]>([]);
  const [loading, setLoading] = useState(true);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const reload = async () => {
    setLoading(true); setError(null);
    try { setStories((await client.listConversations()).items); }
    catch (cause) { setError(messageFor(cause)); }
    finally { setLoading(false); }
  };
  useEffect(() => { void reload(); }, []);
  const create = async (title?: string) => {
    setPending(true); setError(null);
    try { const story = await client.createConversation({ title }); navigate(`/stories/${story.id}`); }
    catch (cause) { setError(messageFor(cause)); }
    finally { setPending(false); }
  };
  return <StoryList stories={stories} loading={loading} pending={pending} error={error} onReload={() => void reload()} onCreate={create} onOpen={(id) => navigate(`/stories/${id}`)} />;
}

function StoryRoute() {
  const { conversationId = "" } = useParams();
  const navigate = useNavigate();
  const [story, setStory] = useState<ConversationView | null>(null);
  const [timeline, setTimeline] = useState<ConversationTimelineView | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const reload = async () => {
    setLoading(true); setError(null);
    try { const [nextStory, nextTimeline] = await Promise.all([client.getConversation(conversationId), client.getTimeline(conversationId)]); setStory(nextStory); setTimeline(nextTimeline); }
    catch (cause) { setError(messageFor(cause)); }
    finally { setLoading(false); }
  };
  useEffect(() => { void reload(); }, [conversationId]);
  return <StoryDetail story={story} timeline={timeline} loading={loading} error={error} onBack={() => navigate("/stories")} onReload={() => void reload()} />;
}

function useModePreference(): [UiExperienceMode, (mode: UiExperienceMode) => void] {
  const [mode, setMode] = useState<UiExperienceMode>(() => localStorage.getItem("zhuangsheng.uiMode") === "expert" ? "expert" : "user");
  return [mode, (next) => { localStorage.setItem("zhuangsheng.uiMode", next); setMode(next); }];
}

function messageFor(cause: unknown): string {
  if (cause instanceof ApiError) return `${cause.body.message}（${cause.body.code} · ${cause.body.traceId}）`;
  return cause instanceof Error ? cause.message : "无法读取服务端响应。";
}
