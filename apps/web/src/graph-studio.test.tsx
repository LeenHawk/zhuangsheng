// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { GraphStudio } from "@zhuangsheng/domain-ui";

import { parseGraphDraft } from "./graph-draft-validation";

describe("Graph Studio", () => {
  it("rejects identity changes and reports dangling endpoints without repairing the document", () => {
    const mismatch = parseGraphDraft('{"graphId":"other","nodes":[],"edges":[]}', "graph_1");
    expect(mismatch.projection).toBeNull();
    expect(mismatch.diagnostics[0]?.code).toBe("graph_identity_mismatch");

    const dangling = parseGraphDraft(JSON.stringify({
      graphId: "graph_1",
      nodes: [{ id: "input", kind: "input" }],
      edges: [{ from: { nodeId: "missing", output: "value" }, to: { nodeId: "input", input: "value" } }],
    }), "graph_1");
    expect(dangling.projection).not.toBeNull();
    expect(dangling.diagnostics.map((issue) => issue.code)).toContain("edge_source_missing");
  });

  it("keeps Save draft and Apply as separate explicit actions", async () => {
    const onCreate = vi.fn(async () => undefined);
    const onSave = vi.fn();
    render(<GraphStudio
      graphs={[{ id: "graph_1", name: "Story agent", createdAt: 1, updatedAt: 1 }]}
      selectedGraphId="graph_1"
      draft={{ graphId: "graph_1", document: { graphId: "graph_1", nodes: [], edges: [] }, revisionToken: "draftrev_1", updatedAt: 1 }}
      jsonText={'{"graphId":"graph_1","nodes":[],"edges":[]}'}
      projection={{ nodes: [], edges: [] }}
      diagnostics={[]}
      applied={null}
      dirty
      status="ready"
      error={null}
      onSelectGraph={() => undefined}
      onCreateGraph={onCreate}
      onJsonChange={() => undefined}
      onSave={onSave}
      onApply={() => undefined}
      onReload={() => undefined}
    />);

    expect(screen.getByRole("button", { name: "Apply" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "保存草稿" }));
    expect(onSave).toHaveBeenCalledOnce();
    fireEvent.change(screen.getByLabelText("新 Graph 名称"), { target: { value: "Branch agent" } });
    fireEvent.click(screen.getByRole("button", { name: "创建" }));
    await waitFor(() => expect(onCreate).toHaveBeenCalledWith("Branch agent"));
  });
});
