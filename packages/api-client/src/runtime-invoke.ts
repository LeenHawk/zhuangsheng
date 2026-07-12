import { DecodeError } from "./decode-error";
import {
  followRunEvents,
  type FollowRunEventsOptions,
  type RunEventStreamClient,
} from "./run-stream-follow";
import type {
  InvokeRunResult,
  RunOutputsView,
  RunView,
  StartRunInput,
} from "./run-types";

export interface RuntimeInvocationClient extends RunEventStreamClient {
  startRun(graphRevisionId: string, input: StartRunInput): Promise<RunView>;
  getRun(runId: string, signal?: AbortSignal): Promise<RunView>;
  getRunOutputs(runId: string, signal?: AbortSignal): Promise<RunOutputsView>;
}

export type WaitForRunOptions = Partial<Pick<
  FollowRunEventsOptions,
  "backoffBaseMs" | "backoffMaxMs" | "random" | "onProjection" | "onConnection"
>> & { initialDurableSeq?: number };

export async function waitForRunTerminal(
  client: RuntimeInvocationClient,
  runId: string,
  signal: AbortSignal,
  options: WaitForRunOptions = {},
): Promise<RunView> {
  await followRunEvents(client, runId, signal, {
    initialDurableSeq: options.initialDurableSeq,
    backoffBaseMs: options.backoffBaseMs,
    backoffMaxMs: options.backoffMaxMs,
    random: options.random,
    onProjection: options.onProjection ?? (() => undefined),
    onConnection: options.onConnection ?? (() => undefined),
  });
  const run = await client.getRun(runId, signal);
  if (!isTerminal(run.status)) throw new DecodeError("run.status");
  return run;
}

export async function invokeRun(
  client: RuntimeInvocationClient,
  graphRevisionId: string,
  input: StartRunInput,
  signal: AbortSignal,
  options: WaitForRunOptions = {},
): Promise<InvokeRunResult> {
  const started = await client.startRun(graphRevisionId, input);
  const run = await waitForRunTerminal(client, started.id, signal, {
    ...options,
    initialDurableSeq: started.lastDurableSeq,
  });
  return {
    run,
    outputs: run.status === "completed"
      ? await client.getRunOutputs(run.id, signal)
      : null,
  };
}

const isTerminal = (status: RunView["status"]) =>
  status === "completed" || status === "failed" || status === "cancelled";
