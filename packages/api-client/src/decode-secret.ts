import { DecodeError } from "./decode-error";
import { boolean, nullableString, number, record, string } from "./decode-helpers";
import type { SecretStoreSessionView, SecretStoreStatusView } from "./secret-types";

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
