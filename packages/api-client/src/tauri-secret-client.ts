import {
  decodeSecretList,
  decodeSecretMetadata,
  decodeSecretStoreSession,
  decodeSecretStoreStatus,
} from "./decode-secret";
import type {
  PutSecretInput,
  SecretMetadataView,
  SecretPasswordCommandInput,
  SecretStoreSessionView,
  SecretStoreStatusView,
} from "./secret-types";
import type { TauriBridge } from "./transport";

export class TauriSecretClient {
  constructor(private readonly bridge: TauriBridge) {}

  async status(): Promise<SecretStoreStatusView> {
    return decodeSecretStoreStatus(await this.bridge.invoke("get_secret_store_status", {}));
  }

  initialize(input: SecretPasswordCommandInput): Promise<SecretStoreSessionView> {
    return this.passwordCommand("initialize_secret_store", input);
  }

  unlock(input: SecretPasswordCommandInput): Promise<SecretStoreSessionView> {
    return this.passwordCommand("unlock_secret_store", input);
  }

  async list(): Promise<SecretMetadataView[]> {
    return decodeSecretList(await this.bridge.invoke("list_secrets", {}));
  }

  async put(input: PutSecretInput): Promise<SecretMetadataView> {
    return decodeSecretMetadata(await this.bridge.invoke("put_secret", { input }));
  }

  private async passwordCommand(
    operation: string,
    input: SecretPasswordCommandInput,
  ): Promise<SecretStoreSessionView> {
    return decodeSecretStoreSession(await this.bridge.invoke(operation, { input: {
      masterPassword: input.masterPassword,
      idempotencyKey: input.idempotencyKey,
    } }));
  }
}
