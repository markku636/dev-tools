use sqlx::postgres::{PgPoolOptions, PgRow};
use sqlx::{Column, PgPool, Row, TypeInfo, ValueRef};
use std::time::Duration;

use crate::db::{
    collect_relations, filter_op_sql, op_needs_value, AlterOp, CellEdit, ColumnInfo,
    ConnectionConfig, DataQuery, DatabaseDriver, ErColumn, ErModel, ErTable, Filter, PagedData,
    PoolStatus, QueryResult, RowDelete, RowInsert, Sort, SortDir, TableInfo,
};
use crate::error::{AppError, AppResult};

/// PostgreSQL 驅動。與 MySQL 共用 sqlx，差異：
/// - 佔位符為 $1, $2…（非 ?）
/// - 識別字以雙引號包裹
/// - schema 概念：list_databases 回傳 schema 清單（public 等）
pub struct PostgresDriver {
    pool: PgPool,
}

#[async_trait::async_trait]
impl DatabaseDriver for PostgresDriver {
    async fn connect(config: &ConnectionConfig) -> AppResult<Self> {
        let db = config.database.clone().unwrap_or_else(|| "postgres".to_string());
        let url = format!(
            "postgres://{user}:{pass}@{host}:{port}/{db}",
            user = config.username,
            pass = config.password,
            host = config.host,
            port = config.port,
            db = db,
        );

        let pool = PgPoolOptions::new()
            .max_connections(config.max_connections)
            .min_connections(0)
            .idle_timeout(Duration::from_secs(300))
            .max_lifetime(Duration::from_secs(1800))
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
        // PG 以 schema 對應 MySQL 的「資料庫」層級，列出非系統 schema。
        let rows = sqlx::query(
            "SELECT schema_name FROM information_schema.schemata \
             WHERE schema_name NOT IN ('pg_catalog','information_schema') \
             AND schema_name NOT LIKE 'pg_%' ORDER BY schema_name",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Query(e.to_string()))?;
        Ok(rows
            .iter()
            .filter_map(|r| r.try_get::<String, _>(0).ok())
            .collect())
    }

    async fn list_tables(&self, database: &str) -> AppResult<Vec<TableInfo>> {
        // database 此處即 schema 名。
        let rows = sqlx::query(
            "SELECT table_name, table_type FROM information_schema.tables \
             WHERE table_schema = $1 ORDER BY table_name",
        )
        .bind(database)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Query(e.to_string()))?;
        Ok(rows
            .iter()
            .filter_map(|r| {
                let name: String = r.try_get(0).ok()?;
                let ttype: String = r.try_get(1).unwrap_or_default();
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
        // 欄位定義
        let col_rows = sqlx::query(
            "SELECT column_name, data_type, is_nullable, column_default \
             FROM information_schema.columns \
             WHERE table_schema = $1 AND table_name = $2 \
             ORDER BY ordinal_position",
        )
        .bind(database)
        .bind(table)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Query(e.to_string()))?;

        // 主鍵欄位集合（標記 key=PRI）
        let pk = self.primary_key(database, table).await?;

        Ok(col_rows
            .iter()
            .map(|r| {
                let name: String = r.try_get(0).unwrap_or_default();
                let key = if pk.contains(&name) { "PRI" } else { "" };
                ColumnInfo {
                    name: name.clone(),
                    data_type: r.try_get(1).unwrap_or_default(),
                    nullable: r
                        .try_get::<String, _>(2)
                        .map(|v| v.eq_ignore_ascii_case("YES"))
                        .unwrap_or(false),
                    key: key.to_string(),
                    default: r.try_get(3).ok(),
                    extra: String::new(),
                }
            })
            .collect())
    }

    async fn table_data(
        &self,
        database: &str,
        table: &str,
        query: &DataQuery,
    ) -> AppResult<PagedData> {
        let q_tbl = format!("{}.{}", quote_ident(database), quote_ident(table));
        let offset = query.page.saturating_mul(query.page_size);
        let page_size = query.page_size;

        // PG 用 $N；從 $1 起編號。
        let (where_sql, bind_values) = build_where(&query.filters, 1, query.match_any)?;
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
            primary_key: self.primary_key(database, table).await?,
        })
    }

    async fn query(&self, sql: &str) -> AppResult<QueryResult> {
        let trimmed = sql.trim_start().to_ascii_lowercase();
        let is_read = trimmed.starts_with("select")
            || trimmed.starts_with("show")
            || trimmed.starts_with("explain")
            || trimmed.starts_with("table")
            || trimmed.starts_with("with");

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

    async fn update_cell(
        &self,
        database: &str,
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

        let q_tbl = format!("{}.{}", quote_ident(database), quote_ident(table));
        let q_col = quote_ident(&edit.column);

        // PG 用 $1, $2… 佔位符。$1 為新值，主鍵從 $2 起。
        let where_clause = edit
            .pk_columns
            .iter()
            .enumerate()
            .map(|(i, c)| format!("{} = ${}", quote_ident(c), i + 2))
            .collect::<Vec<_>>()
            .join(" AND ");

        let sql = format!("UPDATE {q_tbl} SET {q_col} = $1 WHERE {where_clause}");
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
        let q_tbl = format!("{}.{}", quote_ident(database), quote_ident(table));
        let cols = row
            .columns
            .iter()
            .map(|c| quote_ident(c))
            .collect::<Vec<_>>()
            .join(", ");
        let placeholders = (1..=row.values.len())
            .map(|i| format!("${i}"))
            .collect::<Vec<_>>()
            .join(", ");
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
        let q_tbl = format!("{}.{}", quote_ident(database), quote_ident(table));
        let where_clause = del
            .pk_columns
            .iter()
            .enumerate()
            .map(|(i, c)| format!("{} = ${}", quote_ident(c), i + 1))
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

    fn pool_status(&self) -> PoolStatus {
        let size = self.pool.size();
        let idle = self.pool.num_idle() as u32;
        PoolStatus {
            size,
            idle,
            in_use: size.saturating_sub(idle),
        }
    }

    async fn explain(&self, sql: &str) -> AppResult<QueryResult> {
        let rows = sqlx::query(&format!("EXPLAIN {sql}"))
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Query(e.to_string()))?;
        Ok(rows_to_result(&rows))
    }

    async fn alter_table(&self, database: &str, table: &str, op: &AlterOp) -> AppResult<()> {
        let q_tbl = format!("{}.{}", quote_ident(database), quote_ident(table));
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
        let fk_rows = sqlx::query(
            "SELECT tc.table_name, kcu.column_name, ccu.table_name, ccu.column_name \
             FROM information_schema.table_constraints tc \
             JOIN information_schema.key_column_usage kcu \
               ON tc.constraint_name = kcu.constraint_name AND tc.table_schema = kcu.table_schema \
             JOIN information_schema.constraint_column_usage ccu \
               ON ccu.constraint_name = tc.constraint_name AND ccu.table_schema = tc.table_schema \
             WHERE tc.constraint_type = 'FOREIGN KEY' AND tc.table_schema = $1",
        )
        .bind(database)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Query(e.to_string()))?;
        let (relations, fk_cols) =
            collect_relations(&fk_rows, |r, i| r.try_get::<String, _>(i).ok());
        let mut tables = Vec::new();
        for t in self.list_tables(database).await? {
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
            tables.push(ErTable { name: t.name, columns: er_cols });
        }
        Ok(ErModel { tables, relations })
    }

    async fn close(&self) {
        self.pool.close().await;
    }
}

impl PostgresDriver {
    async fn primary_key(&self, schema: &str, table: &str) -> AppResult<Vec<String>> {
        // 以 to_regclass 安全解析 schema.table（傳完整限定名字串）。
        let qualified = format!("{}.{}", quote_ident(schema), quote_ident(table));
        let sql = "SELECT a.attname \
                   FROM pg_index i \
                   JOIN pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = ANY(i.indkey) \
                   WHERE i.indrelid = to_regclass($1) AND i.indisprimary \
                   ORDER BY array_position(i.indkey, a.attnum)";
        let rows = sqlx::query(sql)
            .bind(qualified)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Query(e.to_string()))?;
        Ok(rows
            .iter()
            .filter_map(|r| r.try_get::<String, _>(0).ok())
            .collect())
    }
}

/// PG 識別字以雙引號包裹，轉義內部雙引號。
fn quote_ident(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}

/// 組 WHERE 子句（PG `$N` 佔位符，從 start_idx 起編號）。
fn build_where(
    filters: &[Filter],
    start_idx: usize,
    match_any: bool,
) -> AppResult<(String, Vec<Option<String>>)> {
    if filters.is_empty() {
        return Ok((String::new(), vec![]));
    }
    let mut clauses = Vec::new();
    let mut binds = Vec::new();
    let mut idx = start_idx;
    for f in filters {
        let op = filter_op_sql(&f.op)
            .ok_or_else(|| AppError::Query(format!("不支援的運算子：{}", f.op)))?;
        let col = quote_ident(&f.column);
        if op_needs_value(&f.op) {
            clauses.push(format!("{col} {op} ${idx}"));
            binds.push(f.value.clone());
            idx += 1;
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

fn rows_to_result(rows: &[PgRow]) -> QueryResult {
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

fn cell_to_string(row: &PgRow, idx: usize) -> Option<String> {
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
        "INT2" | "SMALLINT" => row.try_get::<i16, _>(idx).ok().map(|v| v.to_string())
            .or_else(|| string_fallback(row, idx)),
        "INT4" | "INT" | "INTEGER" => row.try_get::<i32, _>(idx).ok().map(|v| v.to_string())
            .or_else(|| string_fallback(row, idx)),
        "INT8" | "BIGINT" => row.try_get::<i64, _>(idx).ok().map(|v| v.to_string())
            .or_else(|| string_fallback(row, idx)),
        "FLOAT4" | "REAL" => row.try_get::<f32, _>(idx).ok().map(|v| v.to_string())
            .or_else(|| string_fallback(row, idx)),
        "FLOAT8" | "DOUBLE PRECISION" => row.try_get::<f64, _>(idx).ok().map(|v| v.to_string())
            .or_else(|| string_fallback(row, idx)),
        "BOOL" | "BOOLEAN" => row.try_get::<bool, _>(idx).ok().map(|v| v.to_string())
            .or_else(|| string_fallback(row, idx)),
        _ => string_fallback(row, idx),
    }
}

fn string_fallback(row: &PgRow, idx: usize) -> Option<String> {
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
    if let Ok(v) = row.try_get::<uuid::Uuid, _>(idx) {
        return Some(v.to_string());
    }
    if let Ok(v) = row.try_get::<Vec<u8>, _>(idx) {
        return Some(String::from_utf8_lossy(&v).into_owned());
    }
    Some("<unrenderable>".to_string())
}
