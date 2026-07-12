export interface UiPreferences {
  theme: "system" | "light" | "dark" | "contrast";
  fontScale: "small" | "normal" | "large";
  density: "comfortable" | "compact";
  reducedMotion: boolean;
  defaultMode: "user" | "expert";
  startPage: "stories" | "library";
  streamPresentation: boolean;
  autoScroll: boolean;
  candidateGestures: boolean;
  notifications: boolean;
  confirmSensitiveDownloads: boolean;
  localBackups: boolean;
  diagnostics: boolean;
}

const key = "zhuangsheng.uiPreferences.v1";
export const defaultUiPreferences: UiPreferences = {
  theme: "system", fontScale: "normal", density: "comfortable", reducedMotion: false,
  defaultMode: "user", startPage: "stories", streamPresentation: true, autoScroll: true,
  candidateGestures: true, notifications: false, confirmSensitiveDownloads: true,
  localBackups: false, diagnostics: false,
};

export function loadUiPreferences(): UiPreferences {
  try {
    const stored = JSON.parse(localStorage.getItem(key) ?? "null") as Partial<UiPreferences> | null;
    return stored ? { ...defaultUiPreferences, ...stored } : defaultUiPreferences;
  } catch { return defaultUiPreferences; }
}

export function saveUiPreferences(value: UiPreferences) {
  localStorage.setItem(key, JSON.stringify(value));
  localStorage.setItem("zhuangsheng.uiMode", value.defaultMode);
  applyUiPreferences(value);
  window.dispatchEvent(new CustomEvent("zhuangsheng:preferences", { detail: value }));
}

export function applyUiPreferences(value: UiPreferences) {
  const root = document.documentElement;
  root.dataset.theme = value.theme;
  root.dataset.density = value.density;
  root.dataset.reducedMotion = value.reducedMotion ? "true" : "false";
  root.dataset.confirmSensitiveDownloads = value.confirmSensitiveDownloads ? "true" : "false";
  root.style.fontSize = value.fontScale === "small" ? "15px" : value.fontScale === "large" ? "18px" : "16px";
}
