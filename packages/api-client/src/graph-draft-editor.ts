import { parseJsonExact } from "./exact-json";
import { projectGraphStructure } from "./decode-graphs";
import { isJsonNumber } from "./exact-json";
import type {
  GraphStructureProjection,
  JsonObject,
  ValidationIssue,
} from "./graph-types";

export interface ParsedGraphDraft {
  document: JsonObject | null;
  projection: GraphStructureProjection | null;
  diagnostics: ValidationIssue[];
}

export function parseGraphDraft(text: string, expectedGraphId: string): ParsedGraphDraft {
  let value: unknown;
  try { value = parseJsonExact(text); }
  catch (cause) {
    return invalid("invalid_json", "/", cause instanceof Error ? cause.message : "JSON 无法解析");
  }
  if (!isObject(value)) return invalid("graph_document_not_object", "/", "GraphDraft 必须是 JSON object。");
  if (value.graphId !== expectedGraphId) {
    return invalid("graph_identity_mismatch", "/graphId", "graphId 必须与当前 Graph 一致。");
  }
  let projection: GraphStructureProjection;
  try { projection = projectGraphStructure(value); }
  catch (cause) {
    return invalid("invalid_graph_envelope", "/", cause instanceof Error ? cause.message : "Graph 结构无法投影。");
  }
  return { document: value, projection, diagnostics: structuralDiagnostics(projection) };
}

function structuralDiagnostics(graph: GraphStructureProjection): ValidationIssue[] {
  const issues: ValidationIssue[] = [];
  const nodeIds = new Set<string>();
  for (const node of graph.nodes) {
    if (nodeIds.has(node.id)) {
      issues.push({ code: "duplicate_node_id", path: "/nodes", message: `节点 ID “${node.id}” 重复。` });
    }
    nodeIds.add(node.id);
  }
  graph.edges.forEach((edge, index) => {
    if (!nodeIds.has(edge.source)) issues.push({ code: "edge_source_missing", path: `/edges/${index}/from/nodeId`, message: `起点节点 “${edge.source}” 不存在。` });
    if (!nodeIds.has(edge.target)) issues.push({ code: "edge_target_missing", path: `/edges/${index}/to/nodeId`, message: `终点节点 “${edge.target}” 不存在。` });
  });
  return issues;
}

function invalid(code: string, path: string, message: string): ParsedGraphDraft {
  return { document: null, projection: null, diagnostics: [{ code, path, message }] };
}

function isObject(value: unknown): value is JsonObject {
  return typeof value === "object" && value !== null && !Array.isArray(value) && !isJsonNumber(value);
}
