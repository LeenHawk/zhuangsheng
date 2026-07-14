const KEY = "zhuangsheng.plugin.renderer.conversation_message_body";
export const PLUGIN_PREFERENCE_EVENT = "zhuangsheng:plugin-renderer-preference";

export const loadPluginRendererPreference = (): string | null => localStorage.getItem(KEY);

export const savePluginRendererPreference = (value: string | null): void => {
  if (value === null) localStorage.removeItem(KEY);
  else localStorage.setItem(KEY, value);
  window.dispatchEvent(new CustomEvent(PLUGIN_PREFERENCE_EVENT, { detail: value }));
};

export const notifyPluginsChanged = (): void => {
  window.dispatchEvent(new Event("zhuangsheng:plugins-changed"));
};
