export type UiExperienceMode = "user" | "expert";

export interface ArtifactRef {
  artifactId: string;
  contentHash: string;
  byteSize: number;
  mediaType: string;
}

export type LlmContentPart =
  | { type: "text"; text: string }
  | { type: "image"; artifactRef: ArtifactRef }
  | { type: "file"; artifactRef: ArtifactRef };

export interface ConversationRunSpec {
  graphRevisionId: string;
  replyOutputKey: string;
  inputShape: "conversation_message_v1";
}

export interface ConversationRunProfile extends ConversationRunSpec {
  revisionNo: number;
}

export interface ConversationView {
  id: string;
  title: string | null;
  contextId: string;
  activeBranchId: string;
  activeHeadCommitId: string;
  runProfile: ConversationRunProfile | null;
  createdAt: number;
  updatedAt: number;
}

export interface ConversationListView {
  items: ConversationView[];
}

export type CandidateStatus =
  | "running"
  | "ready"
  | "failed"
  | "cancelled"
  | "projection_conflicted"
  | "projection_failed"
  | "projection_abandoned";

export interface ConversationMessageView {
  id: string;
  turnId: string;
  branchId: string;
  commitId: string;
  parentMessageId: string | null;
  role: "user" | "assistant";
  source: "user_input" | "run_output" | "saved_partial";
  content: LlmContentPart[];
  originRunId: string | null;
  createdAt: number;
}

export interface ConversationCandidateView {
  turnId: string;
  runId: string;
  branchId: string;
  baseCommitId: string;
  replyOutputKey: string;
  status: CandidateStatus;
  assistantMessageId: string | null;
  candidateCommitId: string | null;
  projectionError: { code: string; safeMessage: string } | null;
  createdAt: number;
}

export interface ConversationTurnView {
  id: string;
  conversationId: string;
  userMessageId: string;
  userCommitId: string;
  createdAt: number;
  selectedRunId: string | null;
  candidates: ConversationCandidateView[];
}

export interface ConversationTimelineView {
  conversationId: string;
  activeBranchId: string;
  activeHeadCommitId: string;
  messages: ConversationMessageView[];
  turns: ConversationTurnView[];
}

export interface ApiErrorBody {
  code: string;
  message: string;
  retryable: boolean;
  details?: unknown;
  traceId: string;
}
