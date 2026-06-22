# at-kit

一個跨平台的統一資料庫管理工具，以單一 Navicat 風格介面管理 **MySQL · PostgreSQL · MongoDB · Redis**。

採用 **Tauri (Rust) + React + TypeScript**，輕量、高效、安全。

> 目前進度：**五大資料庫全部可連線**；關聯式完整 CRUD + 多欄複合篩選排序、MongoDB 文件攤平、Redis 五種結構**檢視＋編輯**
> 連線設定持久化（密碼存 OS keychain）、SSH Tunnel、排程備份 + 備份歷史、欄寬可拖曳
> 可實際連線：**MySQL · PostgreSQL · SQLite · MongoDB · Redis**

完整規劃文件見 [`docs/`](./docs/)：[規劃](./docs/planning.md) · [架構](./docs/architecture.md) · [連線生命週期](./docs/connection-lifecycle.md) · [Navicat 操作習慣](./docs/navicat-ux.md) · [路線圖](./docs/roadmap.md)。變更紀錄見 [CHANGELOG](./CHANGELOG.md)。

## 功能藍圖

- [x] P0 骨架：佈局、大圖示工具列、連線池與釋放層、MySQL 連線
- [x] P1/P2 雙擊開表 → 表格檢視 + 底部分頁導覽 + 結構/資料分頁切換
- [x] SQLite 支援（檔案型，與 MySQL 共用 trait）
- [x] PostgreSQL 支援
- [x] 儲存格直接編輯 + ✓ 套用（以主鍵定位，寫回 DB）
- [x] 新增列 / 刪除列（完整 CRUD）
- [x] 篩選（單欄條件）、排序（點欄位標題）
- [x] MongoDB（文件攤平成表格，沿用表格手感）
- [x] Redis（key 列表化 + 五種結構檢視）
- [x] 備份 / 還原（手動，CLI 為主 + SQLite 檔案複製）
- [x] 連線設定持久化 + 密碼 OS keychain 加密
- [x] SSH Tunnel（密碼 / 私鑰認證）
- [x] 排程備份 + 備份歷史管理（清單 / 立即執行 / 還原 / 保留份數）
- [x] Redis 結構編輯（List / Set / ZSet / Hash 元素增刪改）、多欄複合篩選、欄寬調整
- [x] 多欄篩選 AND / OR 條件切換
- [x] 資料匯出（CSV / TSV / JSON / SQL INSERT / Markdown，含標題 / 分隔字元 / NULL / BOM / 範圍選項）
- [x] 操作體驗：原生檔案選擇器、編輯連線、Toast 通知、連線樹右鍵選單
- [ ] Redis host key 驗證（TOFU）
- [ ] ER 圖、查詢效能分析、結構編輯（DDL）

## 技術棧

| 層 | 技術 |
|----|------|
| 應用框架 | Tauri 2 |
| 前端 | React 18 + TypeScript + Vite |
| UI | Tailwind CSS（shadcn/ui 風格） |
| 狀態 | Zustand |
| 後端 | Rust：sqlx (MySQL / PostgreSQL / SQLite)、mongodb、redis |

## 連線生命週期設計

連線釋放是此類工具最易出錯處，本專案在 P0 即建立：

- **連線池**：sqlx 內建 pool，設定 `max_connections` / `idle_timeout` / `max_lifetime`
- **健康檢查**：取得連線前以 `SELECT 1` 驗證殭屍連線
- **優雅關閉**：應用結束時 drain 所有連線池（RAII / Drop 保底）
- **連線數監控**：每連線回報 in-use / idle 狀態

## 開發環境需求

- [Rust](https://rustup.rs/) (stable)
- [Node.js](https://nodejs.org/) 18+
- Tauri 系統依賴：見 <https://tauri.app/start/prerequisites/>

## 開始開發

```bash
# 安裝前端依賴
npm install

# 開發模式（同時起前端與 Tauri）
npm run tauri dev

# 打包
npm run tauri build
```

### 測試 MySQL 連線

啟動後在 UI 左上點「新增連線」，填入 MySQL 連線資訊並測試。
或可先用 Docker 起一個測試用 MySQL：

```bash
docker run --name mysql-test -e MYSQL_ROOT_PASSWORD=test1234 -p 3306:3306 -d mysql:8
```

## 打包 Windows 安裝檔

在 **Windows** 上執行隨附的 PowerShell 腳本，它會自動檢查並安裝 Rust 與 Node.js，然後產出 `.msi` 與 `.exe` 安裝檔：

```powershell
powershell -ExecutionPolicy Bypass -File .\build-installer.ps1
```

完成後安裝檔位於 `src-tauri\target\release\bundle\`（`msi\` 與 `nsis\` 子目錄）。

> 注意：Tauri 需要 WebView2 Runtime（Windows 11 內建，Windows 10 多數已有）。

## 授權

[MIT](./LICENSE)
