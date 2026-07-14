import type { JsonValue } from "./graph-types";

export type PluginUpdatePolicy = "manual" | "notify" | "automatic";
export type PluginPermission =
  | "ui_message_read_display"
  | "ui_message_decorate"
  | "ui_artifact_render"
  | "ui_panel"
  | "ui_theme";
export type PluginRendererSlot = "conversation_message_body";
export type PluginMessageRole = "user" | "assistant";

export interface PluginRendererDeclaration {
  id: string;
  slot: PluginRendererSlot;
  priority: number;
  roles: PluginMessageRole[];
}

export interface PluginManifest {
  apiVersion: number;
  id: string;
  name: string;
  version: string;
  description: string | null;
  minimumHostVersion: string | null;
  entrypoints: { uiWorker: string };
  permissions: PluginPermission[];
  renderers: PluginRendererDeclaration[];
  dependencies: string[];
  settingsSchema: JsonValue | null;
}

export interface PluginVersionView {
  id: string;
  pluginId: string;
  version: string;
  resolvedCommit: string;
  treeHash: string;
  manifestHash: string;
  manifest: PluginManifest;
  installedAt: number;
}

export interface PluginInstallationView {
  pluginId: string;
  sourceUrl: string;
  sourceRef: string | null;
  credentialSecretId: string | null;
  credentialUsername: string | null;
  updatePolicy: PluginUpdatePolicy;
  enabled: boolean;
  activeVersion: PluginVersionView;
  previousVersions: PluginVersionView[];
  createdAt: number;
  updatedAt: number;
}

export interface PluginCandidateView {
  id: string;
  plannedVersionId: string;
  sourceUrl: string;
  sourceRef: string | null;
  credentialSecretId: string | null;
  credentialUsername: string | null;
  resolvedCommit: string;
  treeHash: string;
  manifestHash: string;
  manifest: PluginManifest;
  currentVersionId: string | null;
  addedPermissions: PluginPermission[];
  createdAt: number;
}

export interface PluginEntrypointView {
  pluginId: string;
  versionId: string;
  contentHash: string;
  code: string;
}

export interface InspectGitPluginInput {
  sourceUrl: string;
  sourceRef?: string | null;
  credentialSecretId?: string | null;
  credentialUsername?: string | null;
}

export interface PluginClient {
  list(signal?: AbortSignal): Promise<PluginInstallationView[]>;
  inspect(input: InspectGitPluginInput, signal?: AbortSignal): Promise<PluginCandidateView>;
  activate(candidate: PluginCandidateView, updatePolicy: PluginUpdatePolicy, idempotencyKey?: string): Promise<PluginInstallationView>;
  configure(pluginId: string, enabled: boolean, updatePolicy: PluginUpdatePolicy, idempotencyKey?: string): Promise<PluginInstallationView>;
  checkUpdate(pluginId: string, signal?: AbortSignal): Promise<PluginCandidateView | null>;
  rollback(pluginId: string, targetVersionId: string, expectedActiveVersionId: string, idempotencyKey?: string): Promise<PluginInstallationView>;
  getEntrypoint(pluginId: string, signal?: AbortSignal): Promise<PluginEntrypointView>;
}
