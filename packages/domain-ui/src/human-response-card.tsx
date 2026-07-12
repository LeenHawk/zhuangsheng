import { useMemo, useState } from "react";
import { MessageCircleQuestion } from "lucide-react";

import type { JsonValue, WaitView } from "@zhuangsheng/api-client";
import { Button, Card, Input, Textarea } from "@zhuangsheng/ui";

type Scalar = string | boolean | number;
type Field = {
  name: string;
  label: string;
  description: string | null;
  required: boolean;
  type: "string" | "boolean" | "number";
  enumValues: Scalar[] | null;
  multiline: boolean;
};
type Form = { kind: "scalar"; field: Field } | { kind: "object"; fields: Field[] };

export function HumanResponseCard(props: {
  wait: WaitView;
  pending: boolean;
  error: string | null;
  onSubmit: (wait: WaitView, value: JsonValue) => Promise<void>;
}) {
  const form = useMemo(() => parseForm(props.wait), [props.wait]);
  const [values, setValues] = useState<Record<string, Scalar>>({});
  if (props.wait.request.kind !== "human_response") return null;
  const request = props.wait.request;
  const submit = async (event: React.FormEvent) => {
    event.preventDefault();
    if (!form) return;
    const value = form.kind === "scalar"
      ? valueFor(form.field, values[form.field.name])
      : Object.fromEntries(form.fields
          .filter((field) => field.required || values[field.name] !== undefined)
          .map((field) => [field.name, valueFor(field, values[field.name])]));
    await props.onSubmit(props.wait, value as JsonValue);
  };
  return (
    <Card className="border-info/30 p-5">
      <div className="flex items-center gap-2 font-semibold">
        <MessageCircleQuestion className="size-5 text-info" />
        {request.title || "角色正在等待你的回应"}
      </div>
      {request.description && <p className="mt-2 text-sm text-secondary">{request.description}</p>}
      {!form ? (
        <p className="mt-3 text-sm text-warning">此响应 schema 超出用户模式支持范围，请在专家模式中处理。</p>
      ) : (
        <form className="mt-4 space-y-4" onSubmit={submit}>
          {(form.kind === "scalar" ? [form.field] : form.fields).map((field) => (
            <FieldInput key={field.name} field={field} value={values[field.name]} onChange={(value) =>
              setValues((current) => ({ ...current, [field.name]: value }))} />
          ))}
          {props.error && <p role="alert" className="text-sm text-danger">{props.error}</p>}
          <Button type="submit" disabled={props.pending || !complete(form, values)}>
            {props.pending ? "提交中…" : "提交回应"}
          </Button>
        </form>
      )}
    </Card>
  );
}

function FieldInput(props: { field: Field; value: Scalar | undefined; onChange: (value: Scalar) => void }) {
  const { field } = props;
  return (
    <label className="block text-sm font-semibold">
      {field.label}{field.required && <span className="text-danger"> *</span>}
      {field.description && <span className="mt-1 block text-xs font-normal text-muted">{field.description}</span>}
      {field.enumValues ? (
        <select className="mt-2 min-h-11 w-full rounded-xl border border-default bg-canvas px-3 text-sm"
          value={props.value === undefined ? "" : String(props.value)}
          onChange={(event) => props.onChange(enumValue(field, event.target.value))}>
          <option value="" disabled>请选择</option>
          {field.enumValues.map((value) => <option key={String(value)} value={String(value)}>{String(value)}</option>)}
        </select>
      ) : field.type === "boolean" ? (
        <input className="ml-3 size-4 align-middle" type="checkbox" checked={props.value === true}
          onChange={(event) => props.onChange(event.target.checked)} />
      ) : field.multiline ? (
        <Textarea className="mt-2" value={String(props.value ?? "")} required={field.required}
          onChange={(event) => props.onChange(event.target.value)} />
      ) : (
        <Input className="mt-2" type={field.type === "number" ? "number" : "text"}
          value={String(props.value ?? "")} required={field.required}
          onChange={(event) => props.onChange(field.type === "number" ? Number(event.target.value) : event.target.value)} />
      )}
    </label>
  );
}

function parseForm(wait: WaitView): Form | null {
  const document = wait.responseSchema?.document;
  if (!document) return null;
  if (document.type === "object") {
    if (document.additionalProperties !== false || !isRecord(document.properties)) return null;
    const required = Array.isArray(document.required) ? document.required.filter((item): item is string => typeof item === "string") : [];
    const fields = Object.entries(document.properties).map(([name, raw]) => parseField(name, raw, required.includes(name)));
    return fields.every((field): field is Field => field !== null) ? { kind: "object", fields } : null;
  }
  const field = parseField("value", document, true);
  return field ? { kind: "scalar", field } : null;
}

function parseField(name: string, raw: unknown, required: boolean): Field | null {
  if (!isRecord(raw)) return null;
  const values = Array.isArray(raw.enum) && raw.enum.length > 0 && raw.enum.every(isScalar) ? raw.enum : null;
  const type = raw.type === "boolean" ? "boolean" : raw.type === "number" || raw.type === "integer" ? "number" : raw.type === "string" || values ? "string" : null;
  if (!type || (values && values.some((value) => typeof value !== type))) return null;
  return { name, label: typeof raw.title === "string" ? raw.title : name, description: typeof raw.description === "string" ? raw.description : null, required, type, enumValues: values, multiline: raw.format === "textarea" || raw["x-ui-control"] === "textarea" };
}

const complete = (form: Form, values: Record<string, Scalar>) =>
  (form.kind === "scalar" ? [form.field] : form.fields).every((field) => !field.required || values[field.name] !== undefined && values[field.name] !== "");
const valueFor = (field: Field, value: Scalar | undefined): Scalar => value ?? (field.type === "boolean" ? false : "");
const enumValue = (field: Field, raw: string): Scalar => field.enumValues?.find((value) => String(value) === raw) ?? raw;
const isScalar = (value: unknown): value is Scalar => ["string", "boolean", "number"].includes(typeof value);
const isRecord = (value: unknown): value is Record<string, unknown> => typeof value === "object" && value !== null && !Array.isArray(value);
