import { useCallback, useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";

import {
  createIdempotencyKey,
  createOpeningConversation,
  stringifyJsonExact,
  type ConversationRunSpec,
  type ConversationAttentionView,
  type ConversationView,
  type RolePlayGraphOptionView,
  type RolePlaySettingsView,
  type SecretStoreStatusView,
} from "@zhuangsheng/api-client";
import { notifyShellStatusChanged, StoryList } from "@zhuangsheng/domain-ui";

import { client, messageFor } from "./api";

export function StoriesRoute() {
  const navigate = useNavigate();
  const [stories, setStories] = useState<ConversationView[]>([]);
  const [attention, setAttention] = useState<ConversationAttentionView[]>([]);
  const [templates, setTemplates] = useState<RolePlayGraphOptionView[]>([]);
  const [templateSettings, setTemplateSettings] = useState<Record<string, RolePlaySettingsView | null>>({});
  const [secretStatus, setSecretStatus] = useState<SecretStoreStatusView | null>(null);
  const [loading, setLoading] = useState(true);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const reload = useCallback(async () => {
    setLoading(true);
    setError(null);
    const [storiesResult, templatesResult, secretResult] = await Promise.allSettled([
      client.listConversations(),
      client.listRolePlayGraphOptions(),
      client.secrets.status(),
    ]);
    const errors: string[] = [];
    if (storiesResult.status === "fulfilled") {
      setStories(storiesResult.value.items);
      setAttention(storiesResult.value.attention);
    } else {
      errors.push(messageFor(storiesResult.reason));
    }
    if (templatesResult.status === "fulfilled") {
      setTemplates(templatesResult.value);
      const details = await Promise.allSettled(templatesResult.value.map((template) => client.graphs.getRolePlaySettings(template.revisionId)));
      setTemplateSettings(Object.fromEntries(templatesResult.value.map((template, index) => [template.revisionId, details[index]?.status === "fulfilled" ? details[index].value : null])));
    } else {
      errors.push(messageFor(templatesResult.reason));
    }
    if (secretResult.status === "fulfilled") setSecretStatus(secretResult.value);
    else errors.push(messageFor(secretResult.reason));
    if (errors.length > 0) setError(errors.join("；"));
    setLoading(false);
  }, []);
  useEffect(() => { void reload(); }, [reload]);

  const createKeys = useRef<{ signature: string; conversation: string; turn: string } | null>(null);
  const create = async (title: string | undefined, defaultRun: ConversationRunSpec, openingMessage: string) => {
    const signature = stringifyJsonExact({ title: title ?? null, defaultRun, openingMessage });
    if (createKeys.current?.signature !== signature) createKeys.current = { signature, conversation: createIdempotencyKey(), turn: createIdempotencyKey() };
    const keys = createKeys.current;
    setPending(true);
    setError(null);
    try {
      const { conversation: story } = await createOpeningConversation(client, {
        title, run: defaultRun, openingMessage,
      }, keys);
      createKeys.current = null;
      navigate(`/stories/${story.id}`);
    } catch (cause) {
      setError(messageFor(cause));
      throw cause;
    } finally {
      setPending(false);
    }
  };
  const unlock = async (masterPassword: string, idempotencyKey: string) => {
    const session = await client.secrets.unlock({ masterPassword, idempotencyKey });
    setSecretStatus({ initialized: true, storeId: session.storeId, formatVersion: session.formatVersion, locked: false });
    notifyShellStatusChanged();
  };
  return (
    <StoryList
      stories={stories}
      attention={attention}
      templates={templates}
      templateSettings={templateSettings}
      secretStatus={secretStatus}
      loading={loading}
      pending={pending}
      error={error}
      onReload={() => void reload()}
      onCreate={create}
      onUnlockSecretStore={unlock}
      onOpen={(id) => navigate(`/stories/${id}`)}
      onConfigure={() => navigate("/settings")}
    />
  );
}
