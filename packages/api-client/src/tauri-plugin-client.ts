import { decodePluginCandidate, decodePluginEntrypoint, decodePluginInstallation, decodePluginInstallations } from "./decode-plugin";
import { createIdempotencyKey } from "./idempotency";
import type { InspectGitPluginInput, PluginCandidateView, PluginClient, PluginEntrypointView, PluginInstallationView, PluginUpdatePolicy } from "./plugin-types";
import type { TauriBridge } from "./transport";

export class TauriPluginClient implements PluginClient {
  constructor(private readonly bridge: TauriBridge) {}

  async list(): Promise<PluginInstallationView[]> {
    return decodePluginInstallations(await this.bridge.invoke("list_plugins", {}));
  }

  async inspect(input: InspectGitPluginInput): Promise<PluginCandidateView> {
    return decodePluginCandidate(await this.bridge.invoke("inspect_git_plugin_source", { command: input }));
  }

  async activate(candidate: PluginCandidateView, updatePolicy: PluginUpdatePolicy, idempotencyKey = createIdempotencyKey()): Promise<PluginInstallationView> {
    return decodePluginInstallation(await this.bridge.invoke("activate_plugin_candidate", { command: {
      candidateId: candidate.id, expectedActiveVersionId: candidate.currentVersionId,
      approvedPermissions: candidate.manifest.permissions, updatePolicy, idempotencyKey,
    } }));
  }

  async configure(pluginId: string, enabled: boolean, updatePolicy: PluginUpdatePolicy, idempotencyKey = createIdempotencyKey()): Promise<PluginInstallationView> {
    return decodePluginInstallation(await this.bridge.invoke("configure_plugin", { command: {
      pluginId, enabled, updatePolicy, idempotencyKey,
    } }));
  }

  async checkUpdate(pluginId: string): Promise<PluginCandidateView | null> {
    const value = await this.bridge.invoke("check_plugin_update", { pluginId });
    return value === null ? null : decodePluginCandidate(value);
  }

  async rollback(pluginId: string, targetVersionId: string, expectedActiveVersionId: string, idempotencyKey = createIdempotencyKey()): Promise<PluginInstallationView> {
    return decodePluginInstallation(await this.bridge.invoke("rollback_plugin", { command: {
      pluginId, targetVersionId, expectedActiveVersionId, idempotencyKey,
    } }));
  }

  async getEntrypoint(pluginId: string): Promise<PluginEntrypointView> {
    return decodePluginEntrypoint(await this.bridge.invoke("get_plugin_entrypoint", { pluginId }));
  }
}
