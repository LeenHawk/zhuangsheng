import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import {
  createIdempotencyKey,
  decodeValidationIssues,
  parseGraphDraft,
  stringifyJsonExact,
  type GraphDraftView,
  type GraphRevisionView,
  type GraphSummary,
  type ValidationIssue,
} from "@zhuangsheng/api-client";
import { GraphStudio } from "@zhuangsheng/domain-ui";

import { graphs, localErrorMessage } from "./bridge";

type Status = "loading" | "ready" | "saving" | "applying";

export function LocalGraphStudio() {
  const [items, setItems] = useState<GraphSummary[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [draft, setDraft] = useState<GraphDraftView | null>(null);
  const [jsonText, setJsonText] = useState("");
  const [applied, setApplied] = useState<GraphRevisionView | null>(null);
  const [serverIssues, setServerIssues] = useState<ValidationIssue[]>([]);
  const [status, setStatus] = useState<Status>("loading");
  const [error, setError] = useState<string | null>(null);
  const keys = useRef(new Map<string, string>());

  const loadDraft = useCallback(async (graphId: string) => {
    setStatus("loading"); setError(null); setApplied(null); setServerIssues([]);
    try {
      const next = await graphs.getDraft(graphId);
      setDraft(next); setJsonText(stringifyJsonExact(next.document, 2));
    } catch (cause) { setDraft(null); setError(localErrorMessage(cause)); }
    finally { setStatus("ready"); }
  }, []);

  useEffect(() => {
    let active = true;
    void graphs.list().then((next) => {
      if (!active) return;
      setItems(next); setSelectedId((current) => current ?? next[0]?.id ?? null);
      if (next.length === 0) setStatus("ready");
    }).catch((cause) => { if (active) { setError(localErrorMessage(cause)); setStatus("ready"); } });
    return () => { active = false; };
  }, []);
  useEffect(() => { if (selectedId) void loadDraft(selectedId); }, [loadDraft, selectedId]);

  const parsed = useMemo(() => parseGraphDraft(jsonText, selectedId ?? ""), [jsonText, selectedId]);
  const savedText = draft ? stringifyJsonExact(draft.document, 2) : "";
  const dirty = draft !== null && jsonText !== savedText;
  const keyFor = (signature: string) => {
    const existing = keys.current.get(signature);
    if (existing) return existing;
    const key = createIdempotencyKey(); keys.current.set(signature, key); return key;
  };
  const complete = (signature: string) => keys.current.delete(signature);

  const create = async (name: string) => {
    const signature = `create:${name}`; setError(null);
    try {
      const result = await graphs.create(name, keyFor(signature));
      complete(signature); setItems((current) => [...current, result.graph]); setSelectedId(result.graph.id);
    } catch (cause) { setError(localErrorMessage(cause)); }
  };
  const save = async () => {
    if (!draft || !parsed.document) return;
    const signature = `save:${draft.graphId}:${draft.revisionToken}:${jsonText}`;
    setStatus("saving"); setError(null); setServerIssues([]);
    try {
      const next = await graphs.updateDraft(
        draft.graphId, draft.revisionToken, parsed.document, keyFor(signature),
      );
      complete(signature); setDraft(next); setJsonText(stringifyJsonExact(next.document, 2));
      setItems((current) => current.map((item) => item.id === next.graphId
        ? { ...item, name: typeof next.document.name === "string" ? next.document.name : item.name, updatedAt: next.updatedAt }
        : item));
    } catch (cause) { setError(commandError(cause, setServerIssues)); }
    finally { setStatus("ready"); }
  };
  const apply = async () => {
    if (!draft || dirty || !parsed.document) return;
    const signature = `apply:${draft.graphId}:${draft.revisionToken}`;
    setStatus("applying"); setError(null); setServerIssues([]); setApplied(null);
    try {
      const revision = await graphs.apply(
        draft.graphId, draft.revisionToken, keyFor(signature),
      );
      complete(signature); setApplied(revision); setServerIssues(revision.warnings);
    } catch (cause) { setError(commandError(cause, setServerIssues)); }
    finally { setStatus("ready"); }
  };

  return <GraphStudio graphs={items} selectedGraphId={selectedId} draft={draft}
    jsonText={jsonText} projection={parsed.projection}
    diagnostics={[...parsed.diagnostics, ...serverIssues]} applied={applied}
    dirty={dirty} status={status} error={error}
    onSelectGraph={(id) => { if (id !== selectedId) { setDraft(null); setSelectedId(id); } }}
    onCreateGraph={create} onJsonChange={(value) => { setJsonText(value); setApplied(null); setServerIssues([]); }}
    onSave={() => void save()} onApply={() => void apply()}
    onReload={() => { if (selectedId) void loadDraft(selectedId); }} />;
}

function commandError(cause: unknown, setIssues: (issues: ValidationIssue[]) => void): string {
  if (cause && typeof cause === "object") {
    const error = cause as { code?: unknown; details?: { issues?: unknown } };
    try { if (error.details?.issues !== undefined) setIssues(decodeValidationIssues(error.details.issues)); }
    catch { /* keep safe adapter error */ }
    if (error.code === "graph_draft_revision") return "草稿已被其他编辑更新。重新加载后比较并重做本次修改。";
  }
  return localErrorMessage(cause);
}
