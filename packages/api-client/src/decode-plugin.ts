import { boolean, jsonValue, nullableString, number, record, string, stringArray } from "./decode-helpers";
import { DecodeError } from "./decode-error";
import type {
  PluginCandidateView,
  PluginEntrypointView,
  PluginInstallationView,
  PluginManifest,
  PluginMessageRole,
  PluginPermission,
  PluginRendererSlot,
  PluginUpdatePolicy,
  PluginVersionView,
} from "./plugin-types";

const permissions: PluginPermission[] = [
  "ui_message_read_display", "ui_message_decorate", "ui_artifact_render", "ui_panel", "ui_theme",
];
const policies: PluginUpdatePolicy[] = ["manual", "notify", "automatic"];

const oneOf = <T extends string>(value: unknown, values: T[], path: string): T => {
  const decoded = string(value, path);
  if (!values.includes(decoded as T)) throw new DecodeError(path);
  return decoded as T;
};

const decodeManifest = (value: unknown, path: string): PluginManifest => {
  const item = record(value, path);
  const entrypoints = record(item.entrypoints, `${path}.entrypoints`);
  if (!Array.isArray(item.permissions) || !Array.isArray(item.renderers)) throw new DecodeError(path);
  return {
    apiVersion: number(item.apiVersion, `${path}.apiVersion`),
    id: string(item.id, `${path}.id`),
    name: string(item.name, `${path}.name`),
    version: string(item.version, `${path}.version`),
    description: nullableString(item.description, `${path}.description`),
    minimumHostVersion: nullableString(item.minimumHostVersion, `${path}.minimumHostVersion`),
    entrypoints: { uiWorker: string(entrypoints.uiWorker, `${path}.entrypoints.uiWorker`) },
    permissions: item.permissions.map((raw, index) => oneOf(raw, permissions, `${path}.permissions[${index}]`)),
    renderers: item.renderers.map((raw, index) => {
      const rendererPath = `${path}.renderers[${index}]`;
      const renderer = record(raw, rendererPath);
      if (!Array.isArray(renderer.roles)) throw new DecodeError(`${rendererPath}.roles`);
      return {
        id: string(renderer.id, `${rendererPath}.id`),
        slot: oneOf<PluginRendererSlot>(renderer.slot, ["conversation_message_body"], `${rendererPath}.slot`),
        priority: number(renderer.priority, `${rendererPath}.priority`),
        roles: renderer.roles.map((role, roleIndex) => oneOf<PluginMessageRole>(role, ["user", "assistant"], `${rendererPath}.roles[${roleIndex}]`)),
      };
    }),
    dependencies: stringArray(item.dependencies, `${path}.dependencies`),
    settingsSchema: item.settingsSchema === null ? null : jsonValue(item.settingsSchema, `${path}.settingsSchema`),
  };
};

const decodeVersion = (value: unknown, path: string): PluginVersionView => {
  const item = record(value, path);
  return {
    id: string(item.id, `${path}.id`), pluginId: string(item.pluginId, `${path}.pluginId`),
    version: string(item.version, `${path}.version`),
    resolvedCommit: string(item.resolvedCommit, `${path}.resolvedCommit`),
    treeHash: string(item.treeHash, `${path}.treeHash`),
    manifestHash: string(item.manifestHash, `${path}.manifestHash`),
    manifest: decodeManifest(item.manifest, `${path}.manifest`),
    installedAt: number(item.installedAt, `${path}.installedAt`),
  };
};

export const decodePluginCandidate = (value: unknown): PluginCandidateView => {
  const path = "pluginCandidate";
  const item = record(value, path);
  if (!Array.isArray(item.addedPermissions)) throw new DecodeError(`${path}.addedPermissions`);
  return {
    id: string(item.id, `${path}.id`), plannedVersionId: string(item.plannedVersionId, `${path}.plannedVersionId`),
    sourceUrl: string(item.sourceUrl, `${path}.sourceUrl`), sourceRef: nullableString(item.sourceRef, `${path}.sourceRef`),
    credentialSecretId: nullableString(item.credentialSecretId, `${path}.credentialSecretId`),
    credentialUsername: nullableString(item.credentialUsername, `${path}.credentialUsername`),
    resolvedCommit: string(item.resolvedCommit, `${path}.resolvedCommit`), treeHash: string(item.treeHash, `${path}.treeHash`),
    manifestHash: string(item.manifestHash, `${path}.manifestHash`), manifest: decodeManifest(item.manifest, `${path}.manifest`),
    currentVersionId: nullableString(item.currentVersionId, `${path}.currentVersionId`),
    addedPermissions: item.addedPermissions.map((raw, index) => oneOf(raw, permissions, `${path}.addedPermissions[${index}]`)),
    createdAt: number(item.createdAt, `${path}.createdAt`),
  };
};

export const decodePluginInstallation = (value: unknown): PluginInstallationView => {
  const path = "pluginInstallation";
  const item = record(value, path);
  if (!Array.isArray(item.previousVersions)) throw new DecodeError(`${path}.previousVersions`);
  return {
    pluginId: string(item.pluginId, `${path}.pluginId`), sourceUrl: string(item.sourceUrl, `${path}.sourceUrl`),
    sourceRef: nullableString(item.sourceRef, `${path}.sourceRef`),
    credentialSecretId: nullableString(item.credentialSecretId, `${path}.credentialSecretId`),
    credentialUsername: nullableString(item.credentialUsername, `${path}.credentialUsername`),
    updatePolicy: oneOf(item.updatePolicy, policies, `${path}.updatePolicy`), enabled: boolean(item.enabled, `${path}.enabled`),
    activeVersion: decodeVersion(item.activeVersion, `${path}.activeVersion`),
    previousVersions: item.previousVersions.map((raw, index) => decodeVersion(raw, `${path}.previousVersions[${index}]`)),
    createdAt: number(item.createdAt, `${path}.createdAt`), updatedAt: number(item.updatedAt, `${path}.updatedAt`),
  };
};

export const decodePluginInstallations = (value: unknown): PluginInstallationView[] => {
  if (!Array.isArray(value)) throw new DecodeError("pluginInstallations");
  return value.map(decodePluginInstallation);
};

export const decodePluginEntrypoint = (value: unknown): PluginEntrypointView => {
  const item = record(value, "pluginEntrypoint");
  return {
    pluginId: string(item.pluginId, "pluginEntrypoint.pluginId"),
    versionId: string(item.versionId, "pluginEntrypoint.versionId"),
    contentHash: string(item.contentHash, "pluginEntrypoint.contentHash"),
    code: string(item.code, "pluginEntrypoint.code"),
  };
};
