import { decodeOpenWaits, decodeWaitDelivery } from "./decode-waits";
import { DecodeError } from "./decode-error";
import { requestJson } from "./http-json";
import { streamRunEvents, type RunEventStreamObserver } from "./http-sse";
import type { SubmitToolApprovalInput, WaitDeliveryView, WaitView } from "./wait-types";

export class HttpRuntimeClient {
  constructor(private readonly baseUrl: string) {}

  streamRunEvents(
    runId: string,
    afterDurableSeq: number,
    signal: AbortSignal,
    observer: RunEventStreamObserver,
  ): Promise<void> {
    return streamRunEvents(this.baseUrl, runId, afterDurableSeq, signal, observer);
  }

  async listOpenWaits(runId: string, signal?: AbortSignal): Promise<WaitView[]> {
    const value = await requestJson(
      this.baseUrl,
      `/v1/runs/${encodeURIComponent(runId)}/waits`,
      { signal },
    );
    return decodeOpenWaits(value, runId);
  }

  async submitToolApproval(
    waitId: string,
    input: SubmitToolApprovalInput,
  ): Promise<WaitDeliveryView> {
    const value = await requestJson(
      this.baseUrl,
      `/v1/waits/${encodeURIComponent(waitId)}/responses`,
      {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          deliveryId: input.deliveryId,
          response: {
            type: "blocker_decisions",
            decisions: input.decisions.map((decision) => ({
              kind: "tool_call",
              blockerId: decision.toolCallId,
              callDigest: decision.callDigest,
              decision: decision.decision,
              reason: decision.reason?.trim() || null,
            })),
          },
        }),
      },
    );
    const result = decodeWaitDelivery(value);
    const decided = input.decisions.map((decision) => decision.toolCallId);
    const settled = [...result.preparedToolCallIds, ...result.deniedToolCallIds];
    if (result.waitId !== waitId || result.deliveryId !== input.deliveryId || !sameIds(decided, settled)) {
      throw new DecodeError("waitDelivery");
    }
    return result;
  }
}

const sameIds = (left: string[], right: string[]) =>
  left.length === right.length &&
  new Set(left).size === left.length &&
  new Set(right).size === right.length &&
  left.every((id) => right.includes(id));
