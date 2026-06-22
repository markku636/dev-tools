use sqlx::mysql::{MySqlPoolOptions, MySqlRow};
use sqlx::{Column, MySqlPool, Row, TypeInfo, ValueRef};
use std::time::Duration;

use crate::db::{
    filter_op_sql, op_needs_value, CellEdit, ColumnInfo, ConnectionConfig, DataQuery,
    DatabaseDriver, Filter, PagedData, PoolStatus, QueryResult, RowDelete, RowInsert, Sort,
    SortDir, TableInfo,
};
use crate::error::{AppError, AppResult};

/// MySQL 驅動：包一個 sqlx 連線池。
///
/// 連線生命週期（呼應規劃 3.5）：
/// - max_connections / idle_timeout / max_lifetime 由 pool 管理
/// - test_before_acquire 在取得連線前做健康檢查，淘汰殭屍連線
/// - Drop / close 時 drain pool
pub struct MysqlDriver {
    pool: MySqlPool,
}

#[async_trait::async_trait]
impl DatabaseDriver for MysqlDriver {
    async fn connect(config: &ConnectionConfig) -> AppResult<Self> {
        let db = config.database.clone().unwrap_or_default();
        let url = format!(
            "mysql://{user}:{pass}@{host}:{port}/{db}",
            user = config.username,
            pass = config.password,
            host = config.host,
            port = config.port,
            db = db,
        );

        let pool = MySqlPoolOptions::new()
            .max_connections(config.max_connections)
            .min_connections(0)
            .idle_timeout(Duration::from_secs(300)) // 閒置 5 分鐘回收
            .max_lifetime(Duration::from_secs(1800)) // 連線最長存活 30 分鐘
            .acquire_timeout(Duration::from_secs(10))
            .test_before_acquire(true) // 取得前健康檢查
            .connect(&url)
            .await
            .map_err(|e| AppError::Connect(e.to_string()))?;

        let driver = Self { pool };
        driver.ping().await?;
        Ok(driver)
    }

    async fn ping(&self) -> AppResult<()> {
        sqlx::query("SELECT 1")
            .execute(&self.pool)
            .await
            .map(|_| ())
            .map_err(|e| AppError::Connect(e.to_string()))
    }

    async fn list_databases(&self) -> AppResult<Vec<String>> {
        // 用 information_schema.SCHEMATA（SCHEMA_NAME 為 varchar，可穩定解碼成 String）；
        // 不用 `SHOW DATABASES`——其欄位在 sqlx-mysql 常被當成 binary，try_get::<String> 會失敗
        // 而被 .ok() 默默丟棄，導致整個資料庫清單變空（連線樹展不開）。
        let rows = sqlx::query(
            "SELECT SCHEMA_NAME FROM information_schema.SCHEMATA ORDER BY SCHEMA_NAME",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Query(e.to_string()))?;
        Ok(rows.iter().filter_map(|r| str_col(r, 0)).collect())
    }

    async fn list_tables(&self, database: &str) -> AppResult<Vec<TableInfo>> {
        // information_schema 統一取表與視圖。
        let sql = "SELECT TABLE_NAME, TABLE_TYPE \
                   FROM information_schema.TABLES \
                   WHERE TABLE_SCHEMA = ? ORDER BY TABLE_NAME";
        let rows = sqlx::query(sql)
            .bind(database)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Query(e.to_string()))?;
        Ok(rows
            .iter()
            .filter_map(|r| {
                let name = str_col(r, 0)?;
                let ttype = str_col(r, 1).unwrap_or_default();
                let kind = if ttype.eq_ignore_ascii_case("VIEW") {
                    "view"
                } else {
                    "table"
                };
                Some(TableInfo { name, kind: kind.to_string() })
            })
            .collect())
    }

    async fn table_columns(
        &self,
        database: &str,
        table: &str,
    ) -> AppResult<Vec<ColumnInfo>> {
        let sql = "SELECT COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE, COLUMN_KEY, \
                   COLUMN_DEFAULT, EXTRA \
                   FROM information_schema.COLUMNS \
                   WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? \
                   ORDER BY ORDINAL_POSITION";
        let rows = sqlx::query(sql)
            .bind(database)
            .bind(table)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Query(e.to_string()))?;
        Ok(rows
            .iter()
            .map(|r| ColumnInfo {
                name: str_col(r, 0).unwrap_or_default(),
                data_type: str_col(r, 1).unwrap_or_default(),
                nullable: str_col(r, 2)
                    .map(|v| v.eq_ignore_ascii_case("YES"))
                    .unwrap_or(false),
                key: str_col(r, 3).unwrap_or_default(),
                default: str_col(r, 4),
                extra: str_col(r, 5).unwrap_or_default(),
            })
            .collect())
    }

    async fn table_data(
        &self,
        database: &str,
        table: &str,
        query: &DataQuery,
    ) -> AppResult<PagedData> {
        let q_db = quote_ident(database);
        let q_tbl = quote_ident(table);
        let offset = query.page.saturating_mul(query.page_size);
        let page_size = query.page_size;

        // 組 WHERE（運算子白名單檢查；值以 ? 綁定）
        let (where_sql, bind_values) = build_where(&query.filters, query.match_any)?;
        // 組 ORDER BY（只允許識別字，方向限定）
        let order_sql = build_order(&query.sorts);

        // 總列數（套用相同篩選）
        let count_sql = format!("SELECT COUNT(*) FROM {q_db}.{q_tbl}{where_sql}");
        let mut cq = sqlx::query_scalar::<_, i64>(&count_sql);
        for v in &bind_values {
            cq = cq.bind(v.clone());
        }
        let total: i64 = cq
            .fetch_one(&self.pool)
            .await
            .map_err(|e| AppError::Query(e.to_string()))?;

        let data_sql = format!(
            "SELECT * FROM {q_db}.{q_tbl}{where_sql}{order_sql} LIMIT {page_size} OFFSET {offset}"
        );
        let mut dq = sqlx::query(&data_sql);
        for v in &bind_values {
            dq = dq.bind(v.clone());
        }
        let rows = dq
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Query(e.to_string()))?;

        let primary_key = self.primary_key(database, table).await?;

        let result = rows_to_result(&rows);
        Ok(PagedData {
            columns: result.columns,
            rows: result.rows,
            total_rows: total as u64,
            page: query.page,
            page_size,
            primary_key,
        })
    }

    async fn query(&self, sql: &str) -> AppResult<QueryResult> {
        let trimmed = sql.trim_start().to_ascii_lowercase();
        let is_read = trimmed.starts_with("select")
            || trimmed.starts_with("show")
            || trimmed.starts_with("describe")
            || trimmed.starts_with("explain");

        if is_read {
            let rows = sqlx::query(sql)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| AppError::Query(e.to_string()))?;
            Ok(rows_to_result(&rows))
        } else {
            let res = sqlx::query(sql)
                .execute(&self.pool)
                .await
                .map_err(|e| AppError::Query(e.to_string()))?;
            Ok(QueryResult {
                columns: vec![],
                rows: vec![],
                rows_affected: res.rows_affected(),
            })
        }
    }

    fn pool_status(&self) -> PoolStatus {
        let size = self.pool.size();
        let idle = self.pool.num_idle() as u32;
        PoolStatus {
            size,
            idle,
            in_use: size.saturating_sub(idle),
        }
    }

    async fn update_cell(
        &self,
        database: &str,
        table: &str,
        edit: &CellEdit,
    ) -> AppResult<u64> {
        if edit.pk_columns.is_empty() {
            return Err(AppError::Query(
                "此表無主鍵，無法安全更新".to_string(),
            ));
        }
        if edit.pk_columns.len() != edit.pk_values.len() {
            return Err(AppError::Query("主鍵欄位與值數量不符".to_string()));
        }
        // 任一主鍵值為 NULL 則拒絕（無法以 = 安全比對）。
        if edit.pk_values.iter().any(|v| v.is_none()) {
            return Err(AppError::Query(
                "主鍵值為 NULL，無法定位該列".to_string(),
            ));
        }

        let q_db = quote_ident(database);
        let q_tbl = quote_ident(table);
        let q_col = quote_ident(&edit.column);

        let where_clause = edit
            .pk_columns
            .iter()
            .map(|c| format!("{} = ?", quote_ident(c)))
            .collect::<Vec<_>>()
            .join(" AND ");

        let sql = format!(
            "UPDATE {q_db}.{q_tbl} SET {q_col} = ? WHERE {where_clause}"
        );

        // 綁定：先 SET 值，再各主鍵值。以字串綁定，交由 MySQL 隱式轉型。
        let mut q = sqlx::query(&sql).bind(edit.new_value.clone());
        for v in &edit.pk_values {
            q = q.bind(v.clone());
        }
        let res = q
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Query(e.to_string()))?;
        Ok(res.rows_affected())
    }

    async fn insert_row(
        &self,
        database: &str,
        table: &str,
        row: &RowInsert,
    ) -> AppResult<u64> {
        if row.columns.is_empty() {
            return Err(AppError::Query("未提供任何欄位".to_string()));
        }
        if row.columns.len() != row.values.len() {
            return Err(AppError::Query("欄位與值數量不符".to_string()));
        }
        let q_db = quote_ident(database);
        let q_tbl = quote_ident(table);
        let cols = row
            .columns
            .iter()
            .map(|c| quote_ident(c))
            .collect::<Vec<_>>()
            .join(", ");
        let placeholders = vec!["?"; row.values.len()].join(", ");
        let sql = format!("INSERT INTO {q_db}.{q_tbl} ({cols}) VALUES ({placeholders})");
        let mut q = sqlx::query(&sql);
        for v in &row.values {
            q = q.bind(v.clone());
        }
        let res = q
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Query(e.to_string()))?;
        Ok(res.rows_affected())
    }

    async fn delete_row(
        &self,
        database: &str,
        table: &str,
        del: &RowDelete,
    ) -> AppResult<u64> {
        if del.pk_columns.is_empty() {
            return Err(AppError::Query("此表無主鍵，無法安全刪除".to_string()));
        }
        if del.pk_columns.len() != del.pk_values.len() {
            return Err(AppError::Query("主鍵欄位與值數量不符".to_string()));
        }
        if del.pk_values.iter().any(|v| v.is_none()) {
            return Err(AppError::Query("主鍵值為 NULL，無法定位該列".to_string()));
        }
        let q_db = quote_ident(database);
        let q_tbl = quote_ident(table);
        let where_clause = del
            .pk_columns
            .iter()
            .map(|c| format!("{} = ?", quote_ident(c)))
            .collect::<Vec<_>>()
            .join(" AND ");
        let sql = format!("DELETE FROM {q_db}.{q_tbl} WHERE {where_clause}");
        let mut q = sqlx::query(&sql);
        for v in &del.pk_values {
            q = q.bind(v.clone());
        }
        let res = q
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Query(e.to_string()))?;
        Ok(res.rows_affected())
    }

    async fn close(&self) {
        self.pool.close().await;
    }
}

impl MysqlDriver {
    /// 取得表的主鍵欄位（依序）。無主鍵則回空。
    async fn primary_key(&self, database: &str, table: &str) -> AppResult<Vec<String>> {
        let sql = "SELECT COLUMN_NAME FROM information_schema.KEY_COLUMN_USAGE \
                   WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? \
                   AND CONSTRAINT_NAME = 'PRIMARY' \
                   ORDER BY ORDINAL_POSITION";
        let rows = sqlx::query(sql)
            .bind(database)
            .bind(table)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Query(e.to_string()))?;
        Ok(rows.iter().filter_map(|r| str_col(r, 0)).collect())
    }
}

/// 以反引號包裹識別字，並轉義內部反引號，防止 SQL 注入。
fn quote_ident(ident: &str) -> String {
    format!("`{}`", ident.replace('`', "``"))
}

/// 穩健讀取字串欄位：先試 String，失敗則以 bytes 解碼。
/// sqlx-mysql 對部分 information_schema 欄位（如 SCHEMATA.SCHEMA_NAME）會回 binary，
/// 直接 try_get::<String> 會失敗——若再用 .ok() 默默丟棄，整個清單就變空。
fn str_col(row: &MySqlRow, idx: usize) -> Option<String> {
    row.try_get::<String, _>(idx).ok().or_else(|| {
        row.try_get::<Vec<u8>, _>(idx)
            .ok()
            .map(|b| String::from_utf8_lossy(&b).into_owned())
    })
}

/// 組 WHERE 子句（MySQL `?` 佔位符）。回傳 (sql_片段, 需綁定的值)。
/// sql_片段含前導空白與 " WHERE "，無篩選則為空字串。
fn build_where(filters: &[Filter], match_any: bool) -> AppResult<(String, Vec<Option<String>>)> {
    if filters.is_empty() {
        return Ok((String::new(), vec![]));
    }
    let mut clauses = Vec::new();
    let mut binds = Vec::new();
    for f in filters {
        let op = filter_op_sql(&f.op)
            .ok_or_else(|| AppError::Query(format!("不支援的運算子：{}", f.op)))?;
        let col = quote_ident(&f.column);
        if op_needs_value(&f.op) {
            clauses.push(format!("{col} {op} ?"));
            binds.push(f.value.clone());
        } else {
            clauses.push(format!("{col} {op}"));
        }
    }
    let connector = if match_any { " OR " } else { " AND " };
    Ok((format!(" WHERE {}", clauses.join(connector)), binds))
}

/// 組 ORDER BY 子句（識別字 + 方向，無值綁定）。
fn build_order(sorts: &[Sort]) -> String {
    if sorts.is_empty() {
        return String::new();
    }
    let parts = sorts
        .iter()
        .map(|s| {
            let dir = match s.dir {
                SortDir::Asc => "ASC",
                SortDir::Desc => "DESC",
            };
            format!("{} {}", quote_ident(&s.column), dir)
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(" ORDER BY {parts}")
}

/// 將 MySQL 列轉成字串格. 盡量以文字呈現，型別細節留待後續加強。
fn rows_to_result(rows: &[MySqlRow]) -> QueryResult {
    if rows.is_empty() {
        return QueryResult::default();
    }
    let columns: Vec<String> = rows[0]
        .columns()
        .iter()
        .map(|c| c.name().to_string())
        .collect();

    let mut out_rows = Vec::with_capacity(rows.len());
    for row in rows {
        let mut cells = Vec::with_capacity(columns.len());
        for i in 0..columns.len() {
            cells.push(cell_to_string(row, i));
        }
        out_rows.push(cells);
    }

    QueryResult {
        columns,
        rows: out_rows,
        rows_affected: 0,
    }
}

/// 嘗試以常見型別讀取單一儲存格並轉成字串；NULL 回傳 None。
fn cell_to_string(row: &MySqlRow, idx: usize) -> Option<String> {
    // NULL 偵測
    if let Ok(raw) = row.try_get_raw(idx) {
        if raw.is_null() {
            return None;
        }
    }
    let type_name = row
        .columns()
        .get(idx)
        .map(|c| c.type_info().name().to_string())
        .unwrap_or_default();

    // 依宣告型別嘗試對應 Rust 型別，失敗則退回字串。
    match type_name.as_str() {
        "TINYINT" | "SMALLINT" | "INT" | "MEDIUMINT" | "BIGINT" => row
            .try_get::<i64, _>(idx)
            .ok()
            .map(|v| v.to_string())
            .or_else(|| string_fallback(row, idx)),
        "FLOAT" | "DOUBLE" => row
            .try_get::<f64, _>(idx)
            .ok()
            .map(|v| v.to_string())
            .or_else(|| string_fallback(row, idx)),
        "DECIMAL" => string_fallback(row, idx),
        "BOOLEAN" | "BOOL" => row
            .try_get::<bool, _>(idx)
            .ok()
            .map(|v| v.to_string())
            .or_else(|| string_fallback(row, idx)),
        _ => string_fallback(row, idx),
    }
}

fn string_fallback(row: &MySqlRow, idx: usize) -> Option<String> {
    // 依序嘗試常見可轉字串的型別。
    if let Ok(v) = row.try_get::<String, _>(idx) {
        return Some(v);
    }
    if let Ok(v) = row.try_get::<i64, _>(idx) {
        return Some(v.to_string());
    }
    if let Ok(v) = row.try_get::<f64, _>(idx) {
        return Some(v.to_string());
    }
    if let Ok(v) = row.try_get::<bool, _>(idx) {
        return Some(v.to_string());
    }
    if let Ok(v) = row.try_get::<bigdecimal::BigDecimal, _>(idx) {
        return Some(v.to_string());
    }
    if let Ok(v) = row.try_get::<chrono::NaiveDateTime, _>(idx) {
        return Some(v.to_string());
    }
    if let Ok(v) = row.try_get::<chrono::NaiveDate, _>(idx) {
        return Some(v.to_string());
    }
    if let Ok(v) = row.try_get::<Vec<u8>, _>(idx) {
        return Some(String::from_utf8_lossy(&v).into_owned());
    }
    Some("<unrenderable>".to_string())
}
