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
