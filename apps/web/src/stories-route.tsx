import { useCallback, useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";

import type {
  ConversationRunSpec,
  ConversationView,
  RolePlayGraphOptionView,
} from "@zhuangsheng/api-client";
import { StoryList } from "@zhuangsheng/domain-ui";

import { client, messageFor } from "./api";

export function StoriesRoute() {
  const navigate = useNavigate();
  const [stories, setStories] = useState<ConversationView[]>([]);
  const [templates, setTemplates] = useState<RolePlayGraphOptionView[]>([]);
  const [loading, setLoading] = useState(true);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const reload = useCallback(async () => {
    setLoading(true);
    setError(null);
    const [storiesResult, templatesResult] = await Promise.allSettled([
      client.listConversations(),
      client.listRolePlayGraphOptions(),
    ]);
    const errors: string[] = [];
    if (storiesResult.status === "fulfilled") {
      setStories(storiesResult.value.items);
    } else {
      errors.push(messageFor(storiesResult.reason));
    }
    if (templatesResult.status === "fulfilled") {
      setTemplates(templatesResult.value);
    } else {
      errors.push(messageFor(templatesResult.reason));
    }
    if (errors.length > 0) setError(errors.join("；"));
    setLoading(false);
  }, []);
  useEffect(() => { void reload(); }, [reload]);

  const create = async (title: string | undefined, defaultRun: ConversationRunSpec) => {
    setPending(true);
    setError(null);
    try {
      const story = await client.createConversation({ title, defaultRun });
      navigate(`/stories/${story.id}`);
    } catch (cause) {
      setError(messageFor(cause));
      throw cause;
    } finally {
      setPending(false);
    }
  };
  return (
    <StoryList
      stories={stories}
      templates={templates}
      loading={loading}
      pending={pending}
      error={error}
      onReload={() => void reload()}
      onCreate={create}
      onOpen={(id) => navigate(`/stories/${id}`)}
      onConfigure={() => navigate("/settings")}
    />
  );
}
