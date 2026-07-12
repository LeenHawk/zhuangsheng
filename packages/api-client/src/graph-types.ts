export type JsonValue = null | boolean | number | string | JsonValue[] | JsonObject;

export interface JsonObject {
  [key: string]: JsonValue;
}

export interface GraphSummary {
  id: string;
  name: string;
  createdAt: number;
  updatedAt: number;
}

export interface CreateGraphResult {
  graph: GraphSummary;
  draftRevisionToken: string;
}

export interface GraphDraftView {
  graphId: string;
  document: JsonObject;
  revisionToken: string;
  updatedAt: number;
}

export interface ValidationIssue {
  code: string;
  path: string;
  message: string;
}

export interface GraphRevisionView {
  id: string;
  graphId: string;
  revisionNo: number;
  operationTaxonomyVersion: number;
  adapterDecoderVersion: number;
  definition: JsonObject;
  contentHash: string;
  createdAt: number;
  warnings: ValidationIssue[];
}

export interface GraphPortProjection {
  name: string;
}

export interface GraphNodeProjection {
  id: string;
  kind: string;
  name: string | null;
  isEntry: boolean;
  inputs: GraphPortProjection[];
  outputs: GraphPortProjection[];
}

export interface GraphEdgeProjection {
  id: string;
  source: string;
  sourcePort: string;
  target: string;
  targetPort: string;
}

export interface GraphStructureProjection {
  nodes: GraphNodeProjection[];
  edges: GraphEdgeProjection[];
}
