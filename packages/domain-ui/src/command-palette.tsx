import { useEffect, useRef, useState } from "react";
import { Search } from "lucide-react";

import { Input } from "@zhuangsheng/ui";

export function CommandPalette<T extends string>(props: {
  items: Array<{ id: T; label: string }>;
  onSelect: (id: T) => void;
}) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const previous = useRef<HTMLElement | null>(null);
  const dialog = useRef<HTMLDivElement | null>(null);
  const close = () => { setOpen(false); setQuery(""); requestAnimationFrame(() => previous.current?.focus()); };
  useEffect(() => {
    const keydown = (event: KeyboardEvent) => {
      const target = event.target instanceof Element ? event.target : document.activeElement;
      const typing = target?.matches("input, textarea, select, [contenteditable='true']");
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k" && !typing) {
        event.preventDefault(); previous.current = document.activeElement as HTMLElement; setOpen(true);
      } else if (event.key === "Escape" && open) { event.preventDefault(); close(); }
    };
    window.addEventListener("keydown", keydown);
    return () => window.removeEventListener("keydown", keydown);
  }, [open]);
  const filtered = props.items.filter((item) => item.label.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase()));
  const trap = (event: React.KeyboardEvent) => {
    if (event.key !== "Tab" || !dialog.current) return;
    const focusable = Array.from(dialog.current.querySelectorAll<HTMLElement>("input,button:not([disabled])"));
    const first = focusable[0]; const last = focusable.at(-1);
    if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus(); }
    else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus(); }
  };
  return <><button className={`${open ? "hidden" : "hidden lg:block"} min-h-9 rounded-xl border border-default bg-surface px-3 text-xs text-muted hover:text-primary`} onClick={(event) => { previous.current = event.currentTarget; setOpen(true); }} aria-label="打开资源与命令搜索">搜索 <kbd className="ml-2 font-mono">Ctrl K</kbd></button>{open && <div className="fixed inset-0 z-50 grid place-items-start bg-black/45 px-4 pt-[12vh]" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget) close(); }}><div ref={dialog} role="dialog" aria-modal="true" aria-label="资源与命令搜索" className="mx-auto w-full max-w-xl rounded-2xl border border-default bg-surface p-3 shadow-panel" onKeyDown={trap}><label className="relative block"><Search className="pointer-events-none absolute left-3 top-3.5 size-4 text-muted" /><Input autoFocus className="pl-9" value={query} onChange={(event) => setQuery(event.target.value)} aria-label="搜索命令" placeholder="打开故事、资料库、设置或专家 surface" /></label><div className="mt-2 max-h-72 overflow-auto">{filtered.map((item) => <button key={item.id} className="flex min-h-11 w-full items-center rounded-xl px-3 text-left text-sm hover:bg-elevated focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-focus" onClick={() => { props.onSelect(item.id); close(); }}>{item.label}</button>)}{filtered.length === 0 && <p className="p-4 text-center text-sm text-muted">没有匹配的命令。</p>}</div><p className="px-3 pt-2 text-[11px] text-muted">Esc 关闭 · Tab 在对话框内移动</p></div></div>}</>;
}
