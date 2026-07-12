import { describe, expect, it } from "vitest";

import { graphElements } from "./layout";

describe("graphElements", () => {
  it("maps ports into explicit React Flow handles without changing semantics", () => {
    const result = graphElements({
      nodes: [
        { id: "a", name: null, kind: "input", isEntry: true, inputs: [], outputs: [{ name: "value" }] },
        { id: "b", name: "Reply", kind: "llm", isEntry: false, inputs: [{ name: "prompt" }], outputs: [] },
      ],
      edges: [{ id: "edge_1", source: "a", sourcePort: "value", target: "b", targetPort: "prompt" }],
    }, { b: { status: "running", activationCount: 1, attemptCount: 1, lastDurableSeq: 4 } });
    expect(result.nodes[0]?.data.label).toBe("a");
    expect(result.edges[0]).toMatchObject({
      source: "a",
      sourceHandle: "out:value",
      target: "b",
      targetHandle: "in:prompt",
    });
    expect(result.nodes[1]?.data.overlay?.status).toBe("running");
  });
});
