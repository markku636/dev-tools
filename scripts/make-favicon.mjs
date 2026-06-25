// 由品牌標誌產生 (1) 網頁 favicon 與 (2) 桌面 App 圖示的正方形來源。
//
// 來源 src/assets/logo-mark.png 是「海豹吉祥物 + 火焰 + MAGIDB CONNECT 字標」
// 的寬版插畫（1536x1024）。圖示需要正方形且在 16~32px 仍可辨識，因此只取
// 「海豹吉祥物」這個品牌角色。
//   - favicon：置中 contain 進正方形（透明邊），輸出到 public/。
//   - App 圖示來源：再留一圈邊距輸出 src-tauri/app-icon.png（1024），
//     交給 `npm run tauri icon` 產生 src-tauri/icons/ 全套平台圖示。
//
// 重新產生：  node scripts/make-favicon.mjs   （之後若改了 App 圖示，需再跑
//             `npm run tauri icon src-tauri/app-icon.png`）
import sharp from "sharp";
import pngToIco from "png-to-ico";
import { mkdir, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const SRC = resolve(root, "src/assets/logo-mark.png");
const OUT = resolve(root, "public");
const APP_ICON = resolve(root, "src-tauri/app-icon.png");

// 海豹吉祥物在原圖中的範圍（由 alpha 內容分布量測）。左界取 535 以排除左下角
// 魔術帽邊緣（帽緣右界 x≈530，僅出現在 y≥540），僅犧牲極細的鬍鬚尖端。
const CROP = { left: 533, top: 4, width: 577, height: 655 };

// 輸出尺寸：.ico 內含的多解析度 + apple-touch + 給 <link> 用的 png。
const ICO_SIZES = [16, 32, 48];
const PNG_SIZES = [32, 180];

const TRANSPARENT = { r: 0, g: 0, b: 0, alpha: 0 };

async function squareMark(size) {
  // 取吉祥物 → contain 進正方形（透明邊），高品質縮放。
  return sharp(SRC)
    .extract(CROP)
    .resize(size, size, { fit: "contain", background: TRANSPARENT })
    .png()
    .toBuffer();
}

// App 圖示：吉祥物縮到 scale 比例後置中於透明正方形，四周留邊，
// 避免帽頂 / 尾鰭貼齊圖示邊緣（桌面 / 工作列 / 安裝檔較好看）。
async function paddedMark(size, scale = 0.86) {
  const inner = Math.round(size * scale);
  const mark = await squareMark(inner);
  return sharp({
    create: { width: size, height: size, channels: 4, background: TRANSPARENT },
  })
    .composite([{ input: mark, gravity: "center" }])
    .png()
    .toBuffer();
}

await mkdir(OUT, { recursive: true });

// 1) favicon.ico（多解析度，給瀏覽器分頁與舊環境最佳相容）
const icoBuffers = await Promise.all(ICO_SIZES.map((s) => squareMark(s)));
await writeFile(resolve(OUT, "favicon.ico"), await pngToIco(icoBuffers));

// 2) png（現代瀏覽器與 apple-touch-icon）
for (const s of PNG_SIZES) {
  const name = s === 180 ? "apple-touch-icon.png" : `favicon-${s}.png`;
  await writeFile(resolve(OUT, name), await squareMark(s));
}

// 3) App 圖示來源（1024 正方、留邊）→ 供 `tauri icon` 使用
await writeFile(APP_ICON, await paddedMark(1024));

console.log("favicon 產生完成 →", OUT);
console.log("  favicon.ico (16/32/48)、favicon-32.png、apple-touch-icon.png");
console.log("App 圖示來源 →", APP_ICON, "（接著跑 npm run tauri icon src-tauri/app-icon.png）");
