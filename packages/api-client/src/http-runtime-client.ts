import { decodeOpenWaits, decodeWaitDelivery } from "./decode-waits";
import { assertJson, decodeRun, decodeRunList, decodeRunOutputs } from "./decode-runs";
import { DecodeError } from "./decode-error";
import { requestJson } from "./http-json";
import { streamRunEvents, type RunEventStreamObserver } from "./http-sse";
import type { SubmitToolApprovalInput, WaitDeliveryView, WaitView } from "./wait-types";
import type {
  RunControlInput,
  RunListView,
  RunOutputsView,
  RunView,
  StartRunInput,
} from "./run-types";

export class HttpRuntimeClient {
  constructor(private readonly baseUrl: string) {}

  async startRun(graphRevisionId: string, input: StartRunInput): Promise<RunView> {
    const run = decodeRun(await requestJson(
      this.baseUrl,
      `/v1/graphs/${encodeURIComponent(graphRevisionId)}/runs`,
      {
        method: "POST",
        headers: { "content-type": "application/json", "idempotency-key": input.idempotencyKey },
        body: JSON.stringify({
          input: input.input,
          context: input.context,
          deadlineAt: input.deadlineAt ?? null,
        }),
      },
    ));
    if (run.graphRevisionId !== graphRevisionId) throw new DecodeError("run.graphRevisionId");
    return run;
  }

  async listRecentRuns(limit = 50, signal?: AbortSignal): Promise<RunListView> {
    return decodeRunList(await requestJson(
      this.baseUrl,
      `/v1/runs?limit=${Math.max(1, Math.min(100, Math.trunc(limit)))}`,
      { signal },
    ));
  }

  async getRun(runId: string, signal?: AbortSignal): Promise<RunView> {
    return decodeRun(await requestJson(
      this.baseUrl,
      `/v1/runs/${encodeURIComponent(runId)}`,
      { signal },
    ));
  }

  async getRunOutputs(runId: string, signal?: AbortSignal): Promise<RunOutputsView> {
    return decodeRunOutputs(await requestJson(
      this.baseUrl,
      `/v1/runs/${encodeURIComponent(runId)}/outputs`,
      { signal },
    ));
  }

  async loadJsonValue(valueRef: string, signal?: AbortSignal): Promise<unknown> {
    const value = await requestJson(
      this.baseUrl,
      `/v1/values/${encodeURIComponent(valueRef)}`,
      { signal },
    );
    assertJson(value, "jsonValue");
    return value;
  }

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

  interrupt(runId: string, input: RunControlInput): Promise<RunView> {
    return this.control(runId, "interrupt", input);
  }

  resume(runId: string, input: RunControlInput): Promise<RunView> {
    return this.control(runId, "resume", input);
  }

  cancel(runId: string, input: RunControlInput): Promise<RunView> {
    return this.control(runId, "cancel", input);
  }

  private async control(
    runId: string,
    action: "interrupt" | "resume" | "cancel",
    input: RunControlInput,
  ): Promise<RunView> {
    const value = await requestJson(
      this.baseUrl,
      `/v1/runs/${encodeURIComponent(runId)}/${action}`,
      {
        method: "POST",
        headers: { "content-type": "application/json", "idempotency-key": input.idempotencyKey },
        body: JSON.stringify({
          expectedEpoch: input.expectedEpoch,
          reason: input.reason?.trim() || null,
        }),
      },
    );
    const run = decodeRun(value);
    if (run.id !== runId) throw new DecodeError("run.id");
    if (run.controlEpoch !== input.expectedEpoch + 1) throw new DecodeError("run.controlEpoch");
    return run;
  }
}

const sameIds = (left: string[], right: string[]) =>
  left.length === right.length &&
  new Set(left).size === left.length &&
  new Set(right).size === right.length &&
  left.every((id) => right.includes(id));
