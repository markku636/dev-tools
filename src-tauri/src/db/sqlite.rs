use sqlx::sqlite::{SqlitePoolOptions, SqliteRow};
use sqlx::{Column, Row, SqlitePool, TypeInfo, ValueRef};
use std::time::Duration;

use crate::db::{
    filter_op_sql, op_needs_value, AlterOp, CellEdit, ColumnInfo, ConnectionConfig, DataQuery,
    DatabaseDriver, ErColumn, ErModel, ErRelation, ErTable, Filter, PagedData, PoolStatus,
    QueryResult, RowDelete, RowInsert, Sort, SortDir, TableInfo,
};
use crate::error::{AppError, AppResult};

/// SQLite 驅動。與 MySQL 共用同一套 trait，差異在：
/// - 無 host/port，連線目標是檔案路徑（取自 config.database）
/// - 只有單一資料庫（檔案本身），list_databases 回傳 "main"
pub struct SqliteDriver {
    pool: SqlitePool,
}

#[async_trait::async_trait]
impl DatabaseDriver for SqliteDriver {
    async fn connect(config: &ConnectionConfig) -> AppResult<Self> {
        // SQLite 路徑放在 database 欄位；空則用記憶體資料庫。
        let path = config
            .database
            .clone()
            .filter(|p| !p.is_empty())
            .unwrap_or_else(|| ":memory:".to_string());

        // 已存在的檔案才開啟，不自動建立（唯讀檢視語意較安全）。
        let url = if path == ":memory:" {
            "sqlite::memory:".to_string()
        } else {
            format!("sqlite://{path}?mode=rwc")
        };

        let pool = SqlitePoolOptions::new()
            .max_connections(config.max_connections.max(1))
            .idle_timeout(Duration::from_secs(300))
            .acquire_timeout(Duration::from_secs(10))
            .test_before_acquire(true)
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
        // SQLite 單檔即一個資料庫，固定回傳 main。
        Ok(vec!["main".to_string()])
    }

    async fn list_tables(&self, _database: &str) -> AppResult<Vec<TableInfo>> {
        let rows = sqlx::query(
            "SELECT name, type FROM sqlite_master \
             WHERE type IN ('table','view') AND name NOT LIKE 'sqlite_%' \
             ORDER BY name",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Query(e.to_string()))?;
        Ok(rows
            .iter()
            .filter_map(|r| {
                let name: String = r.try_get(0).ok()?;
                let ttype: String = r.try_get(1).unwrap_or_default();
                Some(TableInfo { name, kind: ttype })
            })
            .collect())
    }

    async fn table_columns(
        &self,
        _database: &str,
        table: &str,
    ) -> AppResult<Vec<ColumnInfo>> {
        // PRAGMA table_info 不支援參數綁定，需手動轉義表名。
        let sql = format!("PRAGMA table_info({})", quote_ident(table));
        let rows = sqlx::query(&sql)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Query(e.to_string()))?;
        // 欄位順序：cid, name, type, notnull, dflt_value, pk
        Ok(rows
            .iter()
            .map(|r| {
                let notnull: i64 = r.try_get(3).unwrap_or(0);
                let pk: i64 = r.try_get(5).unwrap_or(0);
                ColumnInfo {
                    name: r.try_get(1).unwrap_or_default(),
                    data_type: r.try_get(2).unwrap_or_default(),
                    nullable: notnull == 0,
                    key: if pk > 0 { "PRI".to_string() } else { String::new() },
                    default: r.try_get(4).ok(),
                    extra: String::new(),
                }
            })
            .collect())
    }

    async fn table_data(
        &self,
        _database: &str,
        table: &str,
        query: &DataQuery,
    ) -> AppResult<PagedData> {
        let q_tbl = quote_ident(table);
        let offset = query.page.saturating_mul(query.page_size);
        let page_size = query.page_size;

        let (where_sql, bind_values) = build_where(&query.filters, query.match_any)?;
        let order_sql = build_order(&query.sorts);

        let count_sql = format!("SELECT COUNT(*) FROM {q_tbl}{where_sql}");
        let mut cq = sqlx::query_scalar::<_, i64>(&count_sql);
        for v in &bind_values {
            cq = cq.bind(v.clone());
        }
        let total: i64 = cq
            .fetch_one(&self.pool)
            .await
            .map_err(|e| AppError::Query(e.to_string()))?;

        let data_sql = format!(
            "SELECT * FROM {q_tbl}{where_sql}{order_sql} LIMIT {page_size} OFFSET {offset}"
        );
        let mut dq = sqlx::query(&data_sql);
        for v in &bind_values {
            dq = dq.bind(v.clone());
        }
        let rows = dq
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Query(e.to_string()))?;

        let result = rows_to_result(&rows);
        Ok(PagedData {
            columns: result.columns,
            rows: result.rows,
            total_rows: total as u64,
            page: query.page,
            page_size,
            primary_key: self.primary_key(table).await?,
        })
    }

    async fn query(&self, sql: &str) -> AppResult<QueryResult> {
        let trimmed = sql.trim_start().to_ascii_lowercase();
        let is_read = trimmed.starts_with("select")
            || trimmed.starts_with("pragma")
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
        _database: &str,
        table: &str,
        edit: &CellEdit,
    ) -> AppResult<u64> {
        if edit.pk_columns.is_empty() {
            return Err(AppError::Query("此表無主鍵，無法安全更新".to_string()));
        }
        if edit.pk_columns.len() != edit.pk_values.len() {
            return Err(AppError::Query("主鍵欄位與值數量不符".to_string()));
        }
        if edit.pk_values.iter().any(|v| v.is_none()) {
            return Err(AppError::Query("主鍵值為 NULL，無法定位該列".to_string()));
        }

        let q_tbl = quote_ident(table);
        let q_col = quote_ident(&edit.column);
        let where_clause = edit
            .pk_columns
            .iter()
            .map(|c| format!("{} = ?", quote_ident(c)))
            .collect::<Vec<_>>()
            .join(" AND ");

        let sql = format!("UPDATE {q_tbl} SET {q_col} = ? WHERE {where_clause}");
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
        _database: &str,
        table: &str,
        row: &RowInsert,
    ) -> AppResult<u64> {
        if row.columns.is_empty() {
            return Err(AppError::Query("未提供任何欄位".to_string()));
        }
        if row.columns.len() != row.values.len() {
            return Err(AppError::Query("欄位與值數量不符".to_string()));
        }
        let q_tbl = quote_ident(table);
        let cols = row
            .columns
            .iter()
            .map(|c| quote_ident(c))
            .collect::<Vec<_>>()
            .join(", ");
        let placeholders = vec!["?"; row.values.len()].join(", ");
        let sql = format!("INSERT INTO {q_tbl} ({cols}) VALUES ({placeholders})");
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
        _database: &str,
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
        let q_tbl = quote_ident(table);
        let where_clause = del
            .pk_columns
            .iter()
            .map(|c| format!("{} = ?", quote_ident(c)))
            .collect::<Vec<_>>()
            .join(" AND ");
        let sql = format!("DELETE FROM {q_tbl} WHERE {where_clause}");
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

    async fn explain(&self, sql: &str) -> AppResult<QueryResult> {
        let rows = sqlx::query(&format!("EXPLAIN QUERY PLAN {sql}"))
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Query(e.to_string()))?;
        Ok(rows_to_result(&rows))
    }

    async fn alter_table(&self, _database: &str, table: &str, op: &AlterOp) -> AppResult<()> {
        let q_tbl = quote_ident(table);
        let ddl = match op {
            AlterOp::AddColumn { name, data_type, nullable, default } => {
                if data_type.trim().is_empty() {
                    return Err(AppError::Query("請指定欄位型別".into()));
                }
                let nn = if *nullable { "" } else { " NOT NULL" };
                let def = default.as_ref().map(|d| format!(" DEFAULT {d}")).unwrap_or_default();
                format!("ALTER TABLE {q_tbl} ADD COLUMN {} {data_type}{nn}{def}", quote_ident(name))
            }
            AlterOp::DropColumn { name } => {
                format!("ALTER TABLE {q_tbl} DROP COLUMN {}", quote_ident(name))
            }
            AlterOp::RenameColumn { old, new } => format!(
                "ALTER TABLE {q_tbl} RENAME COLUMN {} TO {}",
                quote_ident(old),
                quote_ident(new)
            ),
        };
        sqlx::query(&ddl)
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Query(e.to_string()))?;
        Ok(())
    }

    async fn er_model(&self, database: &str) -> AppResult<ErModel> {
        let tlist = self.list_tables(database).await?;
        let mut relations = Vec::new();
        let mut fk_cols: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();
        for t in &tlist {
            // PRAGMA foreign_key_list 欄位：id, seq, table, from, to, ...
            let rows = sqlx::query(&format!("PRAGMA foreign_key_list({})", quote_ident(&t.name)))
                .fetch_all(&self.pool)
                .await
                .map_err(|e| AppError::Query(e.to_string()))?;
            for r in &rows {
                let to_table: String = r.try_get(2).unwrap_or_default();
                let from_col: String = r.try_get(3).unwrap_or_default();
                let to_col: String = r.try_get(4).unwrap_or_default();
                if !to_table.is_empty() {
                    fk_cols.insert((t.name.clone(), from_col.clone()));
                    relations.push(ErRelation {
                        from_table: t.name.clone(),
                        from_column: from_col,
                        to_table,
                        to_column: to_col,
                    });
                }
            }
        }
        let mut tables = Vec::new();
        for t in &tlist {
            let cols = self.table_columns(database, &t.name).await?;
            let er_cols = cols
                .into_iter()
                .map(|c| ErColumn {
                    pk: c.key == "PRI",
                    fk: fk_cols.contains(&(t.name.clone(), c.name.clone())),
                    name: c.name,
                    data_type: c.data_type,
                })
                .collect();
            tables.push(ErTable { name: t.name.clone(), columns: er_cols });
        }
        Ok(ErModel { tables, relations })
    }

    async fn close(&self) {
        self.pool.close().await;
    }
}

impl SqliteDriver {
    /// 從 PRAGMA table_info 取主鍵欄位（pk > 0），依 pk 序號排列。
    async fn primary_key(&self, table: &str) -> AppResult<Vec<String>> {
        let sql = format!("PRAGMA table_info({})", quote_ident(table));
        let rows = sqlx::query(&sql)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Query(e.to_string()))?;
        // 收集 (pk_index, name)，pk_index > 0 表示是主鍵的第幾欄。
        let mut pks: Vec<(i64, String)> = rows
            .iter()
            .filter_map(|r| {
                let pk: i64 = r.try_get(5).ok()?;
                if pk > 0 {
                    let name: String = r.try_get(1).ok()?;
                    Some((pk, name))
                } else {
                    None
                }
            })
            .collect();
        pks.sort_by_key(|(idx, _)| *idx);
        Ok(pks.into_iter().map(|(_, n)| n).collect())
    }
}

/// 以雙引號包裹識別字（SQLite 標準），轉義內部雙引號。
fn quote_ident(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}

/// 組 WHERE 子句（`?` 佔位符）。回傳 (sql_片段, 需綁定的值)。
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

fn rows_to_result(rows: &[SqliteRow]) -> QueryResult {
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

/// SQLite 為動態型別，依儲存類別嘗試讀取。
fn cell_to_string(row: &SqliteRow, idx: usize) -> Option<String> {
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

    match type_name.to_ascii_uppercase().as_str() {
        "INTEGER" => row
            .try_get::<i64, _>(idx)
            .ok()
            .map(|v| v.to_string())
            .or_else(|| string_fallback(row, idx)),
        "REAL" => row
            .try_get::<f64, _>(idx)
            .ok()
            .map(|v| v.to_string())
            .or_else(|| string_fallback(row, idx)),
        _ => string_fallback(row, idx),
    }
}

fn string_fallback(row: &SqliteRow, idx: usize) -> Option<String> {
    if let Ok(v) = row.try_get::<String, _>(idx) {
        return Some(v);
    }
    if let Ok(v) = row.try_get::<i64, _>(idx) {
        return Some(v.to_string());
    }
    if let Ok(v) = row.try_get::<f64, _>(idx) {
        return Some(v.to_string());
    }
    if let Ok(v) = row.try_get::<Vec<u8>, _>(idx) {
        return Some(String::from_utf8_lossy(&v).into_owned());
    }
    Some("<unrenderable>".to_string())
}
