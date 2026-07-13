import { useState } from "react";
import { FlaskConical } from "lucide-react";

import type {
  SillyTavernRegexTestResultView,
  TestSillyTavernRegexInput,
  TextTransformPlacement,
  TextTransformSurface,
} from "@zhuangsheng/api-client";
import { Badge, Button, Card } from "@zhuangsheng/ui";

export function SillyTavernRegexTester({
  base,
  onTest,
}: {
  base: Pick<TestSillyTavernRegexInput, "document" | "sourceName" | "targetPresetId">;
  onTest: (input: TestSillyTavernRegexInput) => Promise<SillyTavernRegexTestResultView>;
}) {
  const [input, setInput] = useState("");
  const [placement, setPlacement] = useState<TextTransformPlacement>("ai_output");
  const [surface, setSurface] = useState<TextTransformSurface>("display");
  const [depth, setDepth] = useState(0);
  const [result, setResult] = useState<SillyTavernRegexTestResultView | null>(null);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const run = async () => {
    setPending(true); setError(null);
    try {
      setResult(await onTest({ ...base, input, placement, surface, depth, isEdit: false }));
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "正则试跑失败。");
    } finally { setPending(false); }
  };
  return <Card className="mt-3 border-default p-3">
    <div className="flex items-center gap-2"><FlaskConical className="size-4 text-info" /><p className="text-xs font-semibold">正则试跑</p>{result && <Badge className="ml-auto" tone={result.appliedRuleIds.length ? "success" : "neutral"}>{result.appliedRuleIds.length} 条命中</Badge>}</div>
    <textarea aria-label="正则测试文本" className="mt-3 min-h-24 w-full rounded-xl border border-default bg-canvas p-3 text-sm" value={input} onChange={(event) => setInput(event.target.value)} placeholder="输入一段消息，验证最终转换结果" />
    <div className="mt-2 grid gap-2 sm:grid-cols-3">
      <select aria-label="正则 placement" className="min-h-10 rounded-xl border border-default bg-canvas px-2 text-xs" value={placement} onChange={(event) => setPlacement(event.target.value as TextTransformPlacement)}>{PLACEMENTS.map(([value, label]) => <option key={value} value={value}>{label}</option>)}</select>
      <select aria-label="正则 surface" className="min-h-10 rounded-xl border border-default bg-canvas px-2 text-xs" value={surface} onChange={(event) => setSurface(event.target.value as TextTransformSurface)}>{SURFACES.map(([value, label]) => <option key={value} value={value}>{label}</option>)}</select>
      <label className="flex min-h-10 items-center gap-2 rounded-xl border border-default px-2 text-xs">depth<input className="w-full bg-transparent" type="number" min={0} value={depth} onChange={(event) => setDepth(Math.max(0, Number(event.target.value) || 0))} /></label>
    </div>
    <Button className="mt-2" size="compact" variant="secondary" disabled={pending} onClick={() => void run()}>{pending ? "运行中…" : "运行全部已启用规则"}</Button>
    {error && <p role="alert" className="mt-2 text-xs text-danger">{error}</p>}
    {result && <div className="mt-3 rounded-xl bg-elevated p-3"><p className="whitespace-pre-wrap text-sm">{result.text || "（空字符串）"}</p><p className="mt-2 break-all font-mono text-[10px] text-muted">{result.appliedRuleIds.join(", ") || "无规则改变文本"}</p></div>}
  </Card>;
}

const PLACEMENTS: Array<[TextTransformPlacement, string]> = [["user_input", "用户输入"], ["ai_output", "AI 输出"], ["world_info", "世界信息"], ["reasoning", "推理文本"], ["slash_command", "Slash command"]];
const SURFACES: Array<[TextTransformSurface, string]> = [["canonical", "持久文本"], ["prompt", "模型 prompt"], ["display", "界面显示"]];
