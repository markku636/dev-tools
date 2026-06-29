import { describe, it, expect } from "vitest";
import { fuzzyScore, fuzzyFilter } from "./fuzzy";

describe("fuzzyScore", () => {
  it("空查詢 → 0（全相符）", () => {
    expect(fuzzyScore("", "anything")).toBe(0);
  });
  it("子序列相符回傳分數、非子序列回 null", () => {
    expect(fuzzyScore("ord", "orders")).not.toBeNull();
    expect(fuzzyScore("odr", "orders")).not.toBeNull(); // o,d,r 依序存在
    expect(fuzzyScore("xyz", "orders")).toBeNull();
    expect(fuzzyScore("ordersx", "orders")).toBeNull(); // 多出的字元無法匹配
  });
  it("完整子字串 / 開頭匹配分數較高", () => {
    // "user" 對 "users"（含子字串、開頭）應高於對 "purchase_orders"（散落匹配）。
    const a = fuzzyScore("user", "users")!;
    const b = fuzzyScore("user", "purchase_orders");
    expect(b === null || a > b).toBe(true);
  });
  it("較短目標在同樣匹配下排前（長度懲罰）", () => {
    expect(fuzzyScore("ab", "ab")!).toBeGreaterThan(fuzzyScore("ab", "abxxxxxx")!);
  });
});

describe("fuzzyFilter", () => {
  it("過濾不相符並依分數排序", () => {
    const items = ["orders", "order_items", "users", "products"];
    const out = fuzzyFilter("ord", items, (x) => x);
    expect(out).toContain("orders");
    expect(out).toContain("order_items");
    expect(out).not.toContain("users");
    // 較短且開頭相符的 "orders" 應排在 "order_items" 之前。
    expect(out.indexOf("orders")).toBeLessThan(out.indexOf("order_items"));
  });
  it("limit 限制回傳數量", () => {
    const items = Array.from({ length: 100 }, (_, i) => `t${i}`);
    expect(fuzzyFilter("t", items, (x) => x, 10)).toHaveLength(10);
  });
});
