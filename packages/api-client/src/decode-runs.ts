import { DecodeError } from "./decode-error";
import { nullableString, number, record, string } from "./decode-helpers";
import type {
  RunListView,
  RunOutputValueView,
  RunOutputsView,
  RunStatus,
  RunView,
} from "./run-types";

const statuses = new Set<RunStatus>([
  "created", "running", "waiting", "interrupting", "interrupted",
  "completed", "failed", "cancelled",
]);

export const decodeRun = (value: unknown, path = "run"): RunView => {
  const item = record(value, path);
  const status = string(item.status, `${path}.status`) as RunStatus;
  if (!statuses.has(status)) throw new DecodeError(`${path}.status`);
  return {
    id: string(item.id, `${path}.id`),
    graphRevisionId: string(item.graphRevisionId, `${path}.graphRevisionId`),
    status,
    controlEpoch: nonNegative(item.controlEpoch, `${path}.controlEpoch`),
    contextId: string(item.contextId, `${path}.contextId`),
    branchId: string(item.branchId, `${path}.branchId`),
    inputCommitId: string(item.inputCommitId, `${path}.inputCommitId`),
    inputRef: string(item.inputRef, `${path}.inputRef`),
    outputCommitId: nullableString(item.outputCommitId, `${path}.outputCommitId`),
    lastDurableSeq: nonNegative(item.lastDurableSeq, `${path}.lastDurableSeq`),
    deadlineAt: number(item.deadlineAt, `${path}.deadlineAt`),
    createdAt: number(item.createdAt, `${path}.createdAt`),
    updatedAt: number(item.updatedAt, `${path}.updatedAt`),
  };
};

export const decodeRunList = (value: unknown): RunListView => {
  const item = record(value, "runList");
  if (!Array.isArray(item.items)) throw new DecodeError("runList.items");
  return { items: item.items.map((run, index) => decodeRun(run, `runList.items[${index}]`)) };
};

export const decodeRunOutputs = (value: unknown): RunOutputsView => {
  const outputs = record(value, "runOutputs");
  return Object.fromEntries(Object.entries(outputs).map(([key, raw]) => {
    const entry = record(raw, `runOutputs.${key}`);
    const collection = string(entry.collection, `runOutputs.${key}.collection`);
    if (collection !== "single" && collection !== "append") {
      throw new DecodeError(`runOutputs.${key}.collection`);
    }
    if (!Array.isArray(entry.values)) throw new DecodeError(`runOutputs.${key}.values`);
    return [key, {
      collection,
      values: entry.values.map((item, index) =>
        decodeOutputValue(item, `runOutputs.${key}.values[${index}]`)),
    }];
  }));
};

const decodeOutputValue = (value: unknown, path: string): RunOutputValueView => {
  const item = record(value, path);
  const common = {
    valueRef: string(item.valueRef, `${path}.valueRef`),
    contentHash: string(item.contentHash, `${path}.contentHash`),
    sizeBytes: nonNegative(item.sizeBytes, `${path}.sizeBytes`),
  };
  const kind = string(item.kind, `${path}.kind`);
  if (kind === "inline_json") {
    assertJson(item.value, `${path}.value`);
    return { kind, ...common, value: item.value };
  }
  if (kind === "json_value_ref") {
    return {
      kind,
      ...common,
      downloadPath: string(item.downloadPath, `${path}.downloadPath`),
    };
  }
  throw new DecodeError(`${path}.kind`);
};

export const assertJson = (value: unknown, path: string): void => {
  if (value === null || typeof value === "string" || typeof value === "boolean") return;
  if (typeof value === "number") {
    if (!Number.isFinite(value)) throw new DecodeError(path);
    return;
  }
  if (Array.isArray(value)) {
    value.forEach((item, index) => assertJson(item, `${path}[${index}]`));
    return;
  }
  const item = record(value, path);
  Object.entries(item).forEach(([key, nested]) => assertJson(nested, `${path}.${key}`));
};

const nonNegative = (value: unknown, path: string) => {
  const parsed = number(value, path);
  if (parsed < 0) throw new DecodeError(path);
  return parsed;
};
