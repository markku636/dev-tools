import { useEffect, useMemo, useRef, useState } from "react";
import type { LucideIcon } from "lucide-react";
import { Search } from "lucide-react";
import Icon from "./ui/Icon";
import { fuzzyFilter } from "./fuzzy";

export interface PaletteItem {
  id: string;
  label: string;     // 主要文字（搜尋對象）
  hint?: string;     // 次要文字（資料庫 / 種類等，也納入搜尋）
  group: string;     // 連線 / 資料庫 / 資料表 / 動作
  icon?: LucideIcon;
  run: () => void;
}

// 命令面板（Ctrl/Cmd+K）：跨連線快速跳到 連線 / 資料庫 / 資料表，或執行常用動作。
// 致敬現代工具與 Navicat 的快速導覽；模糊比對排序、方向鍵 + Enter 選取、Esc 關閉。
export default function CommandPalette({ items, onClose }: { items: PaletteItem[]; onClose: () => void }) {
  const [q, setQ] = useState("");
  const [sel, setSel] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  const filtered = useMemo(
    () => fuzzyFilter(q, items, (x) => `${x.label} ${x.hint ?? ""}`, 80),
    [q, items],
  );

  useEffect(() => { setSel(0); }, [q]);
  useEffect(() => { inputRef.current?.focus(); }, []);

  // 選取項捲動到可視範圍。
  useEffect(() => {
    const el = listRef.current?.querySelector<HTMLElement>(`[data-idx="${sel}"]`);
    el?.scrollIntoView({ block: "nearest" });
  }, [sel]);

  const onKey = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown") { e.preventDefault(); setSel((s) => Math.min(s + 1, filtered.length - 1)); }
    else if (e.key === "ArrowUp") { e.preventDefault(); setSel((s) => Math.max(s - 1, 0)); }
    else if (e.key === "Enter") { e.preventDefault(); const it = filtered[sel]; if (it) { onClose(); it.run(); } }
    else if (e.key === "Escape") { e.preventDefault(); onClose(); }
  };

  return (
    <div className="fixed inset-0 z-[120] flex items-start justify-center pt-[12vh] bg-black/40 backdrop-blur-sm" onClick={onClose}>
      <div className="w-[560px] max-w-[92vw] bg-elevated border border-fg/10 rounded-lg shadow-2xl overflow-hidden flex flex-col max-h-[70vh]"
        onClick={(e) => e.stopPropagation()}>
        <div className="flex items-center gap-2 px-3 h-11 border-b border-fg/10 shrink-0">
          <Icon icon={Search} size={15} className="text-fg/40" />
          <input ref={inputRef} value={q} onChange={(e) => setQ(e.target.value)} onKeyDown={onKey}
            placeholder="跳到連線 / 資料庫 / 資料表，或執行動作…"
            className="flex-1 bg-transparent outline-none text-sm" />
          <kbd className="text-[10px] text-fg/30 border border-fg/15 rounded px-1">Esc</kbd>
        </div>
        <div ref={listRef} className="overflow-auto py-1 min-h-0">
          {filtered.length === 0 ? (
            <div className="px-3 py-6 text-center text-xs text-fg/40">無相符項目</div>
          ) : (
            filtered.map((it, i) => (
              <button key={it.id} type="button" data-idx={i}
                onMouseEnter={() => setSel(i)}
                onClick={() => { onClose(); it.run(); }}
                className={`flex items-center gap-2 w-full text-left px-3 py-1.5 text-sm ${i === sel ? "bg-accent/15 text-accent" : "text-fg/80 hover:bg-fg/5"}`}>
                {it.icon && <Icon icon={it.icon} size={13} className={i === sel ? "text-accent" : "text-fg/40"} />}
                <span className="truncate flex-1">{it.label}</span>
                {it.hint && <span className="text-[11px] text-fg/35 truncate max-w-[180px]">{it.hint}</span>}
                <span className="text-[10px] text-fg/25 shrink-0">{it.group}</span>
              </button>
            ))
          )}
        </div>
      </div>
    </div>
  );
}
