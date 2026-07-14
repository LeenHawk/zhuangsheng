import { describe, expect, it } from "vitest";

import { validateUiNodes } from "./validation";

describe("plugin UI validation", () => {
  it("accepts the closed declarative node vocabulary", () => {
    expect(validateUiNodes([{
      type: "stack",
      gap: "medium",
      children: [{
        type: "paragraph",
        children: [
          { type: "text", text: "旁白", emphasis: "strong" },
          { type: "badge", text: "场景", tone: "accent" },
          { type: "link", text: "资料", href: "https://example.test/lore" },
        ],
      }],
    }])).toHaveLength(1);
  });

  it("rejects script links and unknown DOM-shaped nodes", () => {
    expect(() => validateUiNodes([{ type: "link", text: "bad", href: "javascript:alert(1)" }])).toThrow();
    expect(() => validateUiNodes([{ type: "iframe", src: "https://example.test" }])).toThrow();
  });

  it("rejects output beyond the structural budget", () => {
    let node: unknown = { type: "text", text: "deep" };
    for (let index = 0; index < 10; index += 1) node = { type: "quote", children: [node] };
    expect(() => validateUiNodes([node])).toThrow();
  });
});
