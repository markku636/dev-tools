//! 整合驗證測試（針對 Docker 真實資料庫）。
//!
//! - 純函式測試（scheduler / 持久化序列化）正常 `cargo test` 即跑。
//! - 需要 Docker 服務的測試標 `#[ignore]`，以 `cargo test -- --ignored` 執行。
//!   先起容器：MySQL:13306 / PostgreSQL:15432 / MongoDB:27018 / Redis:16379。
#![cfg(test)]

use std::time::Duration;

use crate::db::mongo::MongoDriver;
use crate::db::mysql::MysqlDriver;
use crate::db::postgres::PostgresDriver;
use crate::db::redis::RedisDriver;
use crate::db::sqlite::SqliteDriver;
use crate::db::{
    CellEdit, ConnectionConfig, DataQuery, DatabaseDriver, DbKind, Filter, KeyEdit, RowDelete,
    RowInsert, Sort, SortDir, SshAuthMethod,
};

fn cfg(kind: DbKind, host: &str, port: u16, user: &str, pass: &str, db: Option<&str>) -> ConnectionConfig {
    ConnectionConfig {
        id: "it".into(),
        name: "it".into(),
        kind,
        host: host.into(),
        port,
        username: user.into(),
        password: pass.into(),
        database: db.map(|s| s.to_string()),
        max_connections: 5,
        ssh_enabled: false,
        ssh_host: String::new(),
        ssh_port: 0,
        ssh_username: String::new(),
        ssh_auth_method: SshAuthMethod::Password,
        ssh_password: String::new(),
        ssh_private_key_path: String::new(),
        ssh_passphrase: String::new(),
    }
}

fn filt(col: &str, op: &str, val: Option<&str>) -> Filter {
    Filter { column: col.into(), op: op.into(), value: val.map(|s| s.to_string()) }
}
fn dq(filters: Vec<Filter>, sorts: Vec<Sort>) -> DataQuery {
    DataQuery { page: 0, page_size: 100, filters, sorts, match_any: false }
}
fn ins(cols: &[&str], vals: &[&str]) -> RowInsert {
    RowInsert {
        columns: cols.iter().map(|s| s.to_string()).collect(),
        values: vals.iter().map(|s| Some(s.to_string())).collect(),
    }
}
fn sv(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}
fn col_at(cols: &[String], name: &str) -> usize {
    cols.iter().position(|c| c == name).unwrap_or_else(|| panic!("找不到欄位 {name}"))
}

// ============================ 純函式 ============================

#[test]
fn scheduler_next_run() {
    use crate::scheduler::{compute_next_run, Cadence};
    use chrono::{Duration as Cd, Local, TimeZone, Timelike};

    let now = Local.with_ymd_and_hms(2026, 6, 22, 10, 0, 0).unwrap();

    let n = compute_next_run(&Cadence::EveryMinutes { minutes: 30 }, now).unwrap();
    assert_eq!(n, now + Cd::minutes(30));

    let n = compute_next_run(&Cadence::EveryHours { hours: 2 }, now).unwrap();
    assert_eq!(n, now + Cd::hours(2));

    // 今天稍晚 → 當天
    let n = compute_next_run(&Cadence::DailyAt { hour: 15, minute: 30 }, now).unwrap();
    assert_eq!((n.hour(), n.minute()), (15, 30));
    assert_eq!(n.date_naive(), now.date_naive());

    // 今天較早 → 隔天
    let n = compute_next_run(&Cadence::DailyAt { hour: 9, minute: 0 }, now).unwrap();
    assert_eq!(n.date_naive(), now.date_naive() + Cd::days(1));
}

#[test]
fn persisted_connection_drops_secrets() {
    let mut c = cfg(DbKind::Mysql, "h", 3306, "u", "PW_dbsecret", Some("db"));
    c.ssh_enabled = true;
    c.ssh_host = "bastion".into();
    c.ssh_password = "PW_sshsecret".into();
    c.ssh_passphrase = "PW_phsecret".into();

    let p = crate::store::PersistedConnection::from(&c);
    let json = serde_json::to_string(&p).unwrap();
    // 磁碟格式不得含任何 secret
    assert!(!json.contains("PW_"), "persisted json 不應含 secret：{json}");
    assert!(json.contains("bastion"), "非 secret 的 ssh_host 應保留");

    let back = p.to_config();
    assert_eq!(back.password, "");
    assert_eq!(back.ssh_password, "");
    assert_eq!(back.ssh_passphrase, "");
    assert!(back.ssh_enabled);
    assert_eq!(back.ssh_host, "bastion");
}

// ============================ SQLite（本機檔案）============================

#[tokio::test]
async fn sqlite_crud_and_backup() {
    let dbfile = "atkit_it_test.db";
    let bakfile = "atkit_it_test.bak";
    let _ = std::fs::remove_file(dbfile);
    let _ = std::fs::remove_file(bakfile);
    let c = cfg(DbKind::Sqlite, "", 0, "", "", Some(dbfile));

    {
        let d = SqliteDriver::connect(&c).await.unwrap();
        d.query("CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT)").await.unwrap();
        d.insert_row("main", "t", &ins(&["id", "name"], &["1", "a"])).await.unwrap();
        d.insert_row("main", "t", &ins(&["id", "name"], &["2", "b"])).await.unwrap();
        assert_eq!(d.table_data("main", "t", &dq(vec![], vec![])).await.unwrap().total_rows, 2);
        d.close().await;
    }

    // 備份（檔案複製）
    let res = crate::backup::backup(&c, "main", bakfile).await.unwrap();
    assert_eq!(res.method, "file-copy");
    assert!(res.bytes > 0);

    // 刪一列
    {
        let d = SqliteDriver::connect(&c).await.unwrap();
        d.delete_row("main", "t", &RowDelete { pk_columns: sv(&["id"]), pk_values: vec![Some("1".into())] })
            .await
            .unwrap();
        assert_eq!(d.table_data("main", "t", &dq(vec![], vec![])).await.unwrap().total_rows, 1);
        d.close().await;
    }

    // 還原 → 回到 2 列
    crate::backup::restore(&c, "main", bakfile).await.unwrap();
    {
        let d = SqliteDriver::connect(&c).await.unwrap();
        assert_eq!(d.table_data("main", "t", &dq(vec![], vec![])).await.unwrap().total_rows, 2);
        d.close().await;
    }

    let _ = std::fs::remove_file(dbfile);
    let _ = std::fs::remove_file(bakfile);
}

// ============================ MySQL（Docker）============================

async fn connect_retry_mysql() -> MysqlDriver {
    let c = cfg(DbKind::Mysql, "127.0.0.1", 13306, "root", "test1234", Some("testdb"));
    for i in 0..60 {
        match MysqlDriver::connect(&c).await {
            Ok(d) => return d,
            Err(e) => {
                if i == 59 {
                    panic!("MySQL 連線失敗：{e:?}");
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
    unreachable!()
}

#[tokio::test]
#[ignore = "需要 Docker MySQL:13306"]
async fn mysql_full() {
    let d = connect_retry_mysql().await;
    d.ping().await.unwrap();
    d.query("DROP TABLE IF EXISTS t").await.unwrap();
    d.query("CREATE TABLE t (id VARCHAR(8) PRIMARY KEY, a VARCHAR(8), b VARCHAR(50))").await.unwrap();
    for (id, a, b) in [("1", "2", "xy"), ("2", "5", "xb"), ("3", "9", "xa"), ("4", "1", "qq")] {
        d.insert_row("testdb", "t", &ins(&["id", "a", "b"], &[id, a, b])).await.unwrap();
    }

    // 多欄複合篩選 AND：a >= "3" 且 b LIKE "x%" → 期望 id 2、3
    let pd = d
        .table_data(
            "testdb",
            "t",
            &dq(
                vec![filt("a", ">=", Some("3")), filt("b", "like", Some("x%"))],
                vec![Sort { column: "id".into(), dir: SortDir::Asc }],
            ),
        )
        .await
        .unwrap();
    assert_eq!(pd.total_rows, 2, "多欄 AND 篩選應命中 2 列");
    assert_eq!(pd.primary_key, sv(&["id"]));
    let i = col_at(&pd.columns, "id");
    let ids: Vec<&str> = pd.rows.iter().map(|r| r[i].as_deref().unwrap()).collect();
    assert_eq!(ids, vec!["2", "3"]);

    // update
    d.update_cell("testdb", "t", &CellEdit {
        column: "b".into(),
        new_value: Some("XX".into()),
        pk_columns: sv(&["id"]),
        pk_values: vec![Some("3".into())],
    }).await.unwrap();
    let pd2 = d.table_data("testdb", "t", &dq(vec![filt("id", "=", Some("3"))], vec![])).await.unwrap();
    assert_eq!(pd2.rows[0][col_at(&pd2.columns, "b")].as_deref(), Some("XX"));

    // delete
    d.delete_row("testdb", "t", &RowDelete { pk_columns: sv(&["id"]), pk_values: vec![Some("4".into())] })
        .await
        .unwrap();
    assert_eq!(d.table_data("testdb", "t", &dq(vec![], vec![])).await.unwrap().total_rows, 3);

    // metadata
    let dbs = d.list_databases().await.unwrap();
    assert!(dbs.contains(&"testdb".to_string()), "list_databases 未含 testdb，實得：{dbs:?}");
    assert!(d.list_tables("testdb").await.unwrap().iter().any(|t| t.name == "t"));
    assert!(d.table_columns("testdb", "t").await.unwrap().iter().any(|c| c.name == "a"));
    d.close().await;
}

// ============================ PostgreSQL（Docker）============================

async fn connect_retry_pg() -> PostgresDriver {
    let c = cfg(DbKind::Postgres, "127.0.0.1", 15432, "postgres", "test1234", Some("testdb"));
    for i in 0..60 {
        match PostgresDriver::connect(&c).await {
            Ok(d) => return d,
            Err(e) => {
                if i == 59 {
                    panic!("PostgreSQL 連線失敗：{e:?}");
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
    unreachable!()
}

#[tokio::test]
#[ignore = "需要 Docker PostgreSQL:15432"]
async fn postgres_full() {
    let d = connect_retry_pg().await;
    d.ping().await.unwrap();
    d.query("DROP TABLE IF EXISTS t").await.unwrap();
    d.query("CREATE TABLE t (id TEXT PRIMARY KEY, a TEXT, b TEXT)").await.unwrap();
    for (id, a, b) in [("1", "2", "xy"), ("2", "5", "xb"), ("3", "9", "xa"), ("4", "1", "qq")] {
        d.insert_row("public", "t", &ins(&["id", "a", "b"], &[id, a, b])).await.unwrap();
    }

    // 多欄複合篩選 AND（$N 佔位符）：a >= "3" 且 b LIKE "x%" → id 2、3
    let pd = d
        .table_data(
            "public",
            "t",
            &dq(
                vec![filt("a", ">=", Some("3")), filt("b", "like", Some("x%"))],
                vec![Sort { column: "id".into(), dir: SortDir::Asc }],
            ),
        )
        .await
        .unwrap();
    assert_eq!(pd.total_rows, 2, "PG 多欄 AND 篩選應命中 2 列");
    assert_eq!(pd.primary_key, sv(&["id"]));
    let i = col_at(&pd.columns, "id");
    let ids: Vec<&str> = pd.rows.iter().map(|r| r[i].as_deref().unwrap()).collect();
    assert_eq!(ids, vec!["2", "3"]);

    d.update_cell("public", "t", &CellEdit {
        column: "b".into(),
        new_value: Some("XX".into()),
        pk_columns: sv(&["id"]),
        pk_values: vec![Some("3".into())],
    }).await.unwrap();
    let pd2 = d.table_data("public", "t", &dq(vec![filt("id", "=", Some("3"))], vec![])).await.unwrap();
    assert_eq!(pd2.rows[0][col_at(&pd2.columns, "b")].as_deref(), Some("XX"));

    d.delete_row("public", "t", &RowDelete { pk_columns: sv(&["id"]), pk_values: vec![Some("4".into())] })
        .await
        .unwrap();
    assert_eq!(d.table_data("public", "t", &dq(vec![], vec![])).await.unwrap().total_rows, 3);

    assert!(d.list_databases().await.unwrap().contains(&"public".to_string()));
    assert!(d.list_tables("public").await.unwrap().iter().any(|t| t.name == "t"));
    d.close().await;
}

// ============================ Redis（Docker）— 重點：結構編輯 ============================

#[tokio::test]
#[ignore = "需要 Docker Redis:16379"]
async fn redis_full() {
    let c = cfg(DbKind::Redis, "127.0.0.1", 16379, "", "", None);
    let d = RedisDriver::connect(&c).await.unwrap();
    d.query("0:FLUSHDB").await.unwrap();

    // string：insert_row(SET) → key_detail → update_cell（值 / TTL）
    d.insert_row("0", "keys", &ins(&["key", "value"], &["s1", "hello"])).await.unwrap();
    let det = d.key_detail("0", "s1").await.unwrap().unwrap();
    assert_eq!(det.type_, "string");
    assert_eq!(det.entries.first().map(String::as_str), Some("hello"));
    d.update_cell("0", "keys", &CellEdit {
        column: "value".into(),
        new_value: Some("world".into()),
        pk_columns: sv(&["key"]),
        pk_values: vec![Some("s1".into())],
    }).await.unwrap();
    assert_eq!(d.key_detail("0", "s1").await.unwrap().unwrap().entries[0], "world");
    d.update_cell("0", "keys", &CellEdit {
        column: "ttl".into(),
        new_value: Some("100".into()),
        pk_columns: sv(&["key"]),
        pk_values: vec![Some("s1".into())],
    }).await.unwrap();
    let ttl = d.key_detail("0", "s1").await.unwrap().unwrap().ttl;
    assert!(ttl > 0 && ttl <= 100, "TTL 應被設定，實得 {ttl}");

    // hash
    d.key_edit("0", "h1", &KeyEdit::HashSet { field: "f1".into(), value: "v1".into() }).await.unwrap();
    d.key_edit("0", "h1", &KeyEdit::HashSet { field: "f2".into(), value: "v2".into() }).await.unwrap();
    assert_eq!(d.key_detail("0", "h1").await.unwrap().unwrap().fields.len(), 2);
    d.key_edit("0", "h1", &KeyEdit::HashRemove { field: "f1".into() }).await.unwrap();
    let h = d.key_detail("0", "h1").await.unwrap().unwrap();
    assert_eq!(h.fields, sv(&["f2"]));
    assert_eq!(h.entries, sv(&["v2"]));

    // list（LPUSH/RPUSH/LSET/LREM）
    d.key_edit("0", "l1", &KeyEdit::ListPush { value: "a".into(), front: false }).await.unwrap(); // [a]
    d.key_edit("0", "l1", &KeyEdit::ListPush { value: "b".into(), front: false }).await.unwrap(); // [a,b]
    d.key_edit("0", "l1", &KeyEdit::ListPush { value: "z".into(), front: true }).await.unwrap();   // [z,a,b]
    d.key_edit("0", "l1", &KeyEdit::ListSet { index: 1, value: "A".into() }).await.unwrap();        // [z,A,b]
    assert_eq!(d.key_detail("0", "l1").await.unwrap().unwrap().entries, sv(&["z", "A", "b"]));
    d.key_edit("0", "l1", &KeyEdit::ListRemove { value: "A".into(), count: 1 }).await.unwrap();      // [z,b]
    assert_eq!(d.key_detail("0", "l1").await.unwrap().unwrap().entries, sv(&["z", "b"]));

    // set（SADD/SREM）
    d.key_edit("0", "set1", &KeyEdit::SetAdd { member: "m1".into() }).await.unwrap();
    d.key_edit("0", "set1", &KeyEdit::SetAdd { member: "m2".into() }).await.unwrap();
    let mut members = d.key_detail("0", "set1").await.unwrap().unwrap().entries;
    members.sort();
    assert_eq!(members, sv(&["m1", "m2"]));
    d.key_edit("0", "set1", &KeyEdit::SetRemove { member: "m1".into() }).await.unwrap();
    assert_eq!(d.key_detail("0", "set1").await.unwrap().unwrap().entries, sv(&["m2"]));

    // zset（ZADD/ZREM，含分數）
    d.key_edit("0", "zs1", &KeyEdit::ZsetAdd { member: "z1".into(), score: 1.5 }).await.unwrap();
    d.key_edit("0", "zs1", &KeyEdit::ZsetAdd { member: "z2".into(), score: 2.5 }).await.unwrap();
    let z = d.key_detail("0", "zs1").await.unwrap().unwrap();
    assert_eq!(z.type_, "zset");
    assert_eq!(z.entries, sv(&["z1", "z2"])); // 依分數升冪
    assert_eq!(z.scores, vec![1.5, 2.5]);
    d.key_edit("0", "zs1", &KeyEdit::ZsetRemove { member: "z1".into() }).await.unwrap();
    assert_eq!(d.key_detail("0", "zs1").await.unwrap().unwrap().entries, sv(&["z2"]));

    // 移除最後一個元素 → key 消失（TYPE = none）
    d.key_edit("0", "zs1", &KeyEdit::ZsetRemove { member: "z2".into() }).await.unwrap();
    assert_eq!(d.key_detail("0", "zs1").await.unwrap().unwrap().type_, "none");

    d.close().await;
}

// ============================ MongoDB（Docker）============================

async fn connect_retry_mongo() -> MongoDriver {
    let c = cfg(DbKind::Mongo, "127.0.0.1", 27018, "", "", Some("testdb"));
    for i in 0..60 {
        match MongoDriver::connect(&c).await {
            Ok(d) => return d,
            Err(e) => {
                if i == 59 {
                    panic!("MongoDB 連線失敗：{e:?}");
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
    unreachable!()
}

#[tokio::test]
#[ignore = "需要 Docker MongoDB:27018"]
async fn mongo_full() {
    let d = connect_retry_mongo().await;
    d.ping().await.unwrap();
    // 每次跑用獨立 collection，避免殘留資料干擾。
    let coll = format!("people_{}", std::process::id());

    for (name, age, city) in [("alice", "30", "x"), ("bob", "25", "y"), ("carol", "40", "x")] {
        d.insert_row("testdb", &coll, &ins(&["name", "age", "city"], &[name, age, city])).await.unwrap();
    }

    // 多欄複合篩選 AND：age >= 30 且 city = "x" → alice、carol
    let pd = d
        .table_data(
            "testdb",
            &coll,
            &dq(vec![filt("age", ">=", Some("30")), filt("city", "=", Some("x"))], vec![]),
        )
        .await
        .unwrap();
    assert_eq!(pd.rows.len(), 2, "Mongo 多欄 AND 篩選應命中 2 筆");
    assert_eq!(pd.primary_key, sv(&["_id"]));

    // 取一筆 _id 做 update / delete
    let idi = col_at(&pd.columns, "_id");
    let namei = col_at(&pd.columns, "name");
    let alice = pd.rows.iter().find(|r| r[namei].as_deref() == Some("alice")).unwrap();
    let alice_id = alice[idi].clone().unwrap();

    d.update_cell("testdb", &coll, &CellEdit {
        column: "city".into(),
        new_value: Some("z".into()),
        pk_columns: sv(&["_id"]),
        pk_values: vec![Some(alice_id.clone())],
    }).await.unwrap();
    let pd2 = d.table_data("testdb", &coll, &dq(vec![filt("city", "=", Some("z"))], vec![])).await.unwrap();
    assert_eq!(pd2.rows.len(), 1);

    d.delete_row("testdb", &coll, &RowDelete { pk_columns: sv(&["_id"]), pk_values: vec![Some(alice_id)] })
        .await
        .unwrap();
    let pd3 = d.table_data("testdb", &coll, &dq(vec![], vec![])).await.unwrap();
    assert_eq!(pd3.rows.len(), 2);

    d.close().await;
}
