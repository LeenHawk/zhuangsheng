import { useRef, useState } from "react";
import { FileJson, Import, Settings2 } from "lucide-react";

import {
  parseJsonExact,
  type ApplySillyTavernImportInput,
  type ContextPresetView,
  type JsonValue,
  type SillyTavernImportInput,
  type SillyTavernImportPreviewView,
  type SillyTavernImportResultView,
} from "@zhuangsheng/api-client";
import { Badge, Button, Card } from "@zhuangsheng/ui";

import { SillyTavernImportPreview } from "./sillytavern-import-preview";

export interface SillyTavernImportActions {
  preview(input: SillyTavernImportInput): Promise<SillyTavernImportPreviewView>;
  apply(input: ApplySillyTavernImportInput): Promise<SillyTavernImportResultView>;
}

export function SillyTavernImportCard({
  presets,
  actions,
  onImported,
}: {
  presets: ContextPresetView[];
  actions: SillyTavernImportActions;
  onImported?: (result: SillyTavernImportResultView) => void;
}) {
  const fileInput = useRef<HTMLInputElement>(null);
  const [source, setSource] = useState<{ document: JsonValue; name: string } | null>(null);
  const [targetId, setTargetId] = useState("");
  const [preview, setPreview] = useState<SillyTavernImportPreviewView | null>(null);
  const [expert, setExpert] = useState(false);
  const [pending, setPending] = useState<"preview" | "apply" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const target = presets.find((preset) => preset.id === targetId);

  const chooseFile = async (file: File | undefined) => {
    if (!file) return;
    setError(null); setPreview(null);
    try {
      const document = parseJsonExact(await file.text()) as JsonValue;
      setSource({ document, name: file.name });
    } catch (cause) {
      setSource(null); setError(message(cause, "文件不是有效的安全 JSON。"));
    }
  };
  const runPreview = async () => {
    if (!source || pending) return;
    setPending("preview"); setError(null);
    try {
      setPreview(await actions.preview({
        document: source.document, sourceName: source.name, targetPresetId: targetId || null,
      }));
    } catch (cause) { setError(message(cause, "无法解析这个酒馆预设。")); }
    finally { setPending(null); }
  };
  const apply = async () => {
    if (!source || !preview?.contextSpec || pending) return;
    setPending("apply"); setError(null);
    try {
      const result = await actions.apply({
        document: source.document,
        sourceName: source.name,
        targetPresetId: targetId || null,
        expectedHeadVersionId: target?.headVersionId ?? null,
      });
      onImported?.(result);
      setPreview(result.preview);
    } catch (cause) { setError(message(cause, "导入发布失败。")); }
    finally { setPending(null); }
  };

  return <Card className="mb-4 border-info/25 p-5">
    <div className="flex flex-wrap items-center gap-2"><Import className="size-5 text-info" /><h2 className="font-semibold">导入 SillyTavern 预设 / 正则</h2><Badge className="ml-auto" tone="info">酒馆兼容 v1</Badge></div>
    <p className="mt-2 text-sm text-secondary">先预览字段映射和不兼容项，确认后发布为版本化 ContextPreset。API key、proxy password 和连接地址不会导入。</p>
    <div className="mt-4 grid gap-3 md:grid-cols-[1fr_1fr_auto]">
      <div><input ref={fileInput} className="hidden" type="file" accept=".json,application/json" onChange={(event) => void chooseFile(event.target.files?.[0])} /><Button className="w-full" variant="secondary" onClick={() => fileInput.current?.click()}><FileJson className="size-4" />{source?.name ?? "选择酒馆 JSON"}</Button></div>
      <select className="min-h-11 rounded-xl border border-default bg-canvas px-3 text-sm" value={targetId} onChange={(event) => { setTargetId(event.target.value); setPreview(null); }}><option value="">创建新的 ContextPreset</option>{presets.map((preset) => <option key={preset.id} value={preset.id}>合并到 {preset.name}</option>)}</select>
      <Button disabled={!source || pending !== null} onClick={() => void runPreview()}>{pending === "preview" ? "解析中…" : "预览导入"}</Button>
    </div>
    <label className="mt-3 flex items-center gap-2 text-xs text-secondary"><input type="checkbox" checked={expert} onChange={(event) => setExpert(event.target.checked)} /><Settings2 className="size-3.5" />专家模式：显示 source hash、surface、placement 和 depth</label>
    {error && <div role="alert" className="mt-3 rounded-xl bg-danger/10 p-3 text-sm text-danger">{error}</div>}
    {preview && <><SillyTavernImportPreview preview={preview} expert={expert} /><div className="mt-4 flex items-center gap-3"><Button disabled={!preview.contextSpec || pending !== null} onClick={() => void apply()}>{pending === "apply" ? "正在发布…" : target ? `发布到 ${target.name}` : "确认并创建 preset"}</Button>{!preview.contextSpec && <p className="text-xs text-warning">这个文件只有生成参数，需在创建 Agent 模板时应用。</p>}</div></>}
  </Card>;
}

function message(cause: unknown, fallback: string) {
  return cause instanceof Error && cause.message ? cause.message : fallback;
}
