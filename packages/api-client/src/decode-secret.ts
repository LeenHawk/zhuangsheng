import { DecodeError } from "./decode-error";
import { boolean, nullableString, number, record, string } from "./decode-helpers";
import type { LockSecretStoreResult, SecretMetadataView, SecretStoreSessionView, SecretStoreStatusView } from "./secret-types";

export const decodeSecretStoreStatus = (value: unknown): SecretStoreStatusView => {
  const item = record(value, "secretStoreStatus");
  const initialized = boolean(item.initialized, "secretStoreStatus.initialized");
  const locked = boolean(item.locked, "secretStoreStatus.locked");
  const storeId = nullableString(item.storeId, "secretStoreStatus.storeId");
  const formatVersion = item.formatVersion === null
    ? null
    : number(item.formatVersion, "secretStoreStatus.formatVersion");
  if ((!initialized && (storeId !== null || formatVersion !== null || !locked)) ||
      (initialized && (!storeId || formatVersion !== 1))) {
    throw new DecodeError("secretStoreStatus");
  }
  return { initialized, storeId, formatVersion, locked };
};

export const decodeSecretStoreSession = (value: unknown): SecretStoreSessionView => {
  const item = record(value, "secretStoreSession");
  const formatVersion = number(item.formatVersion, "secretStoreSession.formatVersion");
  if (formatVersion !== 1) throw new DecodeError("secretStoreSession.formatVersion");
  return {
    storeId: string(item.storeId, "secretStoreSession.storeId"),
    formatVersion,
    sessionId: string(item.sessionId, "secretStoreSession.sessionId"),
    expiresAt: number(item.expiresAt, "secretStoreSession.expiresAt"),
  };
};

export const decodeLockSecretStore = (value: unknown): LockSecretStoreResult => {
  const item = record(value, "lockSecretStore");
  if (boolean(item.locked, "lockSecretStore.locked") !== true) {
    throw new DecodeError("lockSecretStore.locked");
  }
  return { locked: true };
};

export const decodeSecretMetadata = (value: unknown, path = "secret"): SecretMetadataView => {
  const item = record(value, path);
  const reference = record(item.secretRef, `${path}.secretRef`);
  const scheme = string(reference.scheme, `${path}.secretRef.scheme`);
  const kind = string(item.kind, `${path}.kind`);
  if (scheme !== "secret" || (kind !== "api_key" && kind !== "token")) throw new DecodeError(path);
  return {
    secretRef: { scheme, id: string(reference.id, `${path}.secretRef.id`) },
    name: nullableString(item.name, `${path}.name`),
    kind,
    createdAt: number(item.createdAt, `${path}.createdAt`),
    updatedAt: number(item.updatedAt, `${path}.updatedAt`),
  };
};

export const decodeSecretList = (value: unknown): SecretMetadataView[] => {
  if (!Array.isArray(value)) throw new DecodeError("secrets");
  return value.map((item, index) => decodeSecretMetadata(item, `secrets[${index}]`));
};
