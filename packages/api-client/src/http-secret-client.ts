import { decodeSecretList, decodeSecretMetadata, decodeSecretStoreSession, decodeSecretStoreStatus } from "./decode-secret";
import { requestJson } from "./http-json";
import type {
  SecretPasswordCommandInput,
  PutSecretInput,
  SecretMetadataView,
  SecretStoreSessionView,
  SecretStoreStatusView,
} from "./secret-types";

export class HttpSecretClient {
  constructor(private readonly baseUrl: string) {}

  async status(signal?: AbortSignal): Promise<SecretStoreStatusView> {
    return decodeSecretStoreStatus(
      await requestJson(this.baseUrl, "/v1/secret-store/status", { signal }),
    );
  }

  unlock(input: SecretPasswordCommandInput): Promise<SecretStoreSessionView> {
    return this.passwordCommand("/v1/secret-store/unlock", input);
  }

  initialize(input: SecretPasswordCommandInput): Promise<SecretStoreSessionView> {
    return this.passwordCommand("/v1/secret-store/initialize", input);
  }

  async list(signal?: AbortSignal): Promise<SecretMetadataView[]> {
    return decodeSecretList(await requestJson(this.baseUrl, "/v1/secrets", { signal }));
  }

  async put(input: PutSecretInput): Promise<SecretMetadataView> {
    return decodeSecretMetadata(await requestJson(this.baseUrl, `/v1/secrets/${encodeURIComponent(input.secretId)}`, {
      method: "PUT",
      headers: { "content-type": "application/json", "idempotency-key": input.idempotencyKey },
      body: JSON.stringify({ name: input.name?.trim() || null, kind: input.kind, value: input.value, sessionId: input.sessionId }),
    }));
  }

  private async passwordCommand(
    path: string,
    input: SecretPasswordCommandInput,
  ): Promise<SecretStoreSessionView> {
    const value = await requestJson(this.baseUrl, path, {
      method: "POST",
      headers: { "content-type": "application/json", "idempotency-key": input.idempotencyKey },
      body: JSON.stringify({ masterPassword: input.masterPassword }),
    });
    return decodeSecretStoreSession(value);
  }
}
