use futures::stream::TryStreamExt;
use mongodb::bson::{doc, Bson, Document};
use mongodb::options::{ClientOptions, FindOptions};
use mongodb::Client;
use std::time::Duration;

use crate::db::{
    CellEdit, ColumnInfo, ConnectionConfig, DataQuery, DatabaseDriver, Filter, PagedData,
    PoolStatus, QueryResult, RowDelete, RowInsert, Sort, SortDir, TableInfo,
};
use crate::error::{AppError, AppResult};

/// MongoDB 驅動。文件型，但盡量沿用 Navicat 表格手感：
/// - list_databases → Mongo 資料庫
/// - list_tables → 集合（kind = "collection"）
/// - table_data → 取一批文件，聯集頂層欄位攤平成表格；巢狀值以 JSON 字串呈現
/// - 主鍵固定為 _id
/// - update_cell / insert_row / delete_row → 以 _id 定位的文件操作
///
/// mongodb crate 的 Client 內建連線池（maxPoolSize），故無需自管池。
pub struct MongoDriver {
    client: Client,
    /// 連線時指定的預設資料庫（list_databases 仍會列全部）。
    default_db: Option<String>,
}

impl MongoDriver {
    fn db_handle(&self, database: &str) -> mongodb::Database {
        self.client.database(database)
    }
}

#[async_trait::async_trait]
impl DatabaseDriver for MongoDriver {
    async fn connect(config: &ConnectionConfig) -> AppResult<Self> {
        // 組 mongodb URI。有帳密則帶上。
        let auth = if config.username.is_empty() {
            String::new()
        } else {
            format!("{}:{}@", config.username, config.password)
        };
        let uri = format!(
            "mongodb://{auth}{host}:{port}",
            host = config.host,
            port = config.port,
        );

        let mut opts = ClientOptions::parse(&uri)
            .await
            .map_err(|e| AppError::Connect(e.to_string()))?;
        opts.max_pool_size = Some(config.max_connections.max(1));
        opts.connect_timeout = Some(Duration::from_secs(10));
        opts.server_selection_timeout = Some(Duration::from_secs(10));

        let client =
            Client::with_options(opts).map_err(|e| AppError::Connect(e.to_string()))?;

        let driver = Self {
            client,
            default_db: config.database.clone().filter(|d| !d.is_empty()),
        };
        driver.ping().await?;
        Ok(driver)
    }

    async fn ping(&self) -> AppResult<()> {
        // 對 admin 跑 ping 指令。
        self.client
            .database("admin")
            .run_command(doc! { "ping": 1 })
            .await
            .map(|_| ())
            .map_err(|e| AppError::Connect(e.to_string()))
    }

    async fn list_databases(&self) -> AppResult<Vec<String>> {
        let names = self
            .client
            .list_database_names()
            .await
            .map_err(|e| AppError::Query(e.to_string()))?;
        // 若指定了預設庫且不在清單（權限不足列全部時），仍補上。
        let mut out = names;
        if let Some(d) = &self.default_db {
            if !out.contains(d) {
                out.insert(0, d.clone());
            }
        }
        Ok(out)
    }

    async fn list_tables(&self, database: &str) -> AppResult<Vec<TableInfo>> {
        let names = self
            .db_handle(database)
            .list_collection_names()
            .await
            .map_err(|e| AppError::Query(e.to_string()))?;
        Ok(names
            .into_iter()
            .map(|name| TableInfo {
                name,
                kind: "collection".to_string(),
            })
            .collect())
    }

    async fn table_columns(
        &self,
        database: &str,
        table: &str,
    ) -> AppResult<Vec<ColumnInfo>> {
        // 無固定 schema：抽樣若干文件，推斷頂層欄位與其 BSON 型別。
        let coll = self.db_handle(database).collection::<Document>(table);
        let opts = FindOptions::builder().limit(50).build();
        let mut cursor = coll
            .find(doc! {})
            .with_options(opts)
            .await
            .map_err(|e| AppError::Query(e.to_string()))?;

        // 保留首次出現順序
        let mut order: Vec<String> = Vec::new();
        let mut types: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        while let Some(d) = cursor
            .try_next()
            .await
            .map_err(|e| AppError::Query(e.to_string()))?
        {
            for (k, v) in &d {
                if !types.contains_key(k) {
                    order.push(k.clone());
                }
                types.insert(k.clone(), bson_type_name(v));
            }
        }

        Ok(order
            .into_iter()
            .map(|name| {
                let is_id = name == "_id";
                ColumnInfo {
                    data_type: types.get(&name).cloned().unwrap_or_default(),
                    key: if is_id { "PRI".to_string() } else { String::new() },
                    nullable: !is_id,
                    default: None,
                    extra: String::new(),
                    name,
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
        let coll = self.db_handle(database).collection::<Document>(table);

        let filter = build_filter(&query.filters, query.match_any);
        let sort = build_sort(&query.sorts);

        // 總數（套用相同 filter）
        let total = coll
            .count_documents(filter.clone())
            .await
            .map_err(|e| AppError::Query(e.to_string()))?;

        let skip = (query.page as u64) * (query.page_size as u64);
        let mut find_opts = FindOptions::builder()
            .skip(skip)
            .limit(query.page_size as i64)
            .build();
        if let Some(s) = sort {
            find_opts.sort = Some(s);
        }

        let mut cursor = coll
            .find(filter)
            .with_options(find_opts)
            .await
            .map_err(|e| AppError::Query(e.to_string()))?;

        let mut docs: Vec<Document> = Vec::new();
        while let Some(d) = cursor
            .try_next()
            .await
            .map_err(|e| AppError::Query(e.to_string()))?
        {
            docs.push(d);
        }

        // 聯集頂層欄位為欄；_id 永遠擺第一欄。
        let mut columns: Vec<String> = Vec::new();
        columns.push("_id".to_string());
        for d in &docs {
            for (k, _) in d {
                if k != "_id" && !columns.contains(k) {
                    columns.push(k.clone());
                }
            }
        }

        let rows: Vec<Vec<Option<String>>> = docs
            .iter()
            .map(|d| {
                columns
                    .iter()
                    .map(|col| d.get(col).map(bson_to_string))
                    .collect()
            })
            .collect();

        Ok(PagedData {
            columns,
            rows,
            total_rows: total,
            page: query.page,
            page_size: query.page_size,
            primary_key: vec!["_id".to_string()],
        })
    }

    async fn query(&self, sql: &str) -> AppResult<QueryResult> {
        // MongoDB 無 SQL。此處接受一個 JSON：{"db","collection","filter"}，
        // 回傳符合的文件（每列一個 JSON 字串）。其餘語法在後續擴充。
        let parsed: serde_json::Value = serde_json::from_str(sql)
            .map_err(|_| AppError::Query(
                "MongoDB 查詢請提供 JSON：{\"db\":\"..\",\"collection\":\"..\",\"filter\":{}}".to_string(),
            ))?;
        let db = parsed.get("db").and_then(|v| v.as_str())
            .ok_or_else(|| AppError::Query("缺少 db".to_string()))?;
        let coll_name = parsed.get("collection").and_then(|v| v.as_str())
            .ok_or_else(|| AppError::Query("缺少 collection".to_string()))?;
        let filter_doc = match parsed.get("filter") {
            Some(f) => bson_from_json(f),
            None => Bson::Document(Document::new()),
        };
        let filter = match filter_doc {
            Bson::Document(d) => d,
            _ => Document::new(),
        };

        let coll = self.db_handle(db).collection::<Document>(coll_name);
        let mut cursor = coll
            .find(filter)
            .await
            .map_err(|e| AppError::Query(e.to_string()))?;
        let mut rows = Vec::new();
        while let Some(d) = cursor
            .try_next()
            .await
            .map_err(|e| AppError::Query(e.to_string()))?
        {
            let json = serde_json::to_string(&d).unwrap_or_default();
            rows.push(vec![Some(json)]);
        }
        Ok(QueryResult {
            columns: vec!["document".to_string()],
            rows,
            rows_affected: 0,
        })
    }

    async fn update_cell(
        &self,
        database: &str,
        table: &str,
        edit: &CellEdit,
    ) -> AppResult<u64> {
        // 以 _id 定位文件，設定單一欄位。
        let id_value = id_filter(edit)?;
        let coll = self.db_handle(database).collection::<Document>(table);
        // 新值：null 代表設為 BSON null（Mongo 沒有「移除欄位」與「設 null」之別，這裡採設 null）
        let new_bson = match &edit.new_value {
            Some(s) => guess_bson(s),
            None => Bson::Null,
        };
        let mut set_doc = Document::new();
        set_doc.insert(edit.column.clone(), new_bson);
        let res = coll
            .update_one(id_value, doc! { "$set": set_doc })
            .await
            .map_err(|e| AppError::Query(e.to_string()))?;
        Ok(res.modified_count)
    }

    async fn insert_row(
        &self,
        database: &str,
        table: &str,
        row: &RowInsert,
    ) -> AppResult<u64> {
        if row.columns.len() != row.values.len() {
            return Err(AppError::Query("欄位與值數量不符".to_string()));
        }
        let mut d = Document::new();
        for (c, v) in row.columns.iter().zip(row.values.iter()) {
            // _id 留空則由 Mongo 自動產生（不放入文件）
            if c == "_id" && v.as_ref().map(|s| s.is_empty()).unwrap_or(true) {
                continue;
            }
            let b = match v {
                Some(s) => guess_bson(s),
                None => Bson::Null,
            };
            d.insert(c.clone(), b);
        }
        let coll = self.db_handle(database).collection::<Document>(table);
        coll.insert_one(d)
            .await
            .map_err(|e| AppError::Query(e.to_string()))?;
        Ok(1)
    }

    async fn delete_row(
        &self,
        database: &str,
        table: &str,
        del: &RowDelete,
    ) -> AppResult<u64> {
        // 從 RowDelete 取 _id。
        let idx = del
            .pk_columns
            .iter()
            .position(|c| c == "_id")
            .ok_or_else(|| AppError::Query("缺少 _id，無法刪除".to_string()))?;
        let raw = del.pk_values.get(idx).and_then(|v| v.clone())
            .ok_or_else(|| AppError::Query("_id 為空，無法刪除".to_string()))?;
        let filter = doc! { "_id": id_bson(&raw) };
        let coll = self.db_handle(database).collection::<Document>(table);
        let res = coll
            .delete_one(filter)
            .await
            .map_err(|e| AppError::Query(e.to_string()))?;
        Ok(res.deleted_count)
    }

    fn pool_status(&self) -> PoolStatus {
        // mongodb crate 未公開即時池統計，回傳 0（介面相容用）。
        PoolStatus { size: 0, idle: 0, in_use: 0 }
    }

    async fn close(&self) {
        // mongodb Client 於 drop 時自行清理連線；無顯式 close。
    }
}

/// 從 CellEdit 取 _id 組成 filter。
fn id_filter(edit: &CellEdit) -> AppResult<Document> {
    let idx = edit
        .pk_columns
        .iter()
        .position(|c| c == "_id")
        .ok_or_else(|| AppError::Query("缺少 _id，無法定位文件".to_string()))?;
    let raw = edit.pk_values.get(idx).and_then(|v| v.clone())
        .ok_or_else(|| AppError::Query("_id 為空".to_string()))?;
    Ok(doc! { "_id": id_bson(&raw) })
}

/// 將字串 _id 還原成 BSON。ObjectId 字串（24 hex）轉 ObjectId，否則當字串。
fn id_bson(raw: &str) -> Bson {
    if raw.len() == 24 && raw.chars().all(|c| c.is_ascii_hexdigit()) {
        if let Ok(oid) = mongodb::bson::oid::ObjectId::parse_str(raw) {
            return Bson::ObjectId(oid);
        }
    }
    Bson::String(raw.to_string())
}

/// 把使用者輸入的字串猜測成適當 BSON：數字 / bool / 其餘為字串。
fn guess_bson(s: &str) -> Bson {
    if let Ok(i) = s.parse::<i64>() {
        return Bson::Int64(i);
    }
    if let Ok(f) = s.parse::<f64>() {
        return Bson::Double(f);
    }
    match s {
        "true" => return Bson::Boolean(true),
        "false" => return Bson::Boolean(false),
        _ => {}
    }
    Bson::String(s.to_string())
}

/// BSON 值轉成表格顯示字串。巢狀物件/陣列以精簡 JSON 呈現。
fn bson_to_string(b: &Bson) -> String {
    match b {
        Bson::String(s) => s.clone(),
        Bson::Int32(i) => i.to_string(),
        Bson::Int64(i) => i.to_string(),
        Bson::Double(f) => f.to_string(),
        Bson::Boolean(v) => v.to_string(),
        Bson::ObjectId(o) => o.to_hex(),
        Bson::Null => "null".to_string(),
        Bson::DateTime(dt) => dt.try_to_rfc3339_string().unwrap_or_else(|_| format!("{dt:?}")),
        other => {
            // 物件、陣列等以 JSON 呈現
            serde_json::to_string(other).unwrap_or_else(|_| format!("{other:?}"))
        }
    }
}

fn bson_type_name(b: &Bson) -> String {
    match b {
        Bson::String(_) => "string",
        Bson::Int32(_) => "int32",
        Bson::Int64(_) => "int64",
        Bson::Double(_) => "double",
        Bson::Boolean(_) => "bool",
        Bson::ObjectId(_) => "objectId",
        Bson::Document(_) => "object",
        Bson::Array(_) => "array",
        Bson::DateTime(_) => "date",
        Bson::Null => "null",
        _ => "mixed",
    }
    .to_string()
}

/// 把 DataQuery 的篩選轉成 Mongo filter document。
/// 支援運算子對應到 Mongo 比較運算子；like → 正規表示式（不分大小寫）。
/// match_any=true 時以 $or 串接（否則合併成單一 doc = AND）。
fn build_filter(filters: &[Filter], match_any: bool) -> Document {
    let mut clauses: Vec<Document> = Vec::new();
    for f in filters {
        let field = f.column.clone();
        let mut d = Document::new();
        match f.op.as_str() {
            "=" => { d.insert(field, value_bson(&f.value)); }
            "!=" => { d.insert(field, doc! { "$ne": value_bson(&f.value) }); }
            ">" => { d.insert(field, doc! { "$gt": value_bson(&f.value) }); }
            ">=" => { d.insert(field, doc! { "$gte": value_bson(&f.value) }); }
            "<" => { d.insert(field, doc! { "$lt": value_bson(&f.value) }); }
            "<=" => { d.insert(field, doc! { "$lte": value_bson(&f.value) }); }
            "like" => {
                // 將 SQL like 的 % 轉成 .*；簡化處理
                let pat = f.value.clone().unwrap_or_default()
                    .replace('%', ".*");
                d.insert(field, doc! { "$regex": pat, "$options": "i" });
            }
            "is_null" => { d.insert(field, Bson::Null); }
            "is_not_null" => { d.insert(field, doc! { "$ne": Bson::Null }); }
            _ => {}
        }
        if !d.is_empty() {
            clauses.push(d);
        }
    }
    if clauses.is_empty() {
        Document::new()
    } else if match_any {
        doc! { "$or": clauses.into_iter().map(Bson::Document).collect::<Vec<_>>() }
    } else {
        let mut merged = Document::new();
        for c in clauses {
            for (k, v) in c {
                merged.insert(k, v);
            }
        }
        merged
    }
}

fn value_bson(v: &Option<String>) -> Bson {
    match v {
        Some(s) => guess_bson(s),
        None => Bson::Null,
    }
}

/// 排序轉成 Mongo sort document（1 / -1）。
fn build_sort(sorts: &[Sort]) -> Option<Document> {
    if sorts.is_empty() {
        return None;
    }
    let mut d = Document::new();
    for s in sorts {
        let dir = match s.dir {
            SortDir::Asc => 1,
            SortDir::Desc => -1,
        };
        d.insert(s.column.clone(), dir);
    }
    Some(d)
}

/// 將 serde_json::Value 轉成 BSON（供 query 的 filter 用）。
fn bson_from_json(v: &serde_json::Value) -> Bson {
    match mongodb::bson::to_bson(v) {
        Ok(b) => b,
        Err(_) => Bson::Null,
    }
}
