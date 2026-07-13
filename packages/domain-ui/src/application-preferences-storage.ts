import type { ApplicationPreferenceView } from "./application-settings";

const key = "zhuangsheng.uiPreferences.v1";

export const defaultApplicationPreferences: ApplicationPreferenceView = {
  theme: "system", fontScale: "normal", density: "comfortable", reducedMotion: false,
  defaultMode: "user", startPage: "stories", streamPresentation: true, autoScroll: true,
  candidateGestures: true, notifications: false, confirmSensitiveDownloads: true,
  localBackups: false, diagnostics: false,
};

export function loadApplicationPreferences(): ApplicationPreferenceView {
  try {
    const stored = JSON.parse(localStorage.getItem(key) ?? "null") as Partial<ApplicationPreferenceView> | null;
    return stored ? { ...defaultApplicationPreferences, ...stored } : defaultApplicationPreferences;
  } catch {
    return defaultApplicationPreferences;
  }
}

export function saveApplicationPreferences(value: ApplicationPreferenceView) {
  localStorage.setItem(key, JSON.stringify(value));
  localStorage.setItem("zhuangsheng.uiMode", value.defaultMode);
  applyApplicationPreferences(value);
  window.dispatchEvent(new CustomEvent("zhuangsheng:preferences", { detail: value }));
}

export function applyApplicationPreferences(value: ApplicationPreferenceView) {
  const root = document.documentElement;
  root.dataset.theme = value.theme;
  root.dataset.density = value.density;
  root.dataset.reducedMotion = value.reducedMotion ? "true" : "false";
  root.dataset.confirmSensitiveDownloads = value.confirmSensitiveDownloads ? "true" : "false";
  root.style.fontSize = value.fontScale === "small" ? "15px" : value.fontScale === "large" ? "18px" : "16px";
}
