export interface SecretStoreStatusView {
  initialized: boolean;
  storeId: string | null;
  formatVersion: number | null;
  locked: boolean;
}

export interface SecretStoreSessionView {
  storeId: string;
  formatVersion: number;
  sessionId: string;
  expiresAt: number;
}

export interface SecretPasswordCommandInput {
  masterPassword: string;
  idempotencyKey: string;
}
