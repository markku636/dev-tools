// 連線色標（致敬 Navicat 的 connection color）：給連線標上顏色以區分 正式 / 測試 / 開發 等環境。
// 純前端、localStorage 持久化（與後端 ConnectionConfig 解耦，免動 keychain / 加密匯出）。

export const CONN_COLORS_KEY = "db-kit:connColors";

// 色盤（含「無」＝清除）。值為 CSS 顏色；空字串代表不標色。
export const CONN_COLOR_PALETTE: { name: string; value: string }[] = [
  { name: "無", value: "" },
  { name: "紅", value: "#ef4444" },
  { name: "橙", value: "#f59e0b" },
  { name: "黃", value: "#eab308" },
  { name: "綠", value: "#22c55e" },
  { name: "青", value: "#06b6d4" },
  { name: "藍", value: "#3b82f6" },
  { name: "紫", value: "#8b5cf6" },
  { name: "粉", value: "#ec4899" },
  { name: "灰", value: "#64748b" },
];

export type ConnColorMap = Record<string, string>;

// 純函式：設定 / 清除某連線的顏色（空字串＝清除），回傳新 map。供單元測試。
export function setConnColor(map: ConnColorMap, id: string, color: string): ConnColorMap {
  const next = { ...map };
  if (color) next[id] = color;
  else delete next[id];
  return next;
}

export function loadConnColors(): ConnColorMap {
  try {
    const obj = JSON.parse(localStorage.getItem(CONN_COLORS_KEY) || "{}");
    if (obj && typeof obj === "object" && !Array.isArray(obj)) {
      const out: ConnColorMap = {};
      for (const [k, v] of Object.entries(obj)) if (typeof v === "string" && v) out[k] = v;
      return out;
    }
  } catch {
    /* 忽略損毀的存檔 */
  }
  return {};
}

export function persistConnColors(map: ConnColorMap) {
  try {
    localStorage.setItem(CONN_COLORS_KEY, JSON.stringify(map));
  } catch {
    /* 忽略寫入失敗 */
  }
}
