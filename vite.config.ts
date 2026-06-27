import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

// 版本號單一事實來源：讀 package.json（打包腳本 build-installer.ps1 會把
// package.json / tauri.conf.json / Cargo.toml 三者同步），於建置期注入成 __APP_VERSION__。
const pkg = JSON.parse(
  readFileSync(fileURLToPath(new URL("./package.json", import.meta.url)), "utf-8"),
) as { version: string };

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  define: {
    __APP_VERSION__: JSON.stringify(pkg.version),
  },
});
