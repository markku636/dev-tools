import { create } from "zustand";
import { ConnectionConfig } from "./api";

// 一個開啟的表分頁
export interface OpenTab {
  key: string;        // connId:database:table 唯一鍵
  connId: string;
  database: string;
  table: string;
  view: "data" | "structure"; // 結構 / 資料 分頁
}

interface AppStore {
  // 已儲存的連線設定（持久化於磁碟，密碼存 OS keychain；啟動時載入清單）
  connections: ConnectionConfig[];
  // 目前已開啟連線的 id 集合
  connectedIds: Set<string>;
  // 當前選取的連線
  activeId: string | null;
  // 已開啟的表分頁
  tabs: OpenTab[];
  activeTabKey: string | null;

  setConnections: (cs: ConnectionConfig[]) => void;
  addConnection: (c: ConnectionConfig) => void;
  removeConnection: (id: string) => void;
  setActive: (id: string | null) => void;
  markConnected: (id: string) => void;
  markDisconnected: (id: string) => void;

  openTable: (connId: string, database: string, table: string) => void;
  closeTab: (key: string) => void;
  setActiveTab: (key: string) => void;
  setTabView: (key: string, view: "data" | "structure") => void;
}

export const useStore = create<AppStore>((set) => ({
  connections: [],
  connectedIds: new Set(),
  activeId: null,
  tabs: [],
  activeTabKey: null,

  setConnections: (cs) => set({ connections: cs }),
  addConnection: (c) =>
    set((s) => ({ connections: [...s.connections.filter((x) => x.id !== c.id), c] })),
  removeConnection: (id) =>
    set((s) => ({
      connections: s.connections.filter((c) => c.id !== id),
      activeId: s.activeId === id ? null : s.activeId,
    })),
  setActive: (id) => set({ activeId: id }),
  markConnected: (id) =>
    set((s) => ({ connectedIds: new Set(s.connectedIds).add(id) })),
  markDisconnected: (id) =>
    set((s) => {
      const next = new Set(s.connectedIds);
      next.delete(id);
      // 同時關閉該連線底下所有分頁
      const tabs = s.tabs.filter((t) => t.connId !== id);
      return {
        connectedIds: next,
        tabs,
        activeTabKey: tabs.some((t) => t.key === s.activeTabKey)
          ? s.activeTabKey
          : tabs.length ? tabs[tabs.length - 1].key : null,
      };
    }),

  openTable: (connId, database, table) =>
    set((s) => {
      const key = `${connId}:${database}:${table}`;
      if (s.tabs.some((t) => t.key === key)) {
        return { activeTabKey: key };
      }
      const tab: OpenTab = { key, connId, database, table, view: "data" };
      return { tabs: [...s.tabs, tab], activeTabKey: key };
    }),
  closeTab: (key) =>
    set((s) => {
      const tabs = s.tabs.filter((t) => t.key !== key);
      return {
        tabs,
        activeTabKey:
          s.activeTabKey === key ? tabs.length ? tabs[tabs.length - 1].key : null : s.activeTabKey,
      };
    }),
  setActiveTab: (key) => set({ activeTabKey: key }),
  setTabView: (key, view) =>
    set((s) => ({
      tabs: s.tabs.map((t) => (t.key === key ? { ...t, view } : t)),
    })),
}));
