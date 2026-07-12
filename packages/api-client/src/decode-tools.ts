import { jsonObject, nullableString, record, string } from "./decode-helpers";
import { DecodeError } from "./decode-error";
import type { ToolDescriptorView } from "./tool-types";

export const decodeToolDescriptors = (value: unknown): ToolDescriptorView[] => {
  if (!Array.isArray(value)) throw new DecodeError("toolDescriptors");
  return value.map((raw, index) => {
    const path = `toolDescriptors[${index}]`;
    const item = record(raw, path);
    return {
      toolId: string(item.toolId, `${path}.toolId`),
      version: string(item.version, `${path}.version`),
      name: string(item.name, `${path}.name`),
      description: nullableString(item.description, `${path}.description`),
      inputSchema: jsonObject(item.inputSchema, `${path}.inputSchema`),
    };
  });
};
