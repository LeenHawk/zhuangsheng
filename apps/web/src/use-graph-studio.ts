import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { ApiError, createIdempotencyKey, decodeValidationIssues, type GraphDraftView, type GraphRevisionView, type GraphSummary, type HttpGraphClient } from "@zhuangsheng/api-client";

import { client, messageFor } from "./api";
import { parseGraphDraft } from "./graph-draft-validation";

type Status = "loading" | "ready" | "saving" | "applying";

export function useGraphStudio(graphClient: HttpGraphClient = client.graphs) {
  const [graphs, setGraphs] = useState<GraphSummary[]>([]);
  const [selectedGraphId, setSelectedGraphId] = useState<string | null>(null);
  const [draft, setDraft] = useState<GraphDraftView | null>(null);
  const [jsonText, setJsonText] = useState("");
  const [applied, setApplied] = useState<GraphRevisionView | null>(null);
  const [serverIssues, setServerIssues] = useState<ReturnType<typeof decodeValidationIssues>>([]);
  const [status, setStatus] = useState<Status>("loading");
  const [error, setError] = useState<string | null>(null);
  const commandKeys = useRef(new Map<string, string>());

  const loadDraft = useCallback(async (graphId: string, signal?: AbortSignal) => {
    setStatus("loading");
    setError(null);
    setApplied(null);
    setServerIssues([]);
    try {
      const next = await graphClient.getDraft(graphId, signal);
      setDraft(next);
      setJsonText(JSON.stringify(next.document, null, 2));
      setStatus("ready");
    } catch (cause) {
      if (signal?.aborted) return;
      setDraft(null);
      setError(messageFor(cause));
      setStatus("ready");
    }
  }, [graphClient]);

  useEffect(() => {
    const controller = new AbortController();
    void graphClient.list(controller.signal).then((items) => {
      setGraphs(items);
      const first = items[0]?.id ?? null;
      setSelectedGraphId((current) => current ?? first);
      if (!first) setStatus("ready");
    }).catch((cause) => {
      if (!controller.signal.aborted) { setError(messageFor(cause)); setStatus("ready"); }
    });
    return () => controller.abort();
  }, [graphClient]);

  useEffect(() => {
    if (!selectedGraphId) return;
    const controller = new AbortController();
    void loadDraft(selectedGraphId, controller.signal);
    return () => controller.abort();
  }, [loadDraft, selectedGraphId]);

  const parsed = useMemo(() => parseGraphDraft(jsonText, selectedGraphId ?? ""), [jsonText, selectedGraphId]);
  const savedText = draft ? JSON.stringify(draft.document, null, 2) : "";
  const dirty = draft !== null && jsonText !== savedText;
  const keyFor = (signature: string) => {
    const existing = commandKeys.current.get(signature);
    if (existing) return existing;
    const key = createIdempotencyKey();
    commandKeys.current.set(signature, key);
    return key;
  };
  const complete = (signature: string) => commandKeys.current.delete(signature);

  const createGraph = async (name: string) => {
    const signature = `create:${name}`;
    setError(null);
    try {
      const result = await graphClient.create(name, { idempotencyKey: keyFor(signature) });
      complete(signature);
      setGraphs((items) => [...items, result.graph]);
      setSelectedGraphId(result.graph.id);
    } catch (cause) { setError(messageFor(cause)); }
  };

  const save = async () => {
    if (!draft || !parsed.document) return;
    const signature = `save:${draft.graphId}:${draft.revisionToken}:${jsonText}`;
    setStatus("saving"); setError(null); setServerIssues([]);
    try {
      const next = await graphClient.updateDraft(draft.graphId, draft.revisionToken, parsed.document, { idempotencyKey: keyFor(signature) });
      complete(signature); setDraft(next); setJsonText(JSON.stringify(next.document, null, 2)); setStatus("ready");
      setGraphs((items) => items.map((item) => item.id === next.graphId ? { ...item, name: typeof next.document.name === "string" ? next.document.name : item.name, updatedAt: next.updatedAt } : item));
    } catch (cause) { setError(commandError(cause, setServerIssues)); setStatus("ready"); }
  };

  const apply = async () => {
    if (!draft || dirty || !parsed.document) return;
    const signature = `apply:${draft.graphId}:${draft.revisionToken}`;
    setStatus("applying"); setError(null); setServerIssues([]); setApplied(null);
    try {
      const revision = await graphClient.apply(draft.graphId, draft.revisionToken, { idempotencyKey: keyFor(signature) });
      complete(signature); setApplied(revision); setServerIssues(revision.warnings); setStatus("ready");
    } catch (cause) { setError(commandError(cause, setServerIssues)); setStatus("ready"); }
  };

  const selectGraph = (id: string) => { if (id !== selectedGraphId) { setDraft(null); setSelectedGraphId(id); } };
  const reload = () => { if (selectedGraphId) void loadDraft(selectedGraphId); };
  const changeJson = (value: string) => { setJsonText(value); setApplied(null); setServerIssues([]); };
  return { graphs, selectedGraphId, draft, jsonText, projection: parsed.projection, diagnostics: [...parsed.diagnostics, ...serverIssues], applied, dirty, status, error, selectGraph, createGraph, changeJson, save, apply, reload };
}

function commandError(cause: unknown, setIssues: (issues: ReturnType<typeof decodeValidationIssues>) => void): string {
  if (cause instanceof ApiError && cause.body.details && typeof cause.body.details === "object") {
    const issues = (cause.body.details as { issues?: unknown }).issues;
    try { if (issues !== undefined) setIssues(decodeValidationIssues(issues)); } catch { /* safe error message remains */ }
    if (cause.body.code === "graph_draft_revision") return "草稿已被其他编辑更新。重新加载后比较并重做本次修改。";
  }
  return messageFor(cause);
}
