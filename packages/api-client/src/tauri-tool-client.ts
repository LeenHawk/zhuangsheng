import { decodeToolDescriptors } from "./decode-tools";
import type { ToolDescriptorView } from "./tool-types";
import type { TauriBridge } from "./transport";

export class TauriToolClient {
  constructor(private readonly bridge: TauriBridge) {}

  async listDescriptors(): Promise<ToolDescriptorView[]> {
    return decodeToolDescriptors(await this.bridge.invoke("list_tool_descriptors", {}));
  }
}
