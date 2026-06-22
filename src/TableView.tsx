import { useEffect, useState } from "react";
import {
  api, ColumnInfo, Filter, KeyDetail, KeyEdit, PagedData, RowInsert, Sort, SortDir,
} from "./api";
import { OpenTab, useStore } from "./store";
import { toast, uiConfirm } from "./ui";
import ExportDialog from "./ExportDialog";
import { AlterOp } from "./api";

const PAGE_SIZE = 100;
const DEFAULT_COL_W = 160;
const MIN_COL_W = 60;

// 由 connId 查出該連線是否為 Redis（決定雙擊列開「鍵詳情」而非編輯）
function useIsRedis(connId: string): boolean {
  return useStore((s) => s.connections.find((c) => c.id === connId)?.kind === "redis");
}

export default function TableView({ tab }: { tab: OpenTab }) {
  const setTabView = useStore((s) => s.setTabView);

  return (
    <div className="flex-1 flex flex-col min-h-0">
      {/* 結構 / 資料 分頁切換（Navicat 手感） */}
      <div className="flex items-center gap-1 px-2 py-1 bg-[#161c25] border-b border-white/10">
        <span className="text-xs text-white/40 mr-2 pl-1">{tab.table}</span>
        {(["data", "structure"] as const).map((v) => (
          <button
            key={v}
            onClick={() => setTabView(tab.key, v)}
            className={`text-xs px-3 py-1 rounded ${
              tab.view === v ? "bg-white/10 text-white" : "text-white/50 hover:bg-white/5"
            }`}
          >
            {v === "data" ? "資料" : "結構"}
          </button>
        ))}
      </div>
      {tab.view === "data" ? <DataPane tab={tab} /> : <StructurePane tab={tab} />}
    </div>
  );
}

// ---- 資料分頁：表格 + 底部導覽列 ----
function DataPane({ tab }: { tab: OpenTab }) {
  const [page, setPage] = useState(0);
  const [data, setData] = useState<PagedData | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  // 待套用的編輯：key 為 `行索引:欄索引`，值為新字串（null 代表設為 NULL）
  const [edits, setEdits] = useState<Record<string, string | null>>({});
  const [editing, setEditing] = useState<{ r: number; c: number } | null>(null);
  const [applying, setApplying] = useState(false);

  // 排序與篩選
  const [sorts, setSorts] = useState<Sort[]>([]);
  const [filters, setFilters] = useState<Filter[]>([]);
  const [matchAny, setMatchAny] = useState(false); // 多欄篩選：false=AND、true=OR
  const [showFilter, setShowFilter] = useState(false);

  // 新增列 / 匯出對話框
  const [inserting, setInserting] = useState(false);
  const [exporting, setExporting] = useState(false);

  // Redis：雙擊 key 列顯示鍵詳情
  const isRedis = useIsRedis(tab.connId);
  const [detailKey, setDetailKey] = useState<string | null>(null);

  // 欄寬（以欄名為鍵），per-table 持久化於 localStorage。
  const widthsKey = `colw:${tab.connId}:${tab.database}:${tab.table}`;
  const [widths, setWidths] = useState<Record<string, number>>({});
  useEffect(() => {
    try {
      const raw = localStorage.getItem(widthsKey);
      setWidths(raw ? JSON.parse(raw) : {});
    } catch {
      setWidths({});
    }
  }, [widthsKey]);
  const colWidth = (c: string) => widths[c] ?? DEFAULT_COL_W;

  // 拖曳表頭右緣調整欄寬（在 window 上掛 move/up，拖出表頭也能追蹤）。
  const startResize = (col: string, e: React.PointerEvent) => {
    e.preventDefault();
    e.stopPropagation();
    const startX = e.clientX;
    const startW = colWidth(col);
    const onMove = (ev: PointerEvent) => {
      const next = Math.max(MIN_COL_W, startW + (ev.clientX - startX));
      setWidths((w) => ({ ...w, [col]: next }));
    };
    const onUp = () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      document.body.style.cursor = "";
      document.body.style.userSelect = "";
      setWidths((w) => {
        try {
          localStorage.setItem(widthsKey, JSON.stringify(w));
        } catch {
          /* 忽略寫入失敗 */
        }
        return w;
      });
    };
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
  };

  const load = () => {
    let cancelled = false;
    setLoading(true);
    setErr(null);
    api
      .tableData(tab.connId, tab.database, tab.table, {
        page,
        page_size: PAGE_SIZE,
        filters,
        sorts,
        match_any: matchAny,
      })
      .then((d) => {
        if (!cancelled) {
          setData(d);
          setEdits({});
          setEditing(null);
        }
      })
      .catch((e) => !cancelled && setErr(e?.message ?? "讀取失敗"))
      .finally(() => !cancelled && setLoading(false));
    return () => {
      cancelled = true;
    };
  };

  useEffect(load, [tab.connId, tab.database, tab.table, page, sorts, filters, matchAny]);

  const totalPages = data ? Math.max(1, Math.ceil(data.total_rows / PAGE_SIZE)) : 1;
  const startRow = data ? page * PAGE_SIZE : 0;
  const editable = !!data && data.primary_key.length > 0;
  const dirtyCount = Object.keys(edits).length;

  const cellValue = (r: number, c: number): string | null => {
    const key = `${r}:${c}`;
    if (key in edits) return edits[key];
    return data!.rows[r][c];
  };

  const commitEdit = (r: number, c: number, raw: string, setNull: boolean) => {
    const original = data!.rows[r][c];
    const next = setNull ? null : raw;
    const key = `${r}:${c}`;
    setEdits((e) => {
      const copy = { ...e };
      if (next === original) delete copy[key];
      else copy[key] = next;
      return copy;
    });
    setEditing(null);
  };

  const applyEdits = async () => {
    if (!data || dirtyCount === 0) return;
    setApplying(true);
    setErr(null);
    try {
      for (const [key, newVal] of Object.entries(edits)) {
        const [rStr, cStr] = key.split(":");
        const r = Number(rStr);
        const c = Number(cStr);
        const pkValues = data.primary_key.map(
          (pkCol) => data.rows[r][data.columns.indexOf(pkCol)]
        );
        await api.updateCell(tab.connId, tab.database, tab.table, {
          column: data.columns[c],
          new_value: newVal,
          pk_columns: data.primary_key,
          pk_values: pkValues,
        });
      }
      load();
    } catch (e: any) {
      setErr(e?.message ?? "套用失敗");
    } finally {
      setApplying(false);
    }
  };

  // 點欄位標題循環切換排序：無 → asc → desc → 無
  const toggleSort = (col: string) => {
    setPage(0);
    setSorts((prev) => {
      const existing = prev.find((s) => s.column === col);
      if (!existing) return [{ column: col, dir: "asc" }];
      if (existing.dir === "asc") return [{ column: col, dir: "desc" }];
      return [];
    });
  };
  const sortDirOf = (col: string): SortDir | null =>
    sorts.find((s) => s.column === col)?.dir ?? null;

  const deleteRow = async (r: number) => {
    if (!data || !editable) return;
    const pkValues = data.primary_key.map(
      (pkCol) => data.rows[r][data.columns.indexOf(pkCol)]
    );
    if (!(await uiConfirm("確定刪除此列？此動作無法復原。", { title: "刪除列", danger: true, confirmText: "刪除" }))) return;
    setApplying(true);
    setErr(null);
    try {
      await api.deleteRow(tab.connId, tab.database, tab.table, {
        pk_columns: data.primary_key,
        pk_values: pkValues,
      });
      load();
    } catch (e: any) {
      setErr(e?.message ?? "刪除失敗");
    } finally {
      setApplying(false);
    }
  };

  const submitInsert = async (row: RowInsert) => {
    setApplying(true);
    setErr(null);
    try {
      await api.insertRow(tab.connId, tab.database, tab.table, row);
      setInserting(false);
      load();
    } catch (e: any) {
      setErr(e?.message ?? "新增失敗");
    } finally {
      setApplying(false);
    }
  };

  return (
    <div className="flex-1 flex flex-col min-h-0">
      {/* 動作列：篩選切換 + 新增列 */}
      <div className="flex items-center gap-1 px-2 py-1 bg-[#10161e] border-b border-white/10 text-xs">
        <button
          onClick={() => setShowFilter((s) => !s)}
          className={`px-2 py-1 rounded hover:bg-white/10 ${
            filters.length ? "text-amber-300" : "text-white/50"
          }`}
        >
          ⧩ 篩選{filters.length ? `（${filters.length}）` : ""}
        </button>
        <button
          onClick={() => editable && setInserting(true)}
          disabled={!editable}
          title={editable ? "新增列" : "此表無主鍵，不可編輯"}
          className="px-2 py-1 rounded hover:bg-white/10 text-white/50 disabled:opacity-30 disabled:hover:bg-transparent"
        >
          ＋ 新增列
        </button>
        {sorts.length > 0 && (
          <button
            onClick={() => setSorts([])}
            className="px-2 py-1 rounded hover:bg-white/10 text-white/50"
          >
            ⇅ 清除排序
          </button>
        )}
        <button
          onClick={() => data && data.columns.length > 0 && setExporting(true)}
          disabled={!data || data.columns.length === 0}
          title="匯出資料（CSV / TSV / JSON / SQL / Markdown）"
          className="px-2 py-1 rounded hover:bg-white/10 text-white/50 disabled:opacity-30 disabled:hover:bg-transparent"
        >
          ⬇ 匯出
        </button>
      </div>

      {showFilter && data && (
        <FilterBar
          columns={data.columns}
          filters={filters}
          matchAny={matchAny}
          onApply={(f, any) => { setPage(0); setMatchAny(any); setFilters(f); }}
        />
      )}

      <div className="flex-1 overflow-auto">
        {err && <div className="p-3 text-red-400 text-sm mono">{err}</div>}
        {data && data.columns.length > 0 && (
          <table
            className="text-sm border-collapse"
            style={{
              tableLayout: "fixed",
              width: 48 + data.columns.reduce((a, c) => a + colWidth(c), 0) + (editable ? 32 : 0),
            }}
          >
            <thead className="sticky top-0 bg-[#1a212b]">
              <tr>
                <th className="text-left px-3 py-1.5 border-b border-white/10 text-white/30 w-12">#</th>
                {data.columns.map((c) => {
                  const dir = sortDirOf(c);
                  return (
                    <th
                      key={c}
                      onClick={() => toggleSort(c)}
                      title="點擊排序"
                      style={{ width: colWidth(c) }}
                      className="relative text-left px-3 py-1.5 border-b border-white/10 font-medium whitespace-nowrap overflow-hidden text-ellipsis cursor-pointer select-none hover:bg-white/5"
                    >
                      {c}
                      {data.primary_key.includes(c) && (
                        <span className="ml-1 text-[10px] text-blue-400">PK</span>
                      )}
                      {dir && (
                        <span className="ml-1 text-[10px] text-amber-300">
                          {dir === "asc" ? "▲" : "▼"}
                        </span>
                      )}
                      <span
                        onPointerDown={(e) => startResize(c, e)}
                        onClick={(e) => e.stopPropagation()}
                        title="拖曳調整欄寬"
                        className="absolute right-0 top-0 h-full w-1.5 cursor-col-resize hover:bg-blue-500/50"
                      />
                    </th>
                  );
                })}
                {editable && <th className="w-8 border-b border-white/10" />}
              </tr>
            </thead>
            <tbody className="mono">
              {data.rows.map((row, i) => (
                <tr key={i} className="hover:bg-white/5 group">
                  <td className="px-3 py-1 border-b border-white/5 text-white/30">{startRow + i + 1}</td>
                  {row.map((_, j) => {
                    const key = `${i}:${j}`;
                    const isEditing = editing?.r === i && editing?.c === j;
                    const val = cellValue(i, j);
                    const dirty = key in edits;
                    const colName = data.columns[j];
                    // Redis 的 key 欄：雙擊開鍵詳情；其餘照常（ttl 可編輯）
                    const redisKeyCol = isRedis && colName === "key";
                    return (
                      <td
                        key={j}
                        onDoubleClick={() => {
                          if (redisKeyCol) setDetailKey(val);
                          else if (editable) setEditing({ r: i, c: j });
                        }}
                        title={redisKeyCol ? "雙擊檢視鍵內容" : val ?? "NULL"}
                        className={`px-3 py-1 border-b border-white/5 whitespace-nowrap overflow-hidden text-ellipsis ${
                          dirty ? "bg-amber-500/15" : ""
                        } ${redisKeyCol ? "cursor-pointer text-blue-300" : editable ? "cursor-cell" : ""}`}
                      >
                        {isEditing ? (
                          <CellEditor
                            initial={val}
                            onCommit={(raw, setNull) => commitEdit(i, j, raw, setNull)}
                            onCancel={() => setEditing(null)}
                          />
                        ) : val === null ? (
                          <span className="text-white/30 italic">NULL</span>
                        ) : (
                          val
                        )}
                      </td>
                    );
                  })}
                  {editable && (
                    <td className="px-1 py-1 border-b border-white/5 text-center">
                      <button
                        onClick={() => deleteRow(i)}
                        title="刪除此列"
                        className="w-5 h-5 rounded text-white/20 group-hover:text-red-400 hover:bg-red-500/20"
                      >
                        −
                      </button>
                    </td>
                  )}
                </tr>
              ))}
            </tbody>
          </table>
        )}
        {data && data.columns.length > 0 && data.rows.length === 0 && (
          <div className="p-3 text-white/40 text-sm">無符合條件的資料。</div>
        )}
      </div>

      {/* 底部導覽列（Navicat 手感） */}
      <div className="h-9 bg-[#11161d] border-t border-white/10 flex items-center px-3 gap-1 text-sm">
        <NavBtn label="⏮" disabled={page === 0 || loading} onClick={() => setPage(0)} title="第一頁" />
        <NavBtn label="◀" disabled={page === 0 || loading} onClick={() => setPage((p) => p - 1)} title="上一頁" />
        <span className="px-2 text-white/60 mono text-xs">
          {page + 1} / {totalPages}
        </span>
        <NavBtn label="▶" disabled={page + 1 >= totalPages || loading} onClick={() => setPage((p) => p + 1)} title="下一頁" />
        <NavBtn label="⏭" disabled={page + 1 >= totalPages || loading} onClick={() => setPage(totalPages - 1)} title="最後一頁" />

        <div className="w-px h-4 bg-white/10 mx-2" />
        <button
          onClick={applyEdits}
          disabled={dirtyCount === 0 || applying}
          title="套用變更"
          className="h-6 px-2 flex items-center gap-1 rounded text-xs bg-green-600/80 hover:bg-green-600 disabled:opacity-25 disabled:bg-transparent disabled:hover:bg-transparent"
        >
          ✓ 套用{dirtyCount > 0 ? `（${dirtyCount}）` : ""}
        </button>
        <button
          onClick={() => { setEdits({}); setEditing(null); }}
          disabled={dirtyCount === 0 || applying}
          title="捨棄變更"
          className="h-6 px-2 flex items-center rounded text-xs hover:bg-white/10 disabled:opacity-25 disabled:hover:bg-transparent"
        >
          ✗ 捨棄
        </button>

        <span className="ml-auto text-white/40 text-xs">
          {applying
            ? "處理中…"
            : data
            ? `共 ${data.total_rows} 列${editable ? "" : " · 無主鍵唯讀"}`
            : loading
            ? "讀取中…"
            : ""}
        </span>
      </div>

      {inserting && data && (
        <InsertDialog
          columns={data.columns}
          onCancel={() => setInserting(false)}
          onSubmit={submitInsert}
          busy={applying}
        />
      )}

      {exporting && data && (
        <ExportDialog
          connId={tab.connId}
          database={tab.database}
          table={tab.table}
          query={{ page: 0, page_size: PAGE_SIZE, filters, sorts, match_any: matchAny }}
          onClose={() => setExporting(false)}
        />
      )}

      {detailKey !== null && (
        <KeyDetailModal
          connId={tab.connId}
          database={tab.database}
          table={tab.table}
          rkey={detailKey}
          onClose={() => { setDetailKey(null); load(); }}
        />
      )}
    </div>
  );
}

// Redis 鍵詳情：依型別呈現五種資料結構，並支援元素級編輯
function KeyDetailModal({ connId, database, table, rkey, onClose }: {
  connId: string; database: string; table: string; rkey: string; onClose: () => void;
}) {
  const [detail, setDetail] = useState<KeyDetail | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [nonce, setNonce] = useState(0);

  useEffect(() => {
    let cancelled = false;
    api
      .keyDetail(connId, database, rkey)
      .then((d) => !cancelled && setDetail(d))
      .catch((e) => !cancelled && setErr(e?.message ?? "讀取失敗"));
    return () => { cancelled = true; };
  }, [connId, database, rkey, nonce]);

  const reload = () => setNonce((n) => n + 1);

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50" onClick={onClose}>
      <div className="bg-[#1a212b] w-[560px] max-h-[80vh] flex flex-col rounded-lg border border-white/10 shadow-2xl"
        onClick={(e) => e.stopPropagation()}>
        <div className="px-5 py-3 border-b border-white/10 flex items-center gap-2">
          <span className="font-medium text-sm mono truncate">{rkey}</span>
          {detail && (
            <>
              <span className="text-[10px] px-1.5 py-0.5 rounded bg-red-500/20 text-red-300">{detail.type_}</span>
              <span className="text-xs text-white/40">
                TTL: {detail.ttl < 0 ? "無到期" : `${detail.ttl}s`}
              </span>
            </>
          )}
          <button type="button" onClick={onClose} className="ml-auto text-white/40 hover:text-white">✕</button>
        </div>
        <div className="p-4 overflow-auto">
          {err && <div className="text-red-400 text-sm mono mb-2 break-all">{err}</div>}
          {!detail && !err && <div className="text-white/40 text-sm">讀取中…</div>}
          {detail && (
            <KeyDetailBody
              detail={detail}
              connId={connId}
              database={database}
              table={table}
              rkey={rkey}
              reload={reload}
              onError={setErr}
            />
          )}
        </div>
      </div>
    </div>
  );
}

function KeyDetailBody({ detail, connId, database, table, rkey, reload, onError }: {
  detail: KeyDetail;
  connId: string; database: string; table: string; rkey: string;
  reload: () => void;
  onError: (msg: string | null) => void;
}) {
  const { type_, entries, fields, scores } = detail;
  const [busy, setBusy] = useState(false);
  // 新增列輸入（依型別語意不同：A = field/member/value，B = value/score）
  const [addA, setAddA] = useState("");
  const [addB, setAddB] = useState("");
  const [addFront, setAddFront] = useState(false);

  const run = async (fn: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await fn();
      onError(null);
      setAddA("");
      setAddB("");
      reload();
    } catch (e: any) {
      onError(e?.message ?? "操作失敗");
    } finally {
      setBusy(false);
    }
  };
  const edit = (e: KeyEdit) => run(() => api.keyEdit(connId, database, rkey, e));

  if (type_ === "none") {
    return <div className="text-white/40 text-sm">（此鍵已不存在）</div>;
  }

  if (type_ === "string") {
    return (
      <StringEditor
        value={entries[0] ?? ""}
        busy={busy}
        onSave={(v) =>
          run(() =>
            api.updateCell(connId, database, table, {
              column: "value",
              new_value: v,
              pk_columns: ["key"],
              pk_values: [rkey],
            })
          )
        }
      />
    );
  }

  if (type_ === "hash") {
    return (
      <table className="text-sm border-collapse w-full mono">
        <thead><tr>
          <th className="text-left px-2 py-1 border-b border-white/10 w-1/3">field</th>
          <th className="text-left px-2 py-1 border-b border-white/10">value</th>
          <th className="w-8 border-b border-white/10" />
        </tr></thead>
        <tbody>
          {fields.map((f, i) => (
            <tr key={i} className="hover:bg-white/5 group">
              <td className="px-2 py-1 border-b border-white/5 break-all text-white/70">{f}</td>
              <td className="px-2 py-1 border-b border-white/5 break-all">
                <InlineEdit value={entries[i] ?? ""} onSave={(v) => edit({ action: "hash_set", field: f, value: v })} />
              </td>
              <DelCell busy={busy} onClick={() => edit({ action: "hash_remove", field: f })} />
            </tr>
          ))}
          <tr>
            <td className="px-2 py-1"><AddInput value={addA} onChange={setAddA} placeholder="field" /></td>
            <td className="px-2 py-1"><AddInput value={addB} onChange={setAddB} placeholder="value" /></td>
            <AddCell busy={busy} onClick={() => addA && edit({ action: "hash_set", field: addA, value: addB })} />
          </tr>
        </tbody>
      </table>
    );
  }

  if (type_ === "zset") {
    return (
      <table className="text-sm border-collapse w-full mono">
        <thead><tr>
          <th className="text-left px-2 py-1 border-b border-white/10 w-24">score</th>
          <th className="text-left px-2 py-1 border-b border-white/10">member</th>
          <th className="w-8 border-b border-white/10" />
        </tr></thead>
        <tbody>
          {entries.map((m, i) => (
            <tr key={i} className="hover:bg-white/5 group">
              <td className="px-2 py-1 border-b border-white/5 text-white/60">
                <InlineEdit value={String(scores[i] ?? 0)} type="number"
                  onSave={(v) => { const s = Number(v); if (Number.isFinite(s)) edit({ action: "zset_add", member: m, score: s }); }} />
              </td>
              <td className="px-2 py-1 border-b border-white/5 break-all">{m}</td>
              <DelCell busy={busy} onClick={() => edit({ action: "zset_remove", member: m })} />
            </tr>
          ))}
          <tr>
            <td className="px-2 py-1"><AddInput value={addB} onChange={setAddB} placeholder="score" type="number" /></td>
            <td className="px-2 py-1"><AddInput value={addA} onChange={setAddA} placeholder="member" /></td>
            <AddCell busy={busy}
              onClick={() => { const s = Number(addB); if (addA && Number.isFinite(s)) edit({ action: "zset_add", member: addA, score: s }); }} />
          </tr>
        </tbody>
      </table>
    );
  }

  // list / set
  const isList = type_ === "list";
  return (
    <table className="text-sm border-collapse w-full mono">
      <thead><tr>
        <th className="text-left px-2 py-1 border-b border-white/10 w-12 text-white/30">{isList ? "#" : ""}</th>
        <th className="text-left px-2 py-1 border-b border-white/10">value</th>
        <th className="w-8 border-b border-white/10" />
      </tr></thead>
      <tbody>
        {entries.map((v, i) => (
          <tr key={i} className="hover:bg-white/5 group">
            <td className="px-2 py-1 border-b border-white/5 text-white/30">{isList ? i : ""}</td>
            <td className="px-2 py-1 border-b border-white/5 break-all">
              {isList ? (
                <InlineEdit value={v} onSave={(nv) => edit({ action: "list_set", index: i, value: nv })} />
              ) : (
                // set 成員無法就地改名 → 移除舊 + 新增新
                <InlineEdit value={v} onSave={(nv) => {
                  if (nv !== v) run(async () => {
                    await api.keyEdit(connId, database, rkey, { action: "set_remove", member: v });
                    await api.keyEdit(connId, database, rkey, { action: "set_add", member: nv });
                  });
                }} />
              )}
            </td>
            {/* 註：list 刪除以值比對（LREM），重複值會刪到第一個相符項。 */}
            <DelCell busy={busy}
              onClick={() => edit(isList ? { action: "list_remove", value: v, count: 1 } : { action: "set_remove", member: v })} />
          </tr>
        ))}
        <tr>
          <td className="px-2 py-1">
            {isList && (
              <label className="text-[10px] text-white/40 flex items-center gap-1">
                <input type="checkbox" checked={addFront} onChange={(e) => setAddFront(e.target.checked)} />
                前端
              </label>
            )}
          </td>
          <td className="px-2 py-1"><AddInput value={addA} onChange={setAddA} placeholder="value" /></td>
          <AddCell busy={busy}
            onClick={() => addA && edit(isList ? { action: "list_push", value: addA, front: addFront } : { action: "set_add", member: addA })} />
        </tr>
      </tbody>
    </table>
  );
}

// 點擊就地編輯的儲存格（Enter 套用 / Esc 取消 / blur 套用）
function InlineEdit({ value, onSave, type = "text" }: {
  value: string; onSave: (v: string) => void; type?: string;
}) {
  const [editing, setEditing] = useState(false);
  const [text, setText] = useState(value);
  useEffect(() => setText(value), [value]);
  if (!editing) {
    return (
      <span onClick={() => { setText(value); setEditing(true); }} title="點擊編輯"
        className="cursor-text hover:bg-white/10 rounded px-1 -mx-1 inline-block min-w-[2rem]">
        {value === "" ? <span className="text-white/25">（空）</span> : value}
      </span>
    );
  }
  return (
    <input autoFocus type={type} value={text}
      onChange={(e) => setText(e.target.value)}
      onKeyDown={(e) => {
        if (e.key === "Enter") { setEditing(false); if (text !== value) onSave(text); }
        else if (e.key === "Escape") { setEditing(false); setText(value); }
      }}
      onBlur={() => { setEditing(false); if (text !== value) onSave(text); }}
      className="bg-black/50 border border-blue-500 rounded px-1 py-0.5 outline-none w-full" />
  );
}

function StringEditor({ value, onSave, busy }: {
  value: string; onSave: (v: string) => void; busy: boolean;
}) {
  const [text, setText] = useState(value);
  useEffect(() => setText(value), [value]);
  return (
    <div className="space-y-2">
      <textarea value={text} onChange={(e) => setText(e.target.value)} title="字串值"
        className="w-full h-40 bg-black/30 border border-white/10 rounded p-3 mono text-sm outline-none focus:border-blue-500 resize-none break-all" />
      <div className="flex justify-end">
        <button type="button" disabled={busy || text === value} onClick={() => onSave(text)}
          className="px-3 py-1 text-sm rounded bg-blue-600 hover:bg-blue-500 disabled:opacity-40">
          {busy ? "儲存中…" : "儲存"}
        </button>
      </div>
    </div>
  );
}

function DelCell({ onClick, busy }: { onClick: () => void; busy: boolean }) {
  return (
    <td className="px-1 py-1 border-b border-white/5 text-center">
      <button type="button" onClick={onClick} disabled={busy} title="刪除"
        className="w-5 h-5 rounded text-white/20 group-hover:text-red-400 hover:bg-red-500/20 disabled:opacity-30">
        −
      </button>
    </td>
  );
}

function AddCell({ onClick, busy }: { onClick: () => void; busy: boolean }) {
  return (
    <td className="px-1 py-1 text-center">
      <button type="button" onClick={onClick} disabled={busy} title="新增"
        className="w-5 h-5 rounded text-white/30 hover:text-green-400 hover:bg-green-500/20 disabled:opacity-30">
        ＋
      </button>
    </td>
  );
}

function AddInput({ value, onChange, placeholder, type = "text" }: {
  value: string; onChange: (v: string) => void; placeholder: string; type?: string;
}) {
  return (
    <input type={type} value={value} onChange={(e) => onChange(e.target.value)} placeholder={placeholder}
      className="w-full bg-black/30 border border-white/10 rounded px-1.5 py-0.5 text-sm outline-none focus:border-blue-500" />
  );
}

const FILTER_OPS: [string, string][] = [
  ["=", "="], ["!=", "≠"], [">", ">"], [">=", "≥"], ["<", "<"], ["<=", "≤"],
  ["like", "like"], ["is_null", "is null"], ["is_not_null", "not null"],
];
const opNeedsValue = (op: string) => op !== "is_null" && op !== "is_not_null";

// 篩選列：多欄複合條件（以 AND 串接；後端 build_where / Mongo filter 已支援）。
// 註：AND-only（OR 需改後端三個 build_where/filter）；Mongo 同欄多條件會覆蓋（後者勝）；
// Redis 僅 key 欄的 like/= 有效。
function FilterBar({ columns, filters, matchAny, onApply }: {
  columns: string[];
  filters: Filter[];
  matchAny: boolean;
  onApply: (filters: Filter[], matchAny: boolean) => void;
}) {
  const blank = (): Filter => ({ column: columns[0] ?? "", op: "=", value: "" });
  const [rows, setRows] = useState<Filter[]>(filters.length ? filters : [blank()]);
  const [any, setAny] = useState(matchAny);

  // 外部 filters 變更（例如清除）時同步回來。
  useEffect(() => {
    setRows(filters.length ? filters : [blank()]);
    setAny(matchAny);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [filters, matchAny]);

  const update = (i: number, patch: Partial<Filter>) =>
    setRows((rs) => rs.map((r, idx) => (idx === i ? { ...r, ...patch } : r)));
  const removeRow = (i: number) => setRows((rs) => rs.filter((_, idx) => idx !== i));
  const addRow = () => setRows((rs) => [...rs, blank()]);

  const apply = () => {
    const built = rows
      .filter((r) => r.column)
      .map((r) => ({
        column: r.column,
        op: r.op,
        value: opNeedsValue(r.op) ? r.value ?? "" : null,
      }));
    onApply(built, any);
  };
  const clear = () => { setRows([blank()]); setAny(false); onApply([], false); };

  return (
    <div className="px-2 py-1.5 bg-[#0d131a] border-b border-white/10 text-xs space-y-1.5">
      {rows.length > 1 && (
        <div className="flex items-center gap-1 text-white/40">
          <span>符合</span>
          {([["false", "全部 (AND)"], ["true", "任一 (OR)"]] as [string, string][]).map(([v, label]) => (
            <button key={v} type="button" onClick={() => setAny(v === "true")}
              className={`px-2 py-0.5 rounded border ${
                any === (v === "true") ? "border-blue-500 bg-blue-500/15 text-blue-300" : "border-white/10 text-white/50"
              }`}>
              {label}
            </button>
          ))}
          <span>條件</span>
        </div>
      )}
      {rows.map((r, i) => (
        <div key={i} className="flex items-center gap-2">
          <select value={r.column} onChange={(e) => update(i, { column: e.target.value })}
            className="bg-black/30 border border-white/10 rounded px-1.5 py-1 outline-none">
            {columns.map((c) => <option key={c} value={c}>{c}</option>)}
          </select>
          <select value={r.op} onChange={(e) => update(i, { op: e.target.value })}
            className="bg-black/30 border border-white/10 rounded px-1.5 py-1 outline-none">
            {FILTER_OPS.map(([v, label]) => <option key={v} value={v}>{label}</option>)}
          </select>
          {opNeedsValue(r.op) && (
            <input value={r.value ?? ""} onChange={(e) => update(i, { value: e.target.value })}
              onKeyDown={(e) => e.key === "Enter" && apply()}
              placeholder={r.op === "like" ? "%關鍵字%" : "值"}
              className="bg-black/30 border border-white/10 rounded px-2 py-1 outline-none focus:border-blue-500 min-w-[140px]" />
          )}
          <button onClick={() => removeRow(i)} disabled={rows.length === 1}
            title="移除此條件"
            className="px-1.5 py-1 rounded hover:bg-white/10 text-white/40 disabled:opacity-30">
            −
          </button>
        </div>
      ))}
      <div className="flex items-center gap-2 pt-0.5">
        <button onClick={addRow}
          className="px-2 py-1 rounded hover:bg-white/10 text-white/60">＋ 新增條件</button>
        <button onClick={apply}
          className="px-2 py-1 rounded bg-blue-600/80 hover:bg-blue-600">套用</button>
        <button onClick={clear}
          className="px-2 py-1 rounded hover:bg-white/10 text-white/50">清除</button>
      </div>
    </div>
  );
}

// 新增列對話框
function InsertDialog({ columns, onSubmit, onCancel, busy }: {
  columns: string[];
  onSubmit: (row: RowInsert) => void;
  onCancel: () => void;
  busy: boolean;
}) {
  // 每欄一個值；nulls 標記哪些欄留 NULL（不送出 → 走 DB 預設）
  const [values, setValues] = useState<Record<string, string>>({});
  const [nulls, setNulls] = useState<Record<string, boolean>>({});

  const submit = () => {
    const cols: string[] = [];
    const vals: (string | null)[] = [];
    for (const c of columns) {
      if (nulls[c]) { cols.push(c); vals.push(null); continue; }
      if (c in values) { cols.push(c); vals.push(values[c]); }
      // 未填且未標 NULL 的欄位略過，交由 DB 預設值處理
    }
    onSubmit({ columns: cols, values: vals });
  };

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
      <div className="bg-[#1a212b] w-[480px] max-h-[80vh] flex flex-col rounded-lg border border-white/10 shadow-2xl">
        <div className="px-5 py-3 border-b border-white/10 font-medium text-sm">新增列</div>
        <div className="p-4 space-y-2 overflow-y-auto">
          <p className="text-xs text-white/40">未填寫且未標 NULL 的欄位，交由資料庫預設值處理。</p>
          {columns.map((c) => (
            <div key={c} className="flex items-center gap-2">
              <span className="text-xs text-white/60 w-28 truncate text-right">{c}</span>
              <input
                disabled={nulls[c]}
                value={nulls[c] ? "" : values[c] ?? ""}
                onChange={(e) => setValues((v) => ({ ...v, [c]: e.target.value }))}
                className="flex-1 bg-black/30 border border-white/10 rounded px-2 py-1 text-sm outline-none focus:border-blue-500 disabled:opacity-40"
                placeholder={nulls[c] ? "NULL" : "（預設）"}
              />
              <label className="text-[10px] text-white/40 flex items-center gap-1 shrink-0">
                <input type="checkbox" checked={!!nulls[c]}
                  onChange={(e) => setNulls((n) => ({ ...n, [c]: e.target.checked }))} />
                NULL
              </label>
            </div>
          ))}
        </div>
        <div className="px-5 py-3 border-t border-white/10 flex justify-end gap-2">
          <button onClick={onCancel}
            className="px-3 py-1.5 text-sm rounded border border-white/15 hover:bg-white/5">取消</button>
          <button onClick={submit} disabled={busy}
            className="px-3 py-1.5 text-sm rounded bg-green-600 hover:bg-green-500 disabled:opacity-50">
            {busy ? "新增中…" : "新增"}
          </button>
        </div>
      </div>
    </div>
  );
}

// 儲存格編輯器：Enter 套用、Esc 取消、按鈕設為 NULL
function CellEditor({ initial, onCommit, onCancel }: {
  initial: string | null;
  onCommit: (raw: string, setNull: boolean) => void;
  onCancel: () => void;
}) {
  const [text, setText] = useState(initial ?? "");
  return (
    <span className="flex items-center gap-1">
      <input
        autoFocus
        value={text}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") onCommit(text, false);
          else if (e.key === "Escape") onCancel();
        }}
        onBlur={() => onCommit(text, false)}
        className="bg-black/50 border border-blue-500 rounded px-1 py-0.5 outline-none w-full min-w-[80px]"
      />
      <button
        onMouseDown={(e) => { e.preventDefault(); onCommit("", true); }}
        title="設為 NULL"
        className="text-[10px] text-white/40 hover:text-white/70 shrink-0"
      >
        NULL
      </button>
    </span>
  );
}

function NavBtn({ label, onClick, disabled, title }: {
  label: string; onClick: () => void; disabled?: boolean; title?: string;
}) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      title={title}
      className="w-7 h-6 flex items-center justify-center rounded hover:bg-white/10 disabled:opacity-25 disabled:hover:bg-transparent text-xs"
    >
      {label}
    </button>
  );
}

// ---- 結構分頁：欄位定義 ----
function StructurePane({ tab }: { tab: OpenTab }) {
  const kind = useStore((s) => s.connections.find((c) => c.id === tab.connId)?.kind);
  const isSql = kind === "mysql" || kind === "postgres" || kind === "sqlite";
  const [cols, setCols] = useState<ColumnInfo[] | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [nonce, setNonce] = useState(0);
  const [busy, setBusy] = useState(false);
  const [adding, setAdding] = useState(false);
  const [rename, setRename] = useState<{ col: string; to: string } | null>(null);

  useEffect(() => {
    let cancelled = false;
    setErr(null);
    api
      .tableColumns(tab.connId, tab.database, tab.table)
      .then((c) => !cancelled && setCols(c))
      .catch((e) => !cancelled && setErr(e?.message ?? "讀取失敗"));
    return () => {
      cancelled = true;
    };
  }, [tab.connId, tab.database, tab.table, nonce]);

  const doAlter = async (op: AlterOp, okMsg: string) => {
    setBusy(true);
    try {
      await api.alterTable(tab.connId, tab.database, tab.table, op);
      toast.success(okMsg);
      setAdding(false);
      setRename(null);
      setNonce((n) => n + 1);
    } catch (e: any) {
      toast.error(e?.message ?? "結構變更失敗");
    } finally {
      setBusy(false);
    }
  };

  const dropCol = async (name: string) => {
    if (!(await uiConfirm(`刪除欄位「${name}」？此動作無法復原。`, { title: "刪除欄位", danger: true, confirmText: "刪除" }))) return;
    doAlter({ op: "drop_column", name }, "欄位已刪除");
  };

  if (err) return <div className="p-3 text-red-400 text-sm mono">{err}</div>;
  if (!cols) return <div className="p-3 text-white/40 text-sm">讀取中…</div>;

  return (
    <div className="flex-1 overflow-auto">
      {isSql && (
        <div className="flex items-center gap-1 px-2 py-1 bg-[#10161e] border-b border-white/10 text-xs">
          <button type="button" onClick={() => setAdding((s) => !s)} disabled={busy}
            className="px-2 py-1 rounded hover:bg-white/10 text-white/60 disabled:opacity-40">
            ＋ 新增欄位
          </button>
          {busy && <span className="text-white/40">處理中…</span>}
        </div>
      )}
      {adding && isSql && <AddColumnForm busy={busy} onCancel={() => setAdding(false)}
        onSubmit={(op) => doAlter(op, "欄位已新增")} />}
      <table className="text-sm border-collapse w-full">
        <thead className="sticky top-0 bg-[#1a212b]">
          <tr>
            {["欄位", "型別", "可空", "鍵", "預設", "額外"].map((h) => (
              <th key={h} className="text-left px-3 py-1.5 border-b border-white/10 font-medium">
                {h}
              </th>
            ))}
            {isSql && <th className="w-20 border-b border-white/10" />}
          </tr>
        </thead>
        <tbody>
          {cols.map((c) => (
            <tr key={c.name} className="hover:bg-white/5 group">
              <td className="px-3 py-1 border-b border-white/5 font-medium">
                {rename?.col === c.name ? (
                  <input autoFocus value={rename.to}
                    onChange={(e) => setRename({ col: c.name, to: e.target.value })}
                    onKeyDown={(e) => {
                      if (e.key === "Enter" && rename.to && rename.to !== c.name)
                        doAlter({ op: "rename_column", old: c.name, new: rename.to }, "欄位已改名");
                      else if (e.key === "Escape") setRename(null);
                    }}
                    onBlur={() => setRename(null)}
                    className="bg-black/50 border border-blue-500 rounded px-1 py-0.5 outline-none" />
                ) : (
                  c.name
                )}
              </td>
              <td className="px-3 py-1 border-b border-white/5 mono text-white/70">{c.data_type}</td>
              <td className="px-3 py-1 border-b border-white/5 text-white/60">{c.nullable ? "YES" : "NO"}</td>
              <td className="px-3 py-1 border-b border-white/5">
                {c.key && (
                  <span className="text-xs px-1.5 py-0.5 rounded bg-blue-500/20 text-blue-300">{c.key}</span>
                )}
              </td>
              <td className="px-3 py-1 border-b border-white/5 mono text-white/50">
                {c.default ?? <span className="text-white/25 italic">—</span>}
              </td>
              <td className="px-3 py-1 border-b border-white/5 text-white/50 text-xs">{c.extra}</td>
              {isSql && (
                <td className="px-2 py-1 border-b border-white/5 text-right whitespace-nowrap">
                  <button type="button" title="改名" disabled={busy}
                    onClick={() => setRename({ col: c.name, to: c.name })}
                    className="px-1 text-white/20 group-hover:text-white/70 hover:bg-white/15 rounded disabled:opacity-40">✎</button>
                  <button type="button" title="刪除欄位" disabled={busy}
                    onClick={() => dropCol(c.name)}
                    className="px-1 text-white/20 group-hover:text-red-400 hover:bg-red-500/20 rounded disabled:opacity-40">−</button>
                </td>
              )}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function AddColumnForm({ onSubmit, onCancel, busy }: {
  onSubmit: (op: AlterOp) => void;
  onCancel: () => void;
  busy: boolean;
}) {
  const [name, setName] = useState("");
  const [dataType, setDataType] = useState("");
  const [nullable, setNullable] = useState(true);
  const [def, setDef] = useState("");
  const ic = "bg-black/30 border border-white/10 rounded px-2 py-1 text-sm outline-none focus:border-blue-500";
  const submit = () => {
    if (!name.trim() || !dataType.trim()) { toast.error("請填欄位名稱與型別"); return; }
    onSubmit({ op: "add_column", name: name.trim(), data_type: dataType.trim(), nullable, default: def.trim() || null });
  };
  return (
    <div className="flex flex-wrap items-end gap-2 px-3 py-2 bg-[#0d131a] border-b border-white/10 text-xs">
      <label className="block"><span className="text-white/50 block mb-0.5">欄位名稱</span>
        <input className={ic} value={name} onChange={(e) => setName(e.target.value)} /></label>
      <label className="block"><span className="text-white/50 block mb-0.5">型別</span>
        <input className={ic} value={dataType} onChange={(e) => setDataType(e.target.value)} placeholder="如 VARCHAR(50) / INT" /></label>
      <label className="block"><span className="text-white/50 block mb-0.5">預設值（選填）</span>
        <input className={ic} value={def} onChange={(e) => setDef(e.target.value)} placeholder="如 0 / 'x' / CURRENT_TIMESTAMP" /></label>
      <label className="flex items-center gap-1 pb-1.5 select-none">
        <input type="checkbox" checked={nullable} onChange={(e) => setNullable(e.target.checked)} /> 可空
      </label>
      <button type="button" onClick={submit} disabled={busy}
        className="px-3 py-1.5 rounded bg-green-600 hover:bg-green-500 disabled:opacity-50">新增</button>
      <button type="button" onClick={onCancel}
        className="px-3 py-1.5 rounded border border-white/15 hover:bg-white/5">取消</button>
    </div>
  );
}
