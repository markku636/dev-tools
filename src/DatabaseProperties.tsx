import { useEffect, useState } from "react";
import { api } from "./api";
import { useEscToClose, toast } from "./ui";
import { databaseOptionsSql, buildAlterDatabaseCharset } from "./sql";

const CHARSETS = ["utf8mb4", "utf8", "latin1", "ascii", "big5", "gbk", "gb2312", "utf16"];

// 資料庫屬性（MySQL）：檢視 / 變更預設字元集與定序（ALTER DATABASE，僅影響日後新表）。
export default function DatabaseProperties({ connId, db, onClose }: {
  connId: string;
  db: string;
  onClose: () => void;
}) {
  useEscToClose(onClose);
  const [charset, setCharset] = useState("utf8mb4");
  const [collation, setCollation] = useState("");
  const [curr, setCurr] = useState<{ charset: string; collation: string } | null>(null);
  const [busy, setBusy] = useState(false);

  const load = () => {
    api.runQuery(connId, databaseOptionsSql(db)).then((r) => {
      if (r.rows.length === 0) return;
      const [cs, coll] = r.rows[0];
      setCurr({ charset: cs ?? "", collation: coll ?? "" });
      if (cs) setCharset(cs);
      setCollation(coll ?? "");
    }).catch((e) => toast.error(e?.message ?? "讀取資料庫屬性失敗"));
  };
  useEffect(load, [connId, db]); // eslint-disable-line react-hooks/exhaustive-deps

  const apply = async () => {
    setBusy(true);
    try {
      await api.execDdl(connId, buildAlterDatabaseCharset(db, charset, collation));
      toast.success("資料庫屬性已更新");
      load();
    } catch (e: any) {
      toast.error(e?.message ?? "更新失敗");
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-[95]" onClick={onClose}>
      <div className="bg-[#1a212b] w-[460px] max-w-[94vw] rounded-lg border border-white/10 shadow-2xl"
        onClick={(e) => e.stopPropagation()}>
        <div className="px-5 py-3 border-b border-white/10 flex items-center gap-2">
          <span className="font-medium text-sm">資料庫屬性</span>
          <span className="text-xs text-white/40 mono">{db}</span>
          <button type="button" onClick={onClose} className="ml-auto text-white/40 hover:text-white">✕</button>
        </div>
        <div className="p-5 space-y-3 text-sm">
          <div className="text-white/40 text-xs">
            目前：{curr ? `${curr.charset} / ${curr.collation}` : "讀取中…"}　·　變更僅影響日後新表（不轉換既有資料）。
          </div>
          <div className="flex items-center gap-3">
            <span className="text-white/45 w-20 shrink-0 text-xs">字元集</span>
            <select value={charset} onChange={(e) => setCharset(e.target.value)} title="字元集"
              className="bg-[#0c1118] border border-white/15 rounded px-2 py-1 text-xs min-w-[140px]">
              {charset && !CHARSETS.includes(charset) && <option value={charset}>{charset}</option>}
              {CHARSETS.map((cs) => (<option key={cs} value={cs}>{cs}</option>))}
            </select>
          </div>
          <div className="flex items-center gap-3">
            <span className="text-white/45 w-20 shrink-0 text-xs">定序</span>
            <input value={collation} onChange={(e) => setCollation(e.target.value.replace(/[^a-zA-Z0-9_]/g, ""))}
              title="定序（可留空用預設）"
              className="bg-[#0c1118] border border-white/15 rounded px-2 py-1 text-xs w-52 mono" placeholder="定序（預設）" />
          </div>
        </div>
        <div className="px-5 py-3 border-t border-white/10 flex justify-end gap-2">
          <button type="button" onClick={onClose}
            className="px-3 py-1.5 text-sm rounded border border-white/15 hover:bg-white/5">關閉</button>
          <button type="button" onClick={apply} disabled={busy || !curr}
            className="px-3 py-1.5 text-sm rounded bg-blue-600/80 hover:bg-blue-600 disabled:opacity-40">{busy ? "套用中…" : "套用"}</button>
        </div>
      </div>
    </div>
  );
}
