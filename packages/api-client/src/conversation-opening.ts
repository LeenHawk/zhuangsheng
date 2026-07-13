import type { ConversationCommandOptions, CreateConversationInput, SubmitConversationTurnInput } from "./http-conversation-client";
import type { ConversationRunSpec, ConversationView, SubmitConversationTurnAck } from "./types";

export interface OpeningConversationClient {
  createConversation(input: CreateConversationInput, options?: ConversationCommandOptions): Promise<ConversationView>;
  submitConversationTurn(id: string, input: SubmitConversationTurnInput, options?: ConversationCommandOptions): Promise<SubmitConversationTurnAck>;
}

export interface CreateOpeningConversationInput {
  title?: string;
  run: ConversationRunSpec;
  openingMessage: string;
}

export async function createOpeningConversation(
  client: OpeningConversationClient,
  input: CreateOpeningConversationInput,
  keys: { conversation: string; turn: string },
): Promise<{ conversation: ConversationView; firstTurn: SubmitConversationTurnAck }> {
  if (!input.openingMessage.trim() || !keys.conversation || !keys.turn) {
    throw new Error("opening conversation requires content and stable command keys");
  }
  const conversation = await client.createConversation(
    { title: input.title, defaultRun: input.run },
    { idempotencyKey: keys.conversation },
  );
  const firstTurn = await client.submitConversationTurn(conversation.id, {
    expectedHeadCommitId: conversation.activeHeadCommitId,
    userContent: [{ type: "text", text: input.openingMessage.trim() }],
    run: input.run,
  }, { idempotencyKey: keys.turn });
  return { conversation, firstTurn };
}
