import { decodePluginCandidate, decodePluginEntrypoint, decodePluginInstallation, decodePluginInstallations } from "./decode-plugin";
import { stringifyJsonExact } from "./exact-json";
import { requestJson } from "./http-json";
import { createIdempotencyKey } from "./idempotency";
import type { InspectGitPluginInput, PluginCandidateView, PluginClient, PluginEntrypointView, PluginInstallationView, PluginUpdatePolicy } from "./plugin-types";

export class HttpPluginClient implements PluginClient {
  constructor(private readonly baseUrl = "") {}

  async list(signal?: AbortSignal): Promise<PluginInstallationView[]> {
    return decodePluginInstallations(await requestJson(this.baseUrl, "/v1/plugins", { signal }));
  }

  async inspect(input: InspectGitPluginInput, signal?: AbortSignal): Promise<PluginCandidateView> {
    return decodePluginCandidate(await requestJson(this.baseUrl, "/v1/plugins", {
      method: "POST", headers: { "content-type": "application/json" }, body: stringifyJsonExact(input), signal,
    }));
  }

  async activate(candidate: PluginCandidateView, updatePolicy: PluginUpdatePolicy, idempotencyKey = createIdempotencyKey()): Promise<PluginInstallationView> {
    return decodePluginInstallation(await this.command(
      `/v1/plugins/candidates/${encodeURIComponent(candidate.id)}/activate`,
      { expectedActiveVersionId: candidate.currentVersionId, approvedPermissions: candidate.manifest.permissions, updatePolicy },
      idempotencyKey,
    ));
  }

  async configure(pluginId: string, enabled: boolean, updatePolicy: PluginUpdatePolicy, idempotencyKey = createIdempotencyKey()): Promise<PluginInstallationView> {
    return decodePluginInstallation(await this.command(
      `/v1/plugins/${encodeURIComponent(pluginId)}/configure`, { enabled, updatePolicy }, idempotencyKey,
    ));
  }

  async checkUpdate(pluginId: string, signal?: AbortSignal): Promise<PluginCandidateView | null> {
    const value = await requestJson(this.baseUrl, `/v1/plugins/${encodeURIComponent(pluginId)}/check-update`, { method: "POST", signal });
    return value === null ? null : decodePluginCandidate(value);
  }

  async rollback(pluginId: string, targetVersionId: string, expectedActiveVersionId: string, idempotencyKey = createIdempotencyKey()): Promise<PluginInstallationView> {
    return decodePluginInstallation(await this.command(
      `/v1/plugins/${encodeURIComponent(pluginId)}/rollback`, { targetVersionId, expectedActiveVersionId }, idempotencyKey,
    ));
  }

  async getEntrypoint(pluginId: string, signal?: AbortSignal): Promise<PluginEntrypointView> {
    return decodePluginEntrypoint(await requestJson(
      this.baseUrl, `/v1/plugins/${encodeURIComponent(pluginId)}/entrypoint`, { signal },
    ));
  }

  private command(path: string, body: unknown, key: string): Promise<unknown> {
    return requestJson(this.baseUrl, path, {
      method: "POST", headers: { "content-type": "application/json", "idempotency-key": key }, body: stringifyJsonExact(body),
    });
  }
}
