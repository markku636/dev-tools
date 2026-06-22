# Changelog

## 資料匯出 + 多欄 OR 篩選（本次）

### 資料匯出（Navicat 風格多格式，`export.rs`）
- 新增 `export_table` command：尊重目前的篩選 / 排序 / AND·OR，將表格資料匯出成 **CSV / TSV / JSON / SQL INSERT / Markdown**。
- 選項：含/不含欄位標題、自訂分隔字元、NULL 呈現字串、UTF-8 BOM（方便 Excel 開 CSV）、SQL 目標表名、**匯出全部符合列 vs 只匯目前頁**。
- 逐頁（每批 2000 列）向 driver 取資料，安全上限 100 萬列；CSV 欄位/SQL 值/Markdown 儲存格皆做跳脫。
- 前端 `ExportDialog.tsx`：資料分頁「⬇ 匯出」鈕開啟，格式 + 選項 + 原生「另存」對話框；完成以 toast 回報列數/大小/路徑。

### 多欄篩選 AND / OR
- `DataQuery` 加 `match_any`；四個 driver 的 `build_where`（MySQL / PostgreSQL / SQLite）與 Mongo `build_filter` 依此以 `AND`/`OR`（Mongo `$or`）串接。
- `FilterBar` 在多條件時顯示「全部 (AND) / 任一 (OR)」切換。

---

## 持久化 · SSH Tunnel · 排程備份 · UX 套組（本次）

依相依性順序一次補完四大功能。

### 操作體驗優化（UX）
- **原生檔案選擇器**（接 `tauri-plugin-dialog`）：備份輸出/還原來源路徑、排程備份目錄、SQLite 檔案、SSH 私鑰路徑都加「瀏覽…」，免手打路徑（`capabilities/default.json` 加 `dialog:default`）。
- **編輯現有連線**：連線設定持久化後可重新編輯——連線對話框支援帶入既有設定（保留 id、標題改「編輯連線」），側欄 hover 出現 ✎/× 與右鍵選單。`save_connection` 改為密碼留空＝不變更（編輯時不必重打密碼，也不會誤刪 keychain）。
- **Toast 通知 + 樣式化確認框**（`ui.tsx`）：取代瀏覽器 `alert()`/`confirm()`；右下角滑出通知，刪除/還原等破壞性操作走統一的紅色確認框。
- **連線樹體驗**：右鍵選單（連線/中斷、重新整理資料庫、編輯、刪除）、連線中顯示 loading 轉圈、中斷時只收合該連線的展開節點。

### 驗證 / 修正（以 Docker 真實資料庫整合測試，7/7 通過）
- **修正 MySQL `list_databases` 回空清單的 bug**：原本用 `SHOW DATABASES` + `try_get::<String>().ok()`，但 sqlx-mysql 對該欄位常回 binary 型別導致解碼失敗、被 `.ok()` 默默丟棄，**整個資料庫清單變空（連線樹展不開）**。改用 `information_schema.SCHEMATA` 並加 bytes 解碼後備（`str_col`），`list_tables` / `table_columns` / `primary_key` 一併套用。
- **russh 改用 `ring` crypto backend**（`default-features=false, features=["ring","flate2","rsa"]`），避免預設的 aws-lc-sys 在 Windows 需要 NASM 才能編譯（減少建置前置需求）。
- 補上 `src-tauri/icons/icon.ico`（tauri-build 在 Windows 產生資源檔必需；原本只有 icon.png 會導致建置失敗）。
- 整合測試 `src-tauri/src/it_tests.rs`（`cargo test --lib it_tests -- --include-ignored`）：覆蓋五大資料庫的連線 / CRUD / 多欄 AND 篩選、Redis 五型結構編輯、SQLite 備份還原、排程 next_run、連線持久化序列化（確認 secret 不落地）。

### 連線設定持久化 + OS keychain（`store.rs`）
- 連線設定寫入 `<app_config_dir>/connections.json`（原子寫入 temp + rename），**密碼 / SSH secret 一律存 OS keychain（`keyring`）**，磁碟不含任何密碼、也不回傳前端。
- 啟動自動載入連線清單（不自動連線）；`connect` / `test` / `backup` 於後端從 keychain hydrate 密碼（剛輸入的新密碼非空則跳過，向後相容）。
- 新增 command：`list_saved_connections` / `save_connection` / `remove_saved_connection`；側欄連線列新增刪除鈕（一併清除 keychain）。
- keychain 依平台選原生後端（Windows Credential Manager / macOS Keychain / Linux Secret Service），讀取失敗一律優雅降級為空密碼。

### SSH Tunnel（`ssh.rs`，`russh` 純 Rust）
- 連線前若啟用 SSH，開 `direct-tcpip` 本地轉發（`127.0.0.1:<OS 分配埠>`），driver 連本地埠；支援密碼 / 私鑰（passphrase）認證。
- tunnel 與 driver 生命週期綁定（`LiveConn`）：disconnect / close_all / test 一併收掉；driver 建立失敗時手動關閉 tunnel 避免任務洩漏。
- `ConnectionConfig` 新增 8 個 `#[serde(default)]` SSH 欄位（向後相容）；連線對話框新增可摺疊「SSH Tunnel」區塊（SQLite 隱藏）。
- 安全備註：此版本不驗 host key（dev 工具；TOFU 為後續）。

### 排程備份 + 備份歷史（`scheduler.rs`）
- 結構化週期（每 N 分 / 每 N 時 / 每天定時，`chrono::Local`），背景 tokio 迴圈每 30s 檢查到期排程；**僅在 app 開啟時執行**，關閉期間到期者不補跑。
- 排程持久化 `schedules.json`、歷史 `history.json`（上限 500 筆、newest-first）；fire 時以 `store::load_connection` 從 keychain 補密碼後呼叫既有 `backup::backup`，成敗都寫歷史。
- 保留份數（選填）：只刪該排程自己產出的檔。新增 command：`list/save/remove/toggle_schedule`、`run_schedule_now`、`list_backup_history`、`restore_from_history`、`clear_history`。
- `BackupDialog` 擴為三分頁（手動 / 排程 / 歷史）；歷史可一鍵還原（Redis 列停用，沿用「暫未支援」）。

### UX 套組
- **Redis 結構編輯**：新增 `KeyEdit`（serde `tag="action"`）+ trait 預設方法 `key_edit`（非 Redis 回 `Unsupported`）+ `key_edit` command。Redis driver 實作 List（LSET/LPUSH/RPUSH/LREM）/ Set（SADD/SREM）/ ZSet（ZADD/ZREM）/ Hash（HSET/HDEL）；鍵詳情彈窗各型別可就地編輯 / 新增 / 刪除元素，String 仍走 `update_cell`。
- **多欄複合篩選**：`FilterBar` 改多列（＋新增條件 / 移除），送出完整 `Filter[]`（後端早已 AND 串接）。AND-only；Mongo 同欄多條件後者勝；Redis 僅 `key` 欄有效。
- **欄寬拖曳**：資料表 `table-layout: fixed` + 表頭右緣可拖曳調整，per-table 寬度存於 `localStorage`；長值裁切（ellipsis）並可 hover 看全文。

---

## 備份 / 還原（本次）

### 新增
- **備份模組**（`backup.rs`）：手動備份與還原，策略為「官方 CLI 為主 + SQLite 檔案複製」。
  - MySQL → mysqldump / mysql；PostgreSQL → pg_dump / psql；MongoDB → mongodump / mongorestore（--archive 單檔）；Redis → redis-cli --rdb；SQLite → 直接複製檔案。
  - `backup_detect_cli`：偵測對應 CLI 是否在 PATH，UI 即時顯示狀態；找不到時給明確安裝提示。
  - 密碼以環境變數傳遞（MYSQL_PWD / PGPASSWORD），不出現在行程參數列表。
- **備份對話框**（`BackupDialog.tsx`）：工具列「備份」按鈕開啟；備份/還原模式切換、資料庫名稱、檔案路徑輸入、CLI 偵測狀態與結果顯示。

### 已知限制
- 輸出/輸入路徑目前為手動輸入（尚未接系統檔案選取對話框）。
- Redis 自動還原暫未支援（提示以 redis-cli 手動匯入 RDB）。
- 內建邏輯匯出（無 CLI 時的後備）除 SQLite 外尚未補完。

---

## Redis 支援（本次）— 五大資料庫到齊

### 新增
- **Redis driver**（`db/redis.rs`）：鍵值型，沿用「key 列表化」的表格手感。
  - `list_databases` → DB 0–15；`list_tables` → 虛擬節點 keys。
  - `table_data` → 以 **SCAN**（游標式，嚴禁 KEYS \*）列舉 key，呈現 key / type / ttl 三欄；key 作為主鍵；篩選的 key like/= 自動轉成 SCAN MATCH pattern。
  - **key_detail**（trait 新增的鍵值型專屬方法，非 Redis 預設回 None）：依型別展開五種結構 — String / List / Set / ZSet（含 score）/ Hash（field-value）。
  - `update_cell`：改 string 值或 TTL（EXPIRE/PERSIST）；`insert_row`：SET 新 key；`delete_row`：DEL。
  - `query`：接受原始 Redis 命令列（可加 `<db>:` 前綴選庫）。
- 前端：Redis 連線雙擊 key 開「鍵詳情」彈窗，依型別以對應表格/清單呈現；TTL 可直接在資料表編輯。
- 連線對話框移除「僅部分可連線」提示 — **五種資料庫全部可實際連線**。

### 架構
- `DatabaseDriver` trait 新增 `key_detail` 預設方法（回 None），讓鍵值型專屬能力不污染其他 driver。
- `Active` enum 與 connect/test 現已涵蓋全部 5 種，移除 unsupported catch-all。

---

## MongoDB 支援（本次）

### 新增
- **MongoDB driver**（`db/mongo.rs`）：文件型資料庫，沿用統一 `DatabaseDriver` trait 與 Navicat 表格手感。
  - `list_databases` → Mongo 資料庫；`list_tables` → 集合（kind=collection）。
  - `table_data` → 取一批文件，**聯集頂層欄位攤平成表格**（`_id` 固定第一欄），巢狀物件/陣列以 JSON 字串呈現。
  - `table_columns` → 抽樣 50 份文件推斷頂層欄位與 BSON 型別（「結構」分頁）。
  - 篩選 → Mongo find filter（比較運算子；like → 不分大小寫正規式）；排序 → sort document。
  - `update_cell` / `insert_row` / `delete_row` → 以 `_id` 定位的文件操作；ObjectId 字串自動轉型。
  - `query` → 接受 JSON `{db, collection, filter}` 回傳符合文件。
- mongodb crate 的 Client 內建連線池（maxPoolSize），無需自管池。
- 連線對話框開放 MongoDB；前端表格 / 編輯 / 篩選 / 排序元件直接沿用（`_id` 作為主鍵）。

### 可實際連線
- MySQL、PostgreSQL、SQLite、MongoDB（Redis 為後續階段）。

---

## 完整 CRUD + 篩選排序（本次）

### 新增
- **新增列 / 刪除列**：完成關聯式表的完整 CRUD。
  - 後端 `insert_row`（欄位+值，未列出欄交由 DB 預設）、`delete_row`（以主鍵定位，含 NULL 防呆）。
  - 前端：動作列 **＋ 新增列**（對話框可逐欄填值或標 NULL）、每列尾端 **−** 刪除鈕（含二次確認）。
- **排序**：點欄位標題循環切換 無 → ▲ 升冪 → ▼ 降冪；可一鍵清除。
- **篩選**：單欄條件列，運算子白名單（=, ≠, >, ≥, <, ≤, like, is null, not null），值以參數綁定。
- `table_data` 改用 `DataQuery`（page / page_size / filters / sorts），三個 SQL driver 各自組 WHERE/ORDER（識別字轉義、值綁定，MySQL/SQLite 用 `?`、PG 用 `$N`）。

### 安全
- 篩選運算子限定白名單；排序與篩選的欄位皆經識別字轉義；所有值一律參數綁定，不字串拼接。

---

## P1/P2 + 寫操作 + PostgreSQL（本次）

### 新增
- **PostgreSQL 支援**：新增 `db/postgres.rs`，完整實作 driver trait（schema 對應資料庫層級、`$1` 佔位符、雙引號識別字、pg_index 取主鍵）。
- **儲存格直接編輯**：資料表格可雙擊編輯，待套用變更以琥珀底標示，底部 **✓ 套用 / ✗ 捨棄**。
  - 後端 `update_cell`：以主鍵定位列，無主鍵或主鍵含 NULL 則拒絕；識別字轉義、值參數綁定防注入。
  - 前端可一鍵設為 NULL、Enter 套用、Esc 取消。
- `table_data` 新增回傳 `primary_key`，前端據此判斷可否編輯。
- `docs/` 資料夾：彙整所有規劃文件（planning / architecture / connection-lifecycle / navicat-ux / roadmap）與原始規劃 docx。

### 可實際連線
- MySQL、PostgreSQL、SQLite（MongoDB / Redis 為後續階段）。

---

## P1/P2 + SQLite

### 新增
- 連線樹展開到表，雙擊開表 → 表格檢視。
- **資料 / 結構** 分頁切換；資料分頁含底部導覽列與分頁。
- **SQLite 支援**（檔案型，連線對話框自動切換為檔案路徑欄位）。
- 主區改為多分頁（可開多張表 + 查詢分頁）。
- Windows 打包：`build-installer.ps1`（自動檢查/安裝 Rust 與 Node）+ tauri.conf.json 設定 msi/nsis。

### 修正
- 補上 `db/mod.rs` 遺漏的 `mod mysql;` 宣告。

---

## P0

### 新增
- Tauri 2 + React 18 + TypeScript 專案骨架。
- 大圖示工具列、連線樹（雙擊建連線、類型色標）、查詢編輯器（F6 執行）。
- 統一 `DatabaseDriver` trait + `ConnectionManager`。
- **MySQL driver**：連線池（idle_timeout / max_lifetime / test_before_acquire）。
- **連線釋放**：視窗關閉與程序退出時 drain 全部連線池。
