// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import {
  createRunStreamProjection,
  reduceRunStream,
  type RunView,
} from "@zhuangsheng/api-client";
import { RunDetail, RunList } from "@zhuangsheng/domain-ui";

describe("expert run monitor", () => {
  it("renders recent RunViews and opens the selected immutable run", () => {
    const onOpen = vi.fn();
    render(<RunList runs={[run()]} loading={false} error={null} onReload={() => undefined} onOpen={onOpen} />);
    fireEvent.click(screen.getByRole("button", { name: /run_1/ }));
    expect(onOpen).toHaveBeenCalledWith("run_1");
    expect(screen.getByText("运行中")).toBeInTheDocument();
  });

  it("shows durable metadata and uses explicit two-step cancellation", async () => {
    const onControl = vi.fn(async () => undefined);
    const projection = reduceRunStream(createRunStreamProjection("run_1"), {
      kind: "durable",
      event: {
        id: "event_4",
        runId: "run_1",
        durableSeq: 4,
        type: "node.started",
        schemaVersion: 1,
        timestamp: 4,
        nodeInstanceId: "node_1",
        attemptId: "attempt_1",
        importance: "critical",
        payload: { hidden: "not rendered" },
      },
    });
    render(<RunDetail
      run={run()}
      waits={[]}
      projection={projection}
      connection="live"
      loading={false}
      error={null}
      streamError={null}
      controlPending={null}
      controlError={null}
      reload={() => undefined}
      onBack={() => undefined}
      onControl={onControl}
    />);

    expect(screen.getByText("node.started")).toBeInTheDocument();
    expect(screen.queryByText("not rendered")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "暂停" }));
    await waitFor(() => expect(onControl).toHaveBeenCalledWith("interrupt"));
    fireEvent.click(screen.getByRole("button", { name: "取消运行" }));
    expect(onControl).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "确认取消" }));
    await waitFor(() => expect(onControl).toHaveBeenCalledWith("cancel"));
  });
});

const run = (): RunView => ({
  id: "run_1",
  graphRevisionId: "graphrev_1",
  status: "running",
  controlEpoch: 1,
  contextId: "context_1",
  branchId: "branch_1",
  inputCommitId: "commit_1",
  inputRef: "object_1",
  outputCommitId: null,
  lastDurableSeq: 4,
  deadlineAt: Date.now() + 60_000,
  createdAt: 1,
  updatedAt: 2,
});
