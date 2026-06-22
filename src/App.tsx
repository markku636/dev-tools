import { useEffect, useState } from "react";
import { api, ConnectionConfig, KIND_META, QueryResult, TableInfo } from "./api";
import { useStore } from "./store";
import ConnectionDialog from "./ConnectionDialog";
import TableView from "./TableView";
import BackupDialog from "./BackupDialog";
import { toast, uiConfirm, UiHost } from "./ui";

export default function App() {
  // null = 關閉；{ initial } = 開啟（initial 為 null 表新增、為連線表示編輯）
  const [dialog, setDialog] = useState<{ initial: ConnectionConfig | null } | null>(null);
  const [backupOpen, setBackupOpen] = useState(false);
  const { connections, connectedIds, activeId } = useStore();
  const activeConn = connections.find((c) => c.id === activeId) ?? null;

  // 啟動時載入已存連線清單（僅清單，不自動連線；密碼留在 keychain）。
  useEffect(() => {
    api
      .listSavedConnections()
      .then((saved) =>
        useStore
          .getState()
          .setConnections(saved.map((c) => ({ ...c, password: c.password ?? "" })))
      )
      .catch(() => {});
  }, []);

  return (
    <div className="h-full flex flex-col">
      <Toolbar
        onNewConnection={() => setDialog({ initial: null })}
        onBackup={() => activeConn && setBackupOpen(true)}
        canBackup={!!activeConn}
      />
      <div className="flex-1 flex min-h-0">
        <Sidebar onEdit={(c) => setDialog({ initial: c })} />
        <MainArea />
      </div>
      <StatusBar />
      {dialog && (
        <ConnectionDialog
          initial={dialog.initial}
          onClose={() => setDialog(null)}
          onSaved={async (c) => {
            try {
              await api.saveConnection(c);
              toast.success(dialog.initial ? "連線已更新" : "連線已儲存");
            } catch (e: any) {
              toast.error(e?.message ?? "儲存連線失敗");
            }
            useStore.getState().addConnection(c);
            useStore.getState().setActive(c.id);
            setDialog(null);
          }}
        />
      )}
      {backupOpen && activeConn && (
        <BackupDialog
          conn={activeConn}
          database={null}
          onClose={() => setBackupOpen(false)}
        />
      )}
      <UiHost />
    </div>
  );

  function StatusBar() {
    const active = connections.find((c) => c.id === activeId);
    return (
      <div className="h-7 bg-[#11161d] border-t border-white/10 px-3 flex items-center text-xs text-white/40 gap-4">
        <span>at-kit</span>
        {active && (
          <span>
            {KIND_META[active.kind].label} · {active.host}:{active.port}
            {connectedIds.has(active.id) ? " · 已連線" : " · 未連線"}
          </span>
        )}
      </div>
    );
  }
}

// ---- 上方大圖示工具列（Navicat 風格識別特徵）----
function Toolbar({ onNewConnection, onBackup, canBackup }: {
  onNewConnection: () => void;
  onBackup: () => void;
  canBackup: boolean;
}) {
  const tools = [
    { icon: "🔌", label: "連線", onClick: onNewConnection, disabled: false },
    { icon: "▦", label: "表", disabled: true, onClick: () => {} },
    { icon: "⌗", label: "查詢", disabled: true, onClick: () => {} },
    { icon: "💾", label: "備份", onClick: onBackup, disabled: !canBackup },
  ];
  return (
    <div className="h-16 bg-[#161c25] border-b border-white/10 flex items-center px-3 gap-1">
      <div className="font-semibold text-white/90 mr-4 pl-1">at-kit</div>
      {tools.map((t) => (
        <button
          key={t.label}
          onClick={t.onClick}
          disabled={t.disabled}
          className="w-16 h-12 flex flex-col items-center justify-center rounded hover:bg-white/5 disabled:opacity-30 disabled:hover:bg-transparent"
        >
          <span className="text-lg leading-none">{t.icon}</span>
          <span className="text-[11px] text-white/60 mt-1">{t.label}</span>
        </button>
      ))}
    </div>
  );
}

// ---- 左側連線/物件樹 ----
function Sidebar({ onEdit }: { onEdit: (c: ConnectionConfig) => void }) {
  const { connections, connectedIds, activeId, setActive } = useStore();
  const [databases, setDatabases] = useState<Record<string, string[]>>({});
  // 已展開的資料庫: 鍵為 connId:db
  const [expandedDbs, setExpandedDbs] = useState<Record<string, TableInfo[]>>({});
  // 連線中（顯示 loading）的 id 集合
  const [connecting, setConnecting] = useState<Set<string>>(new Set());
  // 右鍵選單
  const [menu, setMenu] = useState<{ id: string; x: number; y: number } | null>(null);

  const setBusy = (id: string, on: boolean) =>
    setConnecting((s) => {
      const n = new Set(s);
      if (on) n.add(id);
      else n.delete(id);
      return n;
    });

  const doConnect = async (id: string) => {
    const cfg = connections.find((c) => c.id === id);
    if (!cfg || connectedIds.has(id) || connecting.has(id)) return;
    setBusy(id, true);
    try {
      await api.connect(cfg);
      useStore.getState().markConnected(id);
      const dbs = await api.listDatabases(id);
      setDatabases((d) => ({ ...d, [id]: dbs }));
    } catch (e: any) {
      toast.error(e?.message ?? "連線失敗");
    } finally {
      setBusy(id, false);
    }
  };

  const doDisconnect = async (id: string) => {
    await api.disconnect(id);
    useStore.getState().markDisconnected(id);
    setDatabases((d) => ({ ...d, [id]: [] }));
    setExpandedDbs((e) => {
      const n = { ...e };
      Object.keys(n)
        .filter((k) => k.startsWith(`${id}:`))
        .forEach((k) => delete n[k]);
      return n;
    });
  };

  const toggleConnect = (id: string) =>
    connectedIds.has(id) ? doDisconnect(id) : doConnect(id);

  const refreshDbs = async (id: string) => {
    if (!connectedIds.has(id)) return;
    try {
      const dbs = await api.listDatabases(id);
      setDatabases((d) => ({ ...d, [id]: dbs }));
      toast.success("已重新整理");
    } catch (e: any) {
      toast.error(e?.message ?? "重新整理失敗");
    }
  };

  const deleteConn = async (id: string, name: string) => {
    const ok = await uiConfirm(`刪除連線「${name}」？此動作會一併移除已儲存的密碼。`, {
      title: "刪除連線",
      danger: true,
      confirmText: "刪除",
    });
    if (!ok) return;
    try {
      await api.removeSavedConnection(id);
    } catch {
      // 即使後端清理失敗，仍從前端移除
    }
    useStore.getState().markDisconnected(id);
    useStore.getState().removeConnection(id);
    toast.success("連線已刪除");
  };

  const toggleDb = async (connId: string, db: string) => {
    const key = `${connId}:${db}`;
    if (expandedDbs[key]) {
      setExpandedDbs((e) => {
        const next = { ...e };
        delete next[key];
        return next;
      });
      return;
    }
    try {
      const tables = await api.listTables(connId, db);
      setExpandedDbs((e) => ({ ...e, [key]: tables }));
    } catch (e: any) {
      toast.error(e?.message ?? "讀取表失敗");
    }
  };

  const menuConn = menu ? connections.find((x) => x.id === menu.id) ?? null : null;

  return (
    <div className="w-64 bg-[#131922] border-r border-white/10 overflow-y-auto text-sm">
      {connections.length === 0 && (
        <div className="p-4 text-white/30 text-xs leading-relaxed">
          尚無連線。點上方「連線」新增一個，雙擊以建立連線（右鍵有更多選項）。
        </div>
      )}
      {connections.map((c) => {
        const meta = KIND_META[c.kind];
        const connected = connectedIds.has(c.id);
        const busy = connecting.has(c.id);
        return (
          <div key={c.id}>
            <div
              onClick={() => setActive(c.id)}
              onDoubleClick={() => toggleConnect(c.id)}
              onContextMenu={(e) => {
                e.preventDefault();
                setActive(c.id);
                setMenu({ id: c.id, x: e.clientX, y: e.clientY });
              }}
              className={`group flex items-center gap-2 px-3 py-1.5 cursor-pointer ${
                activeId === c.id ? "bg-white/10" : "hover:bg-white/5"
              }`}
            >
              {busy ? (
                <span className="w-2 h-2 shrink-0 rounded-full border border-white/50 border-t-transparent animate-spin" />
              ) : (
                <span
                  className="w-2 h-2 rounded-full shrink-0"
                  style={{ background: connected ? meta.color : "transparent", border: `1px solid ${meta.color}` }}
                />
              )}
              <span className="truncate flex-1">{c.name}</span>
              <button type="button" title="編輯連線"
                onClick={(e) => { e.stopPropagation(); onEdit(c); }}
                className="w-4 h-4 shrink-0 items-center justify-center rounded text-white/30 hover:bg-white/15 hover:text-white/80 hidden group-hover:flex">
                ✎
              </button>
              <button type="button" title="刪除連線"
                onClick={(e) => { e.stopPropagation(); deleteConn(c.id, c.name); }}
                className="w-4 h-4 shrink-0 items-center justify-center rounded text-white/30 hover:bg-white/15 hover:text-red-300 hidden group-hover:flex">
                ×
              </button>
            </div>
            {connected &&
              (databases[c.id] ?? []).map((db) => {
                const dbKey = `${c.id}:${db}`;
                const tables = expandedDbs[dbKey];
                return (
                  <div key={db}>
                    <div
                      onClick={() => toggleDb(c.id, db)}
                      className="pl-7 pr-3 py-1 text-white/70 hover:bg-white/5 cursor-pointer truncate flex items-center gap-1"
                    >
                      <span className="text-white/30 text-[10px] w-3">{tables ? "▼" : "▶"}</span>
                      <span className="truncate">{db}</span>
                    </div>
                    {tables &&
                      tables.map((t) => (
                        <div
                          key={t.name}
                          onDoubleClick={() =>
                            useStore.getState().openTable(c.id, db, t.name)
                          }
                          className="pl-12 pr-3 py-1 text-white/55 hover:bg-white/5 cursor-pointer truncate flex items-center gap-1.5"
                          title="雙擊開啟"
                        >
                          <span className="text-[10px]">{t.kind === "view" ? "◫" : "▦"}</span>
                          <span className="truncate">{t.name}</span>
                        </div>
                      ))}
                    {tables && tables.length === 0 && (
                      <div className="pl-12 pr-3 py-1 text-white/25 text-xs">無表</div>
                    )}
                  </div>
                );
              })}
          </div>
        );
      })}

      {menu && menuConn && (
        <>
          <div className="fixed inset-0 z-[89]"
            onClick={() => setMenu(null)}
            onContextMenu={(e) => { e.preventDefault(); setMenu(null); }} />
          <div className="fixed z-[90] min-w-[150px] bg-[#1a212b] border border-white/10 rounded shadow-2xl py-1 text-sm"
            style={{ left: menu.x, top: menu.y }}>
            {(
              [
                [connectedIds.has(menu.id) ? "中斷連線" : "連線", () => toggleConnect(menu.id), false],
                ...(connectedIds.has(menu.id)
                  ? [["重新整理資料庫", () => refreshDbs(menu.id), false] as [string, () => void, boolean]]
                  : []),
                ["編輯…", () => onEdit(menuConn), false],
                ["刪除", () => deleteConn(menuConn.id, menuConn.name), true],
              ] as [string, () => void, boolean][]
            ).map(([label, fn, danger]) => (
              <button key={label} type="button"
                onClick={() => { setMenu(null); fn(); }}
                className={`block w-full text-left px-3 py-1.5 hover:bg-white/10 ${danger ? "text-red-300" : "text-white/80"}`}>
                {label}
              </button>
            ))}
          </div>
        </>
      )}
    </div>
  );
}

// ---- 中央主工作區：分頁式（表分頁 + 查詢） ----
function MainArea() {
  const { activeId, connectedIds, tabs, activeTabKey, setActiveTab, closeTab } = useStore();

  const canUse = activeId && connectedIds.has(activeId);
  const activeTab = tabs.find((t) => t.key === activeTabKey) ?? null;

  if (!canUse && tabs.length === 0) {
    return (
      <div className="flex-1 flex items-center justify-center text-white/25 text-sm">
        雙擊左側連線以建立連線，再雙擊表即可開啟。
      </div>
    );
  }

  return (
    <div className="flex-1 flex flex-col min-w-0">
      {/* 分頁列 */}
      <div className="flex items-stretch bg-[#11161d] border-b border-white/10 overflow-x-auto">
        {tabs.map((t) => (
          <div
            key={t.key}
            onClick={() => setActiveTab(t.key)}
            className={`flex items-center gap-2 pl-3 pr-2 py-1.5 text-xs border-r border-white/10 cursor-pointer whitespace-nowrap ${
              t.key === activeTabKey ? "bg-[#0f1419] text-white" : "text-white/50 hover:bg-white/5"
            }`}
          >
            <span className="mono">{t.table}</span>
            <button
              onClick={(e) => {
                e.stopPropagation();
                closeTab(t.key);
              }}
              className="w-4 h-4 flex items-center justify-center rounded hover:bg-white/15 text-white/40"
            >
              ×
            </button>
          </div>
        ))}
        <QueryTabButton />
      </div>

      {/* 內容 */}
      {activeTab ? (
        <TableView tab={activeTab} />
      ) : (
        <QueryPane />
      )}
    </div>
  );
}

function QueryTabButton() {
  const { activeTabKey, setActiveTab } = useStore();
  return (
    <button
      onClick={() => setActiveTab("__query__" as any)}
      className={`px-3 py-1.5 text-xs border-r border-white/10 ${
        activeTabKey === "__query__" ? "bg-[#0f1419] text-white" : "text-white/50 hover:bg-white/5"
      }`}
    >
      ⌗ 查詢
    </button>
  );
}

// ---- 查詢面板：上 SQL、下結果（F6 執行） ----
function QueryPane() {
  const { activeId } = useStore();
  const [sql, setSql] = useState("SELECT 1");
  const [result, setResult] = useState<QueryResult | null>(null);
  const [err, setErr] = useState<string | null>(null);

  const run = async () => {
    if (!activeId) return;
    setErr(null);
    try {
      setResult(await api.runQuery(activeId, sql));
    } catch (e: any) {
      setErr(e?.message ?? "查詢失敗");
      setResult(null);
    }
  };

  if (!activeId) {
    return (
      <div className="flex-1 flex items-center justify-center text-white/25 text-sm">
        請先選取一個已連線的連線。
      </div>
    );
  }

  return (
    <div className="flex-1 flex flex-col min-w-0">
      <div className="border-b border-white/10">
        <div className="flex items-center justify-between px-3 py-1.5 bg-[#161c25]">
          <span className="text-xs text-white/40">查詢</span>
          <button onClick={run}
            className="text-xs px-2 py-1 rounded bg-green-600/80 hover:bg-green-600">
            ▶ 執行 (F6)
          </button>
        </div>
        <textarea
          className="w-full h-40 bg-[#0f1419] p-3 outline-none mono text-sm resize-none"
          value={sql}
          onChange={(e) => setSql(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "F6") {
              e.preventDefault();
              run();
            }
          }}
        />
      </div>
      <div className="flex-1 overflow-auto">
        {err && <div className="p-3 text-red-400 text-sm mono">{err}</div>}
        {result && <ResultTable result={result} />}
      </div>
    </div>
  );
}

function ResultTable({ result }: { result: QueryResult }) {
  if (result.columns.length === 0) {
    return (
      <div className="p-3 text-white/50 text-sm">
        影響列數：{result.rows_affected}
      </div>
    );
  }
  return (
    <table className="text-sm border-collapse w-full">
      <thead className="sticky top-0 bg-[#1a212b]">
        <tr>
          {result.columns.map((c) => (
            <th key={c} className="text-left px-3 py-1.5 border-b border-white/10 font-medium">
              {c}
            </th>
          ))}
        </tr>
      </thead>
      <tbody className="mono">
        {result.rows.map((row, i) => (
          <tr key={i} className="hover:bg-white/5">
            {row.map((cell, j) => (
              <td key={j} className="px-3 py-1 border-b border-white/5">
                {cell === null ? <span className="text-white/30 italic">NULL</span> : cell}
              </td>
            ))}
          </tr>
        ))}
      </tbody>
    </table>
  );
}
