export type RunStatus =
  | "created"
  | "running"
  | "waiting"
  | "interrupting"
  | "interrupted"
  | "completed"
  | "failed"
  | "cancelled";

export interface RunView {
  id: string;
  graphRevisionId: string;
  status: RunStatus;
  controlEpoch: number;
  contextId: string;
  branchId: string;
  inputCommitId: string;
  inputRef: string;
  outputCommitId: string | null;
  lastDurableSeq: number;
  deadlineAt: number;
  createdAt: number;
  updatedAt: number;
}

export interface RunListView {
  items: RunView[];
}

export interface RunControlInput {
  expectedEpoch: number;
  idempotencyKey: string;
  reason?: string;
}

export type RunContextInput =
  | { mode: "temporary" }
  | {
      mode: "existing";
      contextId: string;
      branchId: string;
      expectedHeadCommitId: string;
    };

export interface StartRunInput {
  input: unknown;
  context: RunContextInput;
  deadlineAt?: number | null;
  idempotencyKey: string;
}

export type RunOutputValueView =
  | {
      kind: "inline_json";
      valueRef: string;
      contentHash: string;
      sizeBytes: number;
      value: unknown;
    }
  | {
      kind: "json_value_ref";
      valueRef: string;
      contentHash: string;
      sizeBytes: number;
      downloadPath: string;
    };

export interface RunOutputEntryView {
  collection: "single" | "append";
  values: RunOutputValueView[];
}

export type RunOutputsView = Record<string, RunOutputEntryView>;

export interface InvokeRunResult {
  run: RunView;
  outputs: RunOutputsView | null;
}
