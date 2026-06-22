use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

pub mod mongo;
pub mod mysql;
pub mod postgres;
pub mod redis;
pub mod sqlite;

/// 資料庫範式。UI 與操作邏輯依此分流。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DbKind {
    Mysql,
    Postgres,
    Mongo,
    Redis,
    Sqlite,
}

impl DbKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            DbKind::Mysql => "mysql",
            DbKind::Postgres => "postgres",
            DbKind::Mongo => "mongo",
            DbKind::Redis => "redis",
            DbKind::Sqlite => "sqlite",
        }
    }

    /// 備份檔副檔名（與前端 BackupDialog 的 toolHint 對齊）。
    pub fn backup_ext(&self) -> &'static str {
        match self {
            DbKind::Mysql | DbKind::Postgres => ".sql",
            DbKind::Mongo => ".archive",
            DbKind::Redis => ".rdb",
            DbKind::Sqlite => ".db",
        }
    }
}

/// SSH Tunnel 認證方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SshAuthMethod {
    #[default]
    Password,
    Key,
}

/// 使用者填寫的連線設定。密碼與 SSH secret 存於 OS keychain，不寫入磁碟；
/// 連線時於後端 hydrate 回來（見 `store::kc_get` / commands hydrate）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionConfig {
    pub id: String,
    pub name: String,
    pub kind: DbKind,
    pub host: String,
    pub port: u16,
    pub username: String,
    #[serde(default)]
    pub password: String,
    /// 關聯式：預設資料庫；可空。
    #[serde(default)]
    pub database: Option<String>,
    /// 連線池上限。
    #[serde(default = "default_max_conns")]
    pub max_connections: u32,

    // ---- SSH Tunnel（可選；SQLite 不適用）----
    #[serde(default)]
    pub ssh_enabled: bool,
    #[serde(default)]
    pub ssh_host: String,
    /// 0 → 在使用點視為 22。
    #[serde(default)]
    pub ssh_port: u16,
    #[serde(default)]
    pub ssh_username: String,
    #[serde(default)]
    pub ssh_auth_method: SshAuthMethod,
    /// keychain only，不寫入磁碟。
    #[serde(default)]
    pub ssh_password: String,
    #[serde(default)]
    pub ssh_private_key_path: String,
    /// keychain only，不寫入磁碟。
    #[serde(default)]
    pub ssh_passphrase: String,
}

fn default_max_conns() -> u32 {
    5
}

/// 連線目前狀態，供 UI 監控用（呼應規劃文件 3.5 防呆機制）。
#[derive(Debug, Clone, Serialize)]
pub struct PoolStatus {
    pub size: u32,
    pub idle: u32,
    pub in_use: u32,
}

/// 查詢結果：欄位名 + 列（每格為字串，NULL 為 None）。
#[derive(Debug, Default, Serialize)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Option<String>>>,
    pub rows_affected: u64,
}

/// 表 / 視圖的基本資訊（連線樹展開用）。
#[derive(Debug, Clone, Serialize)]
pub struct TableInfo {
    pub name: String,
    /// "table" 或 "view"
    pub kind: String,
}

/// 欄位定義（「結構」分頁用）。
#[derive(Debug, Clone, Serialize)]
pub struct ColumnInfo {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub key: String,    // PRI / UNI / MUL / 空
    pub default: Option<String>,
    pub extra: String,  // auto_increment 等
}

/// 分頁資料結果（「資料」分頁用）。含總列數以渲染分頁器。
#[derive(Debug, Default, Serialize)]
pub struct PagedData {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Option<String>>>,
    pub total_rows: u64,
    pub page: u32,
    pub page_size: u32,
    /// 此表的主鍵欄位名（供前端判斷是否可編輯、組 WHERE 條件用）。
    #[serde(default)]
    pub primary_key: Vec<String>,
}

/// 單一儲存格的更新請求。
///
/// 以主鍵定位列：pk_values 是「主鍵欄位名 → 目前值」的清單。
/// 若該列任一主鍵值為 NULL，視為無法安全定位，後端拒絕更新。
#[derive(Debug, Clone, Deserialize)]
pub struct CellEdit {
    pub column: String,
    pub new_value: Option<String>,
    pub pk_columns: Vec<String>,
    pub pk_values: Vec<Option<String>>,
}

/// 新增列請求：欄位名 → 值。未列出的欄位交由 DB 預設值處理。
#[derive(Debug, Clone, Deserialize)]
pub struct RowInsert {
    pub columns: Vec<String>,
    pub values: Vec<Option<String>>,
}

/// 刪除列請求：以主鍵定位。
#[derive(Debug, Clone, Deserialize)]
pub struct RowDelete {
    pub pk_columns: Vec<String>,
    pub pk_values: Vec<Option<String>>,
}

/// Redis 鍵的詳細內容（鍵值型專屬）。type_ 決定 entries 的語意：
/// - string：entries 為單一 [value]
/// - list：entries 為有序元素 [v0, v1, ...]
/// - set：entries 為成員
/// - zset：entries 為 "member" 與 score 交錯，或見 scores
/// - hash：fields 與 entries 平行（field -> value）
#[derive(Debug, Clone, Serialize)]
pub struct KeyDetail {
    pub key: String,
    pub type_: String,
    pub ttl: i64,
    pub entries: Vec<String>,
    /// hash 用：欄位名（與 entries 對齊）
    #[serde(default)]
    pub fields: Vec<String>,
    /// zset 用：分數（與 entries 對齊）
    #[serde(default)]
    pub scores: Vec<f64>,
}

/// Redis 鍵的單筆結構編輯。`action` 標籤分流到各型別操作。
/// string 值編輯仍走既有 `update_cell`，不在此列。
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum KeyEdit {
    /// LSET：以索引就地改 list 元素。
    ListSet { index: i64, value: String },
    /// LPUSH（front=true）/ RPUSH：新增 list 元素。
    ListPush {
        value: String,
        #[serde(default)]
        front: bool,
    },
    /// LREM：依值移除 list 元素（count 預設 1）。
    ListRemove {
        value: String,
        #[serde(default = "one_i64")]
        count: i64,
    },
    /// SADD：新增 set 成員。
    SetAdd { member: String },
    /// SREM：移除 set 成員。
    SetRemove { member: String },
    /// ZADD：新增 / 覆寫 zset 成員分數。
    ZsetAdd { member: String, score: f64 },
    /// ZREM：移除 zset 成員。
    ZsetRemove { member: String },
    /// HSET：新增 / 覆寫 hash 欄位值。
    HashSet { field: String, value: String },
    /// HDEL：移除 hash 欄位。
    HashRemove { field: String },
}

fn one_i64() -> i64 {
    1
}

/// 排序方向。
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SortDir {
    Asc,
    Desc,
}

/// 單一欄位的篩選條件。運算子限定白名單，值以參數綁定。
#[derive(Debug, Clone, Deserialize)]
pub struct Filter {
    pub column: String,
    /// "=", "!=", ">", ">=", "<", "<=", "like", "is_null", "is_not_null"
    pub op: String,
    #[serde(default)]
    pub value: Option<String>,
}

/// 排序設定。
#[derive(Debug, Clone, Deserialize)]
pub struct Sort {
    pub column: String,
    pub dir: SortDir,
}

/// table_data 的查詢選項（分頁 + 篩選 + 排序）。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct DataQuery {
    pub page: u32,
    pub page_size: u32,
    #[serde(default)]
    pub filters: Vec<Filter>,
    #[serde(default)]
    pub sorts: Vec<Sort>,
    /// 多個篩選條件的連接方式：false = AND（預設）、true = OR。
    #[serde(default)]
    pub match_any: bool,
}

/// 統一驅動介面。各資料庫實作此 trait，差異吸收在 driver 層。
///
/// 注意：MongoDB / Redis 的 query 介面在 P4 / P5 會擴充專屬方法，
/// 此處先定義關聯式共用的能力。
#[async_trait::async_trait]
pub trait DatabaseDriver: Send + Sync {
    /// 建立連線池並做一次健康檢查。
    async fn connect(config: &ConnectionConfig) -> AppResult<Self>
    where
        Self: Sized;

    /// 健康檢查（殭屍連線偵測）。
    async fn ping(&self) -> AppResult<()>;

    /// 列出資料庫 / schema。
    async fn list_databases(&self) -> AppResult<Vec<String>>;

    /// 列出指定資料庫中的表與視圖（連線樹展開用）。
    async fn list_tables(&self, database: &str) -> AppResult<Vec<TableInfo>>;

    /// 取得表的欄位定義（「結構」分頁用）。
    async fn table_columns(&self, database: &str, table: &str)
        -> AppResult<Vec<ColumnInfo>>;

    /// 分頁讀取表資料（「資料」分頁用），支援篩選與排序。
    async fn table_data(
        &self,
        database: &str,
        table: &str,
        query: &DataQuery,
    ) -> AppResult<PagedData>;

    /// 執行查詢。
    async fn query(&self, sql: &str) -> AppResult<QueryResult>;

    /// 更新單一儲存格（寫回 DB）。以主鍵定位列。
    async fn update_cell(
        &self,
        database: &str,
        table: &str,
        edit: &CellEdit,
    ) -> AppResult<u64>;

    /// 新增一列。
    async fn insert_row(
        &self,
        database: &str,
        table: &str,
        row: &RowInsert,
    ) -> AppResult<u64>;

    /// 刪除一列（以主鍵定位）。
    async fn delete_row(
        &self,
        database: &str,
        table: &str,
        del: &RowDelete,
    ) -> AppResult<u64>;

    /// 連線池狀態。
    fn pool_status(&self) -> PoolStatus;

    /// 鍵值型專屬：取得單一鍵的詳細內容。非 Redis 預設回 None。
    async fn key_detail(&self, _database: &str, _key: &str) -> AppResult<Option<KeyDetail>> {
        Ok(None)
    }

    /// 鍵值型專屬：對單一鍵做結構編輯（List/Set/ZSet/Hash 元素增刪改）。
    /// 非鍵值型資料庫預設回 Unsupported。
    async fn key_edit(&self, _database: &str, _key: &str, _edit: &KeyEdit) -> AppResult<u64> {
        Err(AppError::Unsupported("此資料庫不支援鍵結構編輯".into()))
    }

    /// 優雅關閉：drain 連線池。
    async fn close(&self);
}

/// 允許的篩選運算子白名單 → SQL 片段（不含值，值另以參數綁定）。
/// 回傳 None 表示運算子不被允許。
pub fn filter_op_sql(op: &str) -> Option<&'static str> {
    match op {
        "=" => Some("="),
        "!=" => Some("<>"),
        ">" => Some(">"),
        ">=" => Some(">="),
        "<" => Some("<"),
        "<=" => Some("<="),
        "like" => Some("LIKE"),
        "is_null" => Some("IS NULL"),
        "is_not_null" => Some("IS NOT NULL"),
        _ => None,
    }
}

/// 運算子是否需要綁定值（is_null / is_not_null 不需要）。
pub fn op_needs_value(op: &str) -> bool {
    !matches!(op, "is_null" | "is_not_null")
}
