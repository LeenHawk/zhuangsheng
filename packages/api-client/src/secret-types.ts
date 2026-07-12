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

export interface SecretRef {
  scheme: "secret";
  id: string;
}

export interface SecretMetadataView {
  secretRef: SecretRef;
  name: string | null;
  kind: "api_key" | "token";
  createdAt: number;
  updatedAt: number;
}

export interface PutSecretInput {
  secretId: string;
  name?: string;
  kind: "api_key" | "token";
  value: string;
  sessionId: string;
  idempotencyKey: string;
}
