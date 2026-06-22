use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;

use crate::db::{ConnectionConfig, DbKind};
use crate::error::{AppError, AppResult};

/// 備份格式 / 方式。對應到各資料庫的官方工具或內建匯出。
#[derive(Debug, Clone, Copy)]
pub enum BackupMethod {
    /// 使用官方 CLI（mysqldump / pg_dump / mongodump / redis-cli --rdb）
    Cli,
    /// 內建邏輯匯出（無外部工具時的後備；目前 SQLite 採檔案複製，其餘待補）
    Builtin,
}

/// 偵測某資料庫對應的 CLI 工具是否存在於 PATH。
pub async fn detect_cli(kind: DbKind) -> bool {
    let tool = cli_tool_name(kind);
    if tool.is_empty() {
        return false;
    }
    // 跨平台：用 `<tool> --version` 是否能執行來判斷。
    Command::new(tool)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false)
}

fn cli_tool_name(kind: DbKind) -> &'static str {
    match kind {
        DbKind::Mysql => "mysqldump",
        DbKind::Postgres => "pg_dump",
        DbKind::Mongo => "mongodump",
        DbKind::Redis => "redis-cli",
        DbKind::Sqlite => "", // SQLite 用檔案複製，無需 CLI
    }
}

/// 備份結果摘要。
#[derive(Debug, serde::Serialize)]
pub struct BackupResult {
    pub path: String,
    pub bytes: u64,
    pub method: String,
}

/// 執行備份，輸出到 out_path。
///
/// 策略：優先 CLI；SQLite 一律檔案複製；其餘無 CLI 時目前回明確錯誤
/// （提示使用者安裝對應工具），內建邏輯匯出待後續補完。
pub async fn backup(
    config: &ConnectionConfig,
    database: &str,
    out_path: &str,
) -> AppResult<BackupResult> {
    // SQLite：直接複製資料庫檔案。
    if let DbKind::Sqlite = config.kind {
        let src = config
            .database
            .clone()
            .filter(|p| !p.is_empty())
            .ok_or_else(|| AppError::Query("SQLite 連線未指定檔案路徑".to_string()))?;
        tokio::fs::copy(&src, out_path)
            .await
            .map_err(|e| AppError::Query(format!("複製 SQLite 檔失敗：{e}")))?;
        let bytes = file_size(out_path).await;
        return Ok(BackupResult {
            path: out_path.to_string(),
            bytes,
            method: "file-copy".to_string(),
        });
    }

    // 其餘：需要對應 CLI。
    if !detect_cli(config.kind).await {
        return Err(AppError::Query(format!(
            "找不到 {} 工具，請先安裝後再備份（或改用支援內建匯出的格式）",
            cli_tool_name(config.kind)
        )));
    }

    let status = match config.kind {
        DbKind::Mysql => run_mysqldump(config, database, out_path).await?,
        DbKind::Postgres => run_pg_dump(config, database, out_path).await?,
        DbKind::Mongo => run_mongodump(config, database, out_path).await?,
        DbKind::Redis => run_redis_dump(config, out_path).await?,
        DbKind::Sqlite => unreachable!(),
    };

    if !status {
        return Err(AppError::Query("備份指令執行失敗，請檢查連線與權限".to_string()));
    }
    let bytes = file_size(out_path).await;
    Ok(BackupResult {
        path: out_path.to_string(),
        bytes,
        method: "cli".to_string(),
    })
}

/// 還原。SQLite 為檔案複製回去；MySQL/PG 以 CLI 匯入 SQL；
/// Mongo 用 mongorestore；Redis 還原較特殊，此版本先不支援自動還原。
pub async fn restore(
    config: &ConnectionConfig,
    database: &str,
    in_path: &str,
) -> AppResult<()> {
    if !Path::new(in_path).exists() {
        return Err(AppError::Query("備份檔不存在".to_string()));
    }

    match config.kind {
        DbKind::Sqlite => {
            let dst = config
                .database
                .clone()
                .filter(|p| !p.is_empty())
                .ok_or_else(|| AppError::Query("SQLite 連線未指定檔案路徑".to_string()))?;
            tokio::fs::copy(in_path, &dst)
                .await
                .map_err(|e| AppError::Query(format!("還原 SQLite 檔失敗：{e}")))?;
            Ok(())
        }
        DbKind::Mysql => {
            if !detect_cli(DbKind::Mysql).await {
                // 還原用 mysql client（非 mysqldump）
            }
            let ok = run_mysql_restore(config, database, in_path).await?;
            if ok { Ok(()) } else { Err(AppError::Query("MySQL 還原失敗".to_string())) }
        }
        DbKind::Postgres => {
            let ok = run_psql_restore(config, database, in_path).await?;
            if ok { Ok(()) } else { Err(AppError::Query("PostgreSQL 還原失敗".to_string())) }
        }
        DbKind::Mongo => {
            if !detect_cli(DbKind::Mongo).await {
                return Err(AppError::Query("找不到 mongorestore，請先安裝".to_string()));
            }
            let ok = run_mongorestore(config, database, in_path).await?;
            if ok { Ok(()) } else { Err(AppError::Query("MongoDB 還原失敗".to_string())) }
        }
        DbKind::Redis => Err(AppError::Query(
            "Redis 自動還原暫未支援；請以 redis-cli 手動匯入 RDB".to_string(),
        )),
    }
}

// ---- 各 CLI 指令組裝 ----

async fn run_mysqldump(c: &ConnectionConfig, db: &str, out: &str) -> AppResult<bool> {
    // 密碼以環境變數 MYSQL_PWD 傳遞，避免出現在行程列表。
    let file = std::fs::File::create(out)
        .map_err(|e| AppError::Query(format!("建立輸出檔失敗：{e}")))?;
    let status = Command::new("mysqldump")
        .arg("-h").arg(&c.host)
        .arg("-P").arg(c.port.to_string())
        .arg("-u").arg(&c.username)
        .arg("--single-transaction")
        .arg(db)
        .env("MYSQL_PWD", &c.password)
        .stdout(Stdio::from(file))
        .stderr(Stdio::null())
        .status()
        .await
        .map_err(|e| AppError::Query(e.to_string()))?;
    Ok(status.success())
}

async fn run_mysql_restore(c: &ConnectionConfig, db: &str, inp: &str) -> AppResult<bool> {
    let file = std::fs::File::open(inp)
        .map_err(|e| AppError::Query(format!("開啟備份檔失敗：{e}")))?;
    let status = Command::new("mysql")
        .arg("-h").arg(&c.host)
        .arg("-P").arg(c.port.to_string())
        .arg("-u").arg(&c.username)
        .arg(db)
        .env("MYSQL_PWD", &c.password)
        .stdin(Stdio::from(file))
        .stderr(Stdio::null())
        .status()
        .await
        .map_err(|e| AppError::Query(e.to_string()))?;
    Ok(status.success())
}

async fn run_pg_dump(c: &ConnectionConfig, db: &str, out: &str) -> AppResult<bool> {
    // 密碼以 PGPASSWORD 傳遞。
    let status = Command::new("pg_dump")
        .arg("-h").arg(&c.host)
        .arg("-p").arg(c.port.to_string())
        .arg("-U").arg(&c.username)
        .arg("-d").arg(db)
        .arg("-f").arg(out)
        .env("PGPASSWORD", &c.password)
        .stderr(Stdio::null())
        .status()
        .await
        .map_err(|e| AppError::Query(e.to_string()))?;
    Ok(status.success())
}

async fn run_psql_restore(c: &ConnectionConfig, db: &str, inp: &str) -> AppResult<bool> {
    if !detect_cli_named("psql").await {
        return Err(AppError::Query("找不到 psql，請先安裝".to_string()));
    }
    let status = Command::new("psql")
        .arg("-h").arg(&c.host)
        .arg("-p").arg(c.port.to_string())
        .arg("-U").arg(&c.username)
        .arg("-d").arg(db)
        .arg("-f").arg(inp)
        .env("PGPASSWORD", &c.password)
        .stderr(Stdio::null())
        .status()
        .await
        .map_err(|e| AppError::Query(e.to_string()))?;
    Ok(status.success())
}

async fn run_mongodump(c: &ConnectionConfig, db: &str, out: &str) -> AppResult<bool> {
    // mongodump 以 --archive 輸出單一檔。
    let auth = if c.username.is_empty() {
        String::new()
    } else {
        format!("{}:{}@", c.username, c.password)
    };
    let uri = format!("mongodb://{auth}{}:{}", c.host, c.port);
    let status = Command::new("mongodump")
        .arg("--uri").arg(uri)
        .arg("--db").arg(db)
        .arg(format!("--archive={out}"))
        .stderr(Stdio::null())
        .status()
        .await
        .map_err(|e| AppError::Query(e.to_string()))?;
    Ok(status.success())
}

async fn run_mongorestore(c: &ConnectionConfig, db: &str, inp: &str) -> AppResult<bool> {
    let auth = if c.username.is_empty() {
        String::new()
    } else {
        format!("{}:{}@", c.username, c.password)
    };
    let uri = format!("mongodb://{auth}{}:{}", c.host, c.port);
    let status = Command::new("mongorestore")
        .arg("--uri").arg(uri)
        .arg("--nsInclude").arg(format!("{db}.*"))
        .arg(format!("--archive={inp}"))
        .stderr(Stdio::null())
        .status()
        .await
        .map_err(|e| AppError::Query(e.to_string()))?;
    Ok(status.success())
}

async fn run_redis_dump(c: &ConnectionConfig, out: &str) -> AppResult<bool> {
    // redis-cli --rdb <file> 會請求伺服器產生 RDB 並下載。
    let mut cmd = Command::new("redis-cli");
    cmd.arg("-h").arg(&c.host).arg("-p").arg(c.port.to_string());
    if !c.password.is_empty() {
        cmd.arg("-a").arg(&c.password);
    }
    cmd.arg("--rdb").arg(out);
    let status = cmd
        .stderr(Stdio::null())
        .status()
        .await
        .map_err(|e| AppError::Query(e.to_string()))?;
    Ok(status.success())
}

async fn detect_cli_named(tool: &str) -> bool {
    Command::new(tool)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false)
}

async fn file_size(path: &str) -> u64 {
    tokio::fs::metadata(path).await.map(|m| m.len()).unwrap_or(0)
}
