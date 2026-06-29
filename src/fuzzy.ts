// 模糊比對評分（供命令面板排序）。純函式、無相依，可單元測試。
// 規則：查詢字元需依序在目標中出現（子序列）；連續匹配 / 靠前 / 完整子字串加分，名稱越短越優先。

export function fuzzyScore(query: string, text: string): number | null {
  const q = query.toLowerCase().trim();
  const t = text.toLowerCase();
  if (!q) return 0; // 空查詢：全部視為相符（score 0）。
  let ti = 0;
  let score = 0;
  let streak = 0;
  for (const ch of q) {
    const idx = t.indexOf(ch, ti);
    if (idx === -1) return null; // 非子序列 → 不相符。
    streak = idx === ti ? streak + 1 : 0;
    score += 1 + streak * 2 + (idx === 0 ? 3 : 0);
    ti = idx + 1;
  }
  if (t.includes(q)) score += 5; // 完整子字串額外加分。
  return score - t.length * 0.01; // 長度輕微懲罰，使較短名稱排前。
}

// 對一組項目以 keyOf 取文字評分、過濾不相符、依分數（高→低）排序，回傳排序後的項目。
export function fuzzyFilter<T>(query: string, items: T[], keyOf: (x: T) => string, limit = 50): T[] {
  const scored: { item: T; score: number }[] = [];
  for (const item of items) {
    const s = fuzzyScore(query, keyOf(item));
    if (s !== null) scored.push({ item, score: s });
  }
  scored.sort((a, b) => b.score - a.score);
  return scored.slice(0, limit).map((x) => x.item);
}
