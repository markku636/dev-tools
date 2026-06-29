import { describe, it, expect, beforeEach } from "vitest";
import { setConnColor, loadConnColors, persistConnColors, CONN_COLORS_KEY, type ConnColorMap } from "./connColors";

// node 測試環境無 localStorage，提供最小記憶體實作。
const __mem: Record<string, string> = {};
globalThis.localStorage = {
  getItem: (k: string) => (k in __mem ? __mem[k] : null),
  setItem: (k: string, v: string) => { __mem[k] = String(v); },
  removeItem: (k: string) => { delete __mem[k]; },
  clear: () => { for (const k of Object.keys(__mem)) delete __mem[k]; },
  key: () => null,
  length: 0,
} as unknown as Storage;

describe("連線色標（connColors）", () => {
  beforeEach(() => localStorage.removeItem(CONN_COLORS_KEY));

  it("setConnColor：純函式、設定與清除（空字串移除）、不改原 map", () => {
    const a: ConnColorMap = {};
    const b = setConnColor(a, "c1", "#ef4444");
    expect(b).toEqual({ c1: "#ef4444" });
    expect(a).toEqual({}); // 不應改動原 map
    const c = setConnColor(b, "c2", "#3b82f6");
    expect(c).toEqual({ c1: "#ef4444", c2: "#3b82f6" });
    // 空字串 → 清除該鍵。
    expect(setConnColor(c, "c1", "")).toEqual({ c2: "#3b82f6" });
  });

  it("persist / load 往返；過濾非字串 / 空值 / 損毀存檔", () => {
    persistConnColors({ c1: "#22c55e", c2: "#8b5cf6" });
    expect(loadConnColors()).toEqual({ c1: "#22c55e", c2: "#8b5cf6" });
    // 非物件 / 損毀 → 空 map。
    localStorage.setItem(CONN_COLORS_KEY, "[1,2,3]");
    expect(loadConnColors()).toEqual({});
    localStorage.setItem(CONN_COLORS_KEY, "{not json");
    expect(loadConnColors()).toEqual({});
    // 過濾非字串 / 空字串值。
    localStorage.setItem(CONN_COLORS_KEY, JSON.stringify({ a: "#fff", b: 5, c: "", d: null }));
    expect(loadConnColors()).toEqual({ a: "#fff" });
  });
});
