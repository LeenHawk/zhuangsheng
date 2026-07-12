import { DecodeError } from "./decode-error";
import { boolean, record, string } from "./decode-helpers";
import type { EffectResolutionKind, EffectResolutionView } from "./effect-types";

const kinds = new Set<EffectResolutionKind>([
  "confirm_succeeded",
  "confirm_failed_retry_safe",
  "abort_run",
]);

export const decodeEffectResolutionKind = (
  value: unknown,
  path: string,
): EffectResolutionKind => {
  const kind = string(value, path) as EffectResolutionKind;
  if (!kinds.has(kind)) throw new DecodeError(path);
  return kind;
};

export const decodeEffectResolution = (value: unknown): EffectResolutionView => {
  const path = "effectResolution";
  const item = record(value, path);
  return {
    resolutionId: string(item.resolutionId, `${path}.resolutionId`),
    effectId: string(item.effectId, `${path}.effectId`),
    effectAttemptId: string(item.effectAttemptId, `${path}.effectAttemptId`),
    waitId: string(item.waitId, `${path}.waitId`),
    kind: decodeEffectResolutionKind(item.kind, `${path}.kind`),
    replayed: boolean(item.replayed, `${path}.replayed`),
  };
};
