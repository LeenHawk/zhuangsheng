import type { JsonObject } from "./graph-types";

export interface ToolDescriptorView {
  toolId: string;
  version: string;
  name: string;
  description: string | null;
  inputSchema: JsonObject;
}
