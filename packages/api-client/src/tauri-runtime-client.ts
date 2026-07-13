import { decodeEffectResolution } from "./decode-effect";
import { DecodeError } from "./decode-error";
import { parseJsonExact } from "./exact-json";
import { assertJson, decodeRun, decodeRunList, decodeRunOutputs } from "./decode-runs";
import type { RunEventStreamObserver } from "./http-sse";
import { decodeOpenWaits, decodeWaitDelivery } from "./decode-waits";
import type { ResolveEffectUnknownInput, EffectResolutionView } from "./effect-types";
import type { RunControlInput, RunListView, RunOutputsView, RunView } from "./run-types";
import { TauriTransport, type TauriBridge } from "./transport";
import type {
  SubmitHumanResponseInput,
  SubmitMemoryProposalDecisionInput,
  SubmitToolApprovalInput,
  WaitDeliveryView,
  WaitView,
} from "./wait-types";

export class TauriRuntimeClient {
  private readonly transport: TauriTransport;

  constructor(private readonly bridge: TauriBridge) {
    this.transport = new TauriTransport(bridge);
  }

  async listRecentRuns(limit = 50): Promise<RunListView> {
    return decodeRunList(await this.bridge.invoke("list_recent_runs", { limit }));
  }

  async getRunOutputs(runId: string): Promise<RunOutputsView> {
    return decodeRunOutputs(await this.bridge.invoke("get_run_outputs", { runId }));
  }

  async loadJsonValue(valueRef: string): Promise<unknown> {
    const bytes = await this.bridge.invoke<unknown>("load_json_value_bytes", { valueRef });
    if (!Array.isArray(bytes) || bytes.some((byte) => !Number.isInteger(byte) || byte < 0 || byte > 255)) {
      throw new DecodeError("jsonValue.bytes");
    }
    let value: unknown;
    try { value = parseJsonExact(new TextDecoder().decode(Uint8Array.from(bytes as number[]))); }
    catch { throw new DecodeError("jsonValue"); }
    assertJson(value, "jsonValue");
    return value;
  }

  async getRun(runId: string): Promise<RunView> {
    const run = decodeRun(await this.bridge.invoke("get_run", { runId }));
    if (run.id !== runId) throw new DecodeError("run.id");
    return run;
  }

  async listOpenWaits(runId: string): Promise<WaitView[]> {
    return decodeOpenWaits(await this.bridge.invoke("list_open_waits", { runId }), runId);
  }

  streamRunEvents(
    runId: string,
    afterDurableSeq: number,
    signal: AbortSignal,
    observer: RunEventStreamObserver,
  ): Promise<void> {
    return this.transport.subscribeRun(runId, afterDurableSeq, signal, observer);
  }

  interrupt(runId: string, input: RunControlInput): Promise<RunView> {
    return this.control("interrupt_run", runId, input);
  }

  resume(runId: string, input: RunControlInput): Promise<RunView> {
    return this.control("resume_run", runId, input);
  }

  cancel(runId: string, input: RunControlInput): Promise<RunView> {
    return this.control("cancel_run", runId, input);
  }

  async submitToolApproval(
    waitId: string,
    input: SubmitToolApprovalInput,
  ): Promise<WaitDeliveryView> {
    return this.satisfy(waitId, input.deliveryId, {
      kind: "tool_approval",
      decisions: input.decisions.map((item) => ({
        toolCallId: item.toolCallId,
        callDigest: item.callDigest,
        decision: item.decision,
        reason: item.reason?.trim() || null,
      })),
    });
  }

  async submitMemoryProposalDecisions(
    waitId: string,
    input: SubmitMemoryProposalDecisionInput,
  ): Promise<WaitDeliveryView> {
    return this.satisfy(waitId, input.deliveryId, {
      kind: "memory_proposal",
      decisions: input.decisions,
    });
  }

  async submitHumanResponse(
    waitId: string,
    input: SubmitHumanResponseInput,
  ): Promise<WaitDeliveryView> {
    return this.satisfy(waitId, input.deliveryId, { kind: "value", value: input.value });
  }

  async resolveEffectUnknown(
    effectId: string,
    input: ResolveEffectUnknownInput,
  ): Promise<EffectResolutionView> {
    const result = decodeEffectResolution(await this.bridge.invoke("resolve_effect_unknown", { input: {
      effectId,
      ...input,
    } }));
    if (result.effectId !== effectId || result.effectAttemptId !== input.expectedEffectAttemptId) {
      throw new DecodeError("effectResolution");
    }
    return result;
  }

  private async satisfy(
    waitId: string,
    deliveryId: string,
    response: unknown,
  ): Promise<WaitDeliveryView> {
    const result = decodeWaitDelivery(await this.bridge.invoke("satisfy_wait", { input: {
      waitId,
      deliveryId,
      response,
    } }));
    if (result.waitId !== waitId || result.deliveryId !== deliveryId) {
      throw new DecodeError("waitDelivery");
    }
    return result;
  }

  private async control(
    operation: string,
    runId: string,
    input: RunControlInput,
  ): Promise<RunView> {
    const run = decodeRun(await this.bridge.invoke(operation, { command: {
      runId, expectedEpoch: input.expectedEpoch,
      idempotencyKey: input.idempotencyKey, reason: input.reason ?? null,
    } }));
    if (run.id !== runId || run.controlEpoch !== input.expectedEpoch + 1) {
      throw new DecodeError("runControl");
    }
    return run;
  }
}
