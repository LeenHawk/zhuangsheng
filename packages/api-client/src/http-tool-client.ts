import { decodeToolDescriptors } from "./decode-tools";
import { requestJson } from "./http-json";
import type { ToolDescriptorView } from "./tool-types";

export class HttpToolClient {
  constructor(private readonly baseUrl = "") {}

  async listDescriptors(signal?: AbortSignal): Promise<ToolDescriptorView[]> {
    return decodeToolDescriptors(await requestJson(
      this.baseUrl,
      "/v1/tools/descriptors",
      { signal },
    ));
  }
}
