import { decodeSecretStoreSession, decodeSecretStoreStatus } from "./decode-secret";
import { requestJson } from "./http-json";
import type {
  SecretPasswordCommandInput,
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
