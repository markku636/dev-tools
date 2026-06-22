# 架構設計

## 分層

```
┌─────────────────────────────────────────────┐
│ 前端 UI 層 (React + TS)                       │
│  ┌─────────┬──────────────────────────────┐  │
│  │ 共用     │ 大圖示工具列 / 連線樹 / 主題    │  │
│  │ 分流     │ 資料檢視器 / 查詢編輯器         │  │
│  └─────────┴──────────────────────────────┘  │
├─────────────────────────────────────────────┤
│ Tauri 橋接層：command 路由 / 事件 / 進度回報    │
├─────────────────────────────────────────────┤
│ Rust 核心層                                    │
│  ┌─────────┬──────────────────────────────┐  │
│  │ 共用     │ ConnectionManager / 加密 / 排程 │  │
│  │ 分流     │ Driver 實作 / Backup Provider   │  │
│  └─────────┴──────────────────────────────┘  │
└─────────────────────────────────────────────┘
```

共用部分（連線管理、UI 外殼、主題）統一實作；差異部分（資料操作、檢視元件）依範式分流。估計多種資料庫可共用約 60% 程式碼。

## 統一 Driver 抽象

以 Rust trait 定義統一驅動介面，用 enum 區分範式，差異吸收在 driver 層。

```rust
// src-tauri/src/db/mod.rs
pub enum DbKind { Mysql, Postgres, Mongo, Redis, Sqlite }

#[async_trait]
pub trait DatabaseDriver: Send + Sync {
    async fn connect(config: &ConnectionConfig) -> AppResult<Self> where Self: Sized;
    async fn ping(&self) -> AppResult<()>;
    async fn list_databases(&self) -> AppResult<Vec<String>>;
    async fn list_tables(&self, database: &str) -> AppResult<Vec<TableInfo>>;
    async fn table_columns(&self, database: &str, table: &str) -> AppResult<Vec<ColumnInfo>>;
    async fn table_data(&self, database: &str, table: &str, page: u32, page_size: u32) -> AppResult<PagedData>;
    async fn query(&self, sql: &str) -> AppResult<QueryResult>;
    async fn update_cell(&self, database: &str, table: &str, edit: &CellEdit) -> AppResult<u64>;
    fn pool_status(&self) -> PoolStatus;
    async fn close(&self);
}
```

`ConnectionManager` 持有一個 `Active` enum（每種已連線 driver 一個 variant），對外提供統一方法，內部 `match` 分派到對應 driver。新增資料庫只需：(1) 新增 driver 檔、(2) 在 `Active` 加 variant、(3) 在 `connect`/`test` 加 match arm。

## 寫操作安全

`update_cell` 以主鍵定位列：

- 表無主鍵 → 拒絕更新（避免誤改多列）。
- 主鍵值含 NULL → 拒絕（無法以 `=` 安全比對）。
- 所有識別字（庫/表/欄）以對應引號包裹並轉義（MySQL 反引號、PG/SQLite 雙引號）。
- 值一律以參數綁定（MySQL/SQLite 用 `?`，PostgreSQL 用 `$1`），不字串拼接。

## 模組結構

```
src-tauri/src/
├── main.rs            程序進入點
├── lib.rs             Tauri builder、command 註冊、優雅關閉
├── error.rs           統一錯誤型別（序列化為 {kind, message}）
├── manager.rs         ConnectionManager + Active enum 分派
├── commands/mod.rs    Tauri command（薄包裝）
└── db/
    ├── mod.rs         DbKind、共用型別、DatabaseDriver trait
    ├── mysql.rs       MySQL driver
    ├── postgres.rs    PostgreSQL driver
    └── sqlite.rs      SQLite driver
```
