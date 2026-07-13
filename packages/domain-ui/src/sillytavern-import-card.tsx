import { useRef, useState } from "react";
import { FileJson, Import, Settings2 } from "lucide-react";

import {
  parseJsonExact,
  type ChannelView,
  type ContextPresetView,
  type JsonValue,
} from "@zhuangsheng/api-client";
import type {
  ApplySillyTavernImportInput,
  SillyTavernExportBundleView,
  SillyTavernImportInput,
  SillyTavernImportPreviewView,
  SillyTavernImportResultView,
  SillyTavernRegexTestResultView,
  TestSillyTavernRegexInput,
} from "@zhuangsheng/sillytavern-compat";
import { Badge, Button, Card } from "@zhuangsheng/ui";

import { SillyTavernImportPreview } from "./sillytavern-import-preview";
import { SillyTavernRegexTester } from "./sillytavern-regex-tester";

export interface SillyTavernImportActions {
  preview(input: SillyTavernImportInput): Promise<SillyTavernImportPreviewView>;
  apply(input: ApplySillyTavernImportInput): Promise<SillyTavernImportResultView>;
  test(input: TestSillyTavernRegexInput): Promise<SillyTavernRegexTestResultView>;
  export(versionId: string): Promise<SillyTavernExportBundleView>;
}

export function SillyTavernImportCard({
  presets,
  channels,
  actions,
  onImported,
}: {
  presets: ContextPresetView[];
  channels: ChannelView[];
  actions: SillyTavernImportActions;
  onImported?: (result: SillyTavernImportResultView) => void;
}) {
  const fileInput = useRef<HTMLInputElement>(null);
  const [source, setSource] = useState<{ document: JsonValue; name: string } | null>(null);
  const [targetId, setTargetId] = useState("");
  const [channelId, setChannelId] = useState("");
  const [preview, setPreview] = useState<SillyTavernImportPreviewView | null>(null);
  const [expert, setExpert] = useState(false);
  const [pending, setPending] = useState<"preview" | "apply" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const target = presets.find((preset) => preset.id === targetId);
  const usableChannels = channels.filter((channel) => channel.headRevisionId);
  const generationOnlyBlocked = preview !== null && preview.contextSpec === null
    && (!target?.headVersionId || !channelId);

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
      const next = await actions.preview({
        document: source.document, sourceName: source.name, targetPresetId: targetId || null,
      });
      setPreview(next);
      if ((next.generation || next.providerExtensions) && !channelId) {
        setChannelId(usableChannels[0]?.id ?? "");
      }
    } catch (cause) { setError(message(cause, "无法解析这个酒馆预设。")); }
    finally { setPending(null); }
  };
  const apply = async () => {
    if (!source || !preview || generationOnlyBlocked || pending) return;
    setPending("apply"); setError(null);
    try {
      const result = await actions.apply({
        document: source.document,
        sourceName: source.name,
        targetPresetId: targetId || null,
        expectedHeadVersionId: target?.headVersionId ?? null,
        channelId: channelId || null,
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
    {preview && <><SillyTavernImportPreview preview={preview} expert={expert} />
      {preview.textTransforms.length > 0 && <SillyTavernRegexTester base={{ document: source!.document, sourceName: source!.name, targetPresetId: targetId || null }} onTest={actions.test} />}
      <div className="mt-4 grid gap-2 sm:grid-cols-[1fr_auto]">
        <select aria-label="同时创建 Agent" className="min-h-11 rounded-xl border border-default bg-canvas px-3 text-sm" value={channelId} onChange={(event) => setChannelId(event.target.value)}>
          <option value="">只发布 ContextPreset，不创建 Agent</option>
          {usableChannels.map((channel) => <option key={channel.id} value={channel.id}>使用 {channel.name} 创建可运行 Agent</option>)}
        </select>
        <Button disabled={generationOnlyBlocked || pending !== null} onClick={() => void apply()}>{pending === "apply" ? "正在发布…" : channelId ? "导入并创建 Agent" : target ? `发布到 ${target.name}` : "确认并创建 preset"}</Button>
      </div>
      {!preview.contextSpec && <p className="mt-2 text-xs text-warning">这个文件只有生成参数：请选择已发布的 ContextPreset 和 Channel，生成参数会固定进新的 Agent revision。</p>}
      {preview.contextSpec && (preview.generation || preview.providerExtensions) && !channelId && <p className="mt-2 text-xs text-warning">未选择 Channel：本次只发布 ContextPreset，生成参数不会写入 preset。</p>}
    </>}
  </Card>;
}

function message(cause: unknown, fallback: string) {
  return cause instanceof Error && cause.message ? cause.message : fallback;
}
