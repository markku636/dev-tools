import { useEffect, useState } from "react";
import { api, ErModel, ErTable } from "./api";

const CARD_W = 210;

export default function ErDiagram({ connId, onClose }: { connId: string; onClose: () => void }) {
  const [dbs, setDbs] = useState<string[]>([]);
  const [db, setDb] = useState("");
  const [model, setModel] = useState<ErModel | null>(null);
  const [pos, setPos] = useState<Record<string, { x: number; y: number }>>({});
  const [loading, setLoading] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    api
      .listDatabases(connId)
      .then((d) => { setDbs(d); setDb((cur) => cur || d[0] || ""); })
      .catch((e) => setErr(e?.message ?? "讀取資料庫失敗"));
  }, [connId]);

  useEffect(() => {
    if (!db) return;
    setLoading(true);
    setErr(null);
    api
      .erModel(connId, db)
      .then((m) => {
        setModel(m);
        const p: Record<string, { x: number; y: number }> = {};
        m.tables.forEach((t, i) => {
          const col = i % 4;
          const row = Math.floor(i / 4);
          p[t.name] = { x: 24 + col * (CARD_W + 60), y: 24 + row * 250 };
        });
        setPos(p);
      })
      .catch((e) => { setErr(e?.message ?? "讀取 ER 失敗"); setModel(null); })
      .finally(() => setLoading(false));
  }, [connId, db]);

  const tableByName = (n: string) => model?.tables.find((t) => t.name === n);
  const cardH = (t: ErTable) => 26 + t.columns.length * 20 + 4;
  const center = (name: string) => {
    const t = tableByName(name);
    const p = pos[name];
    if (!t || !p) return null;
    return { x: p.x + CARD_W / 2, y: p.y + cardH(t) / 2 };
  };

  const startDrag = (name: string, e: React.PointerEvent) => {
    e.preventDefault();
    const start = pos[name] ?? { x: 0, y: 0 };
    const sx = e.clientX;
    const sy = e.clientY;
    const onMove = (ev: PointerEvent) =>
      setPos((p) => ({ ...p, [name]: { x: start.x + (ev.clientX - sx), y: start.y + (ev.clientY - sy) } }));
    const onUp = () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
  };

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50" onClick={onClose}>
      <div className="bg-[#11161d] w-[92vw] h-[88vh] flex flex-col rounded-lg border border-white/10 shadow-2xl"
        onClick={(e) => e.stopPropagation()}>
        <div className="px-4 py-2 border-b border-white/10 flex items-center gap-3 text-sm">
          <span className="font-medium">ER 圖</span>
          <select value={db} onChange={(e) => setDb(e.target.value)} title="選擇資料庫 / schema"
            className="bg-black/30 border border-white/10 rounded px-2 py-1 text-xs outline-none">
            {dbs.map((d) => <option key={d} value={d}>{d}</option>)}
          </select>
          {loading && <span className="text-white/40 text-xs">讀取中…</span>}
          {model && !loading && (
            <span className="text-white/40 text-xs">{model.tables.length} 表 · {model.relations.length} 關聯（可拖曳表卡）</span>
          )}
          <button type="button" onClick={onClose} className="ml-auto text-white/40 hover:text-white">✕</button>
        </div>
        <div className="flex-1 overflow-auto relative">
          {err && <div className="p-3 text-red-400 text-sm mono">{err}</div>}
          {model && model.tables.length === 0 && !err && (
            <div className="p-3 text-white/40 text-sm">此資料庫沒有表。</div>
          )}
          {model && model.tables.length > 0 && (
            <div className="relative" style={{ width: 2400, height: 1600 }}>
              <svg className="absolute inset-0 pointer-events-none" width={2400} height={1600}>
                {model.relations.map((r, i) => {
                  const a = center(r.from_table);
                  const b = center(r.to_table);
                  if (!a || !b) return null;
                  return (
                    <line key={i} x1={a.x} y1={a.y} x2={b.x} y2={b.y}
                      stroke="#3b82f6" strokeWidth={1.5} opacity={0.55} />
                  );
                })}
              </svg>
              {model.tables.map((t) => {
                const p = pos[t.name];
                if (!p) return null;
                return (
                  <div key={t.name} className="absolute bg-[#1a212b] border border-white/15 rounded shadow-lg text-xs"
                    style={{ left: p.x, top: p.y, width: CARD_W }}>
                    <div className="px-2 py-1 bg-[#22304a] rounded-t font-medium cursor-move select-none truncate"
                      onPointerDown={(e) => startDrag(t.name, e)} title={t.name}>
                      {t.name}
                    </div>
                    <div>
                      {t.columns.map((c) => (
                        <div key={c.name} className="flex items-center gap-1 px-2 py-0.5 border-t border-white/5">
                          <span className="w-3 shrink-0">{c.pk ? "🔑" : c.fk ? "🔗" : ""}</span>
                          <span className={`truncate flex-1 ${c.fk ? "text-blue-300" : ""}`}>{c.name}</span>
                          <span className="text-white/30 truncate max-w-[72px] mono">{c.data_type}</span>
                        </div>
                      ))}
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
