import { DecodeError } from "./decode-error";
import { nullableString, number, record, string } from "./decode-helpers";
import type { RunListView, RunStatus, RunView } from "./run-types";

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

const nonNegative = (value: unknown, path: string) => {
  const parsed = number(value, path);
  if (parsed < 0) throw new DecodeError(path);
  return parsed;
};
