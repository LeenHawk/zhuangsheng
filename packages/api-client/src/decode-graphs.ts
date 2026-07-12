import { DecodeError } from "./decode-error";
import { boolean, nullableString, number, record, string } from "./decode-helpers";
import type {
  CreateGraphResult,
  GraphDraftView,
  GraphRevisionView,
  GraphStructureProjection,
  GraphSummary,
  JsonObject,
  ValidationIssue,
} from "./graph-types";

const jsonObject = (value: unknown, path: string): JsonObject =>
  record(value, path) as JsonObject;

const graphSummary = (value: unknown, path: string): GraphSummary => {
  const item = record(value, path);
  return {
    id: string(item.id, `${path}.id`),
    name: string(item.name, `${path}.name`),
    createdAt: number(item.createdAt, `${path}.createdAt`),
    updatedAt: number(item.updatedAt, `${path}.updatedAt`),
  };
};

export const decodeGraphList = (value: unknown): GraphSummary[] => {
  if (!Array.isArray(value)) throw new DecodeError("graphs");
  return value.map((item, index) => graphSummary(item, `graphs[${index}]`));
};

export const decodeCreateGraph = (value: unknown): CreateGraphResult => {
  const item = record(value, "createGraph");
  return {
    graph: graphSummary(item.graph, "createGraph.graph"),
    draftRevisionToken: string(item.draftRevisionToken, "createGraph.draftRevisionToken"),
  };
};

export const decodeGraphDraft = (value: unknown): GraphDraftView => {
  const item = record(value, "graphDraft");
  const document = jsonObject(item.document, "graphDraft.document");
  const graphId = string(item.graphId, "graphDraft.graphId");
  if (document.graphId !== graphId) throw new DecodeError("graphDraft.document.graphId");
  return {
    graphId,
    document,
    revisionToken: string(item.revisionToken, "graphDraft.revisionToken"),
    updatedAt: number(item.updatedAt, "graphDraft.updatedAt"),
  };
};

export const decodeValidationIssues = (value: unknown, path = "issues"): ValidationIssue[] => {
  if (!Array.isArray(value)) throw new DecodeError(path);
  return value.map((raw, index) => {
    const itemPath = `${path}[${index}]`;
    const item = record(raw, itemPath);
    return {
      code: string(item.code, `${itemPath}.code`),
      path: string(item.path, `${itemPath}.path`),
      message: string(item.message, `${itemPath}.message`),
    };
  });
};

export const decodeGraphRevision = (value: unknown): GraphRevisionView => {
  const item = record(value, "graphRevision");
  return {
    id: string(item.id, "graphRevision.id"),
    graphId: string(item.graphId, "graphRevision.graphId"),
    revisionNo: number(item.revisionNo, "graphRevision.revisionNo"),
    operationTaxonomyVersion: number(item.operationTaxonomyVersion, "graphRevision.operationTaxonomyVersion"),
    adapterDecoderVersion: number(item.adapterDecoderVersion, "graphRevision.adapterDecoderVersion"),
    definition: jsonObject(item.definition, "graphRevision.definition"),
    contentHash: string(item.contentHash, "graphRevision.contentHash"),
    createdAt: number(item.createdAt, "graphRevision.createdAt"),
    warnings: decodeValidationIssues(item.warnings, "graphRevision.warnings"),
  };
};

export const projectGraphStructure = (document: JsonObject): GraphStructureProjection => {
  const rawNodes = document.nodes;
  const rawEdges = document.edges;
  if (!Array.isArray(rawNodes)) throw new DecodeError("graphDocument.nodes");
  if (!Array.isArray(rawEdges)) throw new DecodeError("graphDocument.edges");
  const nodes = rawNodes.map((raw, index) => {
    const path = `graphDocument.nodes[${index}]`;
    const item = record(raw, path);
    const ports = (value: unknown, field: string) => {
      if (!Array.isArray(value)) throw new DecodeError(`${path}.${field}`);
      return value.map((port, portIndex) => ({
        name: string(record(port, `${path}.${field}[${portIndex}]`).name, `${path}.${field}[${portIndex}].name`),
      }));
    };
    return {
      id: string(item.id, `${path}.id`),
      kind: string(item.kind, `${path}.kind`),
      name: item.name === undefined ? null : nullableString(item.name, `${path}.name`),
      isEntry: item.isEntry === undefined ? false : boolean(item.isEntry, `${path}.isEntry`),
      inputs: ports(item.inputs ?? [], "inputs"),
      outputs: ports(item.outputs ?? [], "outputs"),
    };
  });
  const edges = rawEdges.map((raw, index) => {
    const path = `graphDocument.edges[${index}]`;
    const item = record(raw, path);
    const from = record(item.from, `${path}.from`);
    const to = record(item.to, `${path}.to`);
    const source = string(from.nodeId, `${path}.from.nodeId`);
    const sourcePort = string(from.output, `${path}.from.output`);
    const target = string(to.nodeId, `${path}.to.nodeId`);
    const targetPort = string(to.input, `${path}.to.input`);
    return {
      id: typeof item.id === "string" ? item.id : `${source}:${sourcePort}->${target}:${targetPort}:${index}`,
      source,
      sourcePort,
      target,
      targetPort,
    };
  });
  return { nodes, edges };
};
