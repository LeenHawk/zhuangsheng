import { useCallback, useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";

import type { ConversationView } from "@zhuangsheng/api-client";
import { StoryList } from "@zhuangsheng/domain-ui";

import { client, messageFor } from "./api";

export function StoriesRoute() {
  const navigate = useNavigate();
  const [stories, setStories] = useState<ConversationView[]>([]);
  const [loading, setLoading] = useState(true);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const reload = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setStories((await client.listConversations()).items);
    } catch (cause) {
      setError(messageFor(cause));
    } finally {
      setLoading(false);
    }
  }, []);
  useEffect(() => { void reload(); }, [reload]);

  const create = async (title?: string) => {
    setPending(true);
    setError(null);
    try {
      const story = await client.createConversation({ title });
      navigate(`/stories/${story.id}`);
    } catch (cause) {
      setError(messageFor(cause));
    } finally {
      setPending(false);
    }
  };
  return (
    <StoryList
      stories={stories}
      loading={loading}
      pending={pending}
      error={error}
      onReload={() => void reload()}
      onCreate={create}
      onOpen={(id) => navigate(`/stories/${id}`)}
    />
  );
}
