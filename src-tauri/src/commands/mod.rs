use std::sync::Arc;

use parking_lot::Mutex;
use tauri::{AppHandle, State};

use crate::backup::{self, BackupResult};
use crate::db::{
    AlterOp, CellEdit, ColumnInfo, ConnectionConfig, DataQuery, ErModel, KeyDetail, KeyEdit,
    PagedData, PoolStatus, QueryResult, RowDelete, RowInsert, TableInfo,
};
use crate::error::{AppError, AppResult};
use crate::manager::ConnectionManager;
use crate::scheduler::{self, BackupHistoryEntry, BackupSchedule, BackupStatus};
use crate::store::{self, PersistedConnection};

pub struct AppState {
    pub manager: ConnectionManager,
    /// 排程清單的執行時權威副本（背景迴圈每 tick 讀取；命令變更後持久化）。
    pub schedules: Arc<Mutex<Vec<BackupSchedule>>>,
    /// 序列化 history.json 的讀-改-寫（避免排程 append 與 clear_history 競態）。
    pub history_lock: Arc<tokio::sync::Mutex<()>>,
}

/// 若前端送來的 secret 為空（存檔但未重新輸入的連線），從 keychain 補回。
/// 剛輸入的新密碼非空 → 跳過，向後相容。
fn hydrate_secrets(config: &mut ConnectionConfig) {
    if config.password.is_empty() {
        config.password = store::kc_get(&config.id).unwrap_or_default();
    }
    if config.ssh_enabled {
        if config.ssh_password.is_empty() {
            config.ssh_password = store::kc_get(&store::ssh_account(&config.id)).unwrap_or_default();
        }
        if config.ssh_passphrase.is_empty() {
            config.ssh_passphrase =
                store::kc_get(&store::ssh_passphrase_account(&config.id)).unwrap_or_default();
        }
    }
}

#[tauri::command]
pub async fn test_connection(
    state: State<'_, AppState>,
    config: ConnectionConfig,
) -> AppResult<()> {
    let mut config = config;
    hydrate_secrets(&mut config);
    state.manager.test(&config).await
}

#[tauri::command]
pub async fn connect(
    state: State<'_, AppState>,
    config: ConnectionConfig,
) -> AppResult<()> {
    let mut config = config;
    hydrate_secrets(&mut config);
    state.manager.connect(config).await
}

// ---- 連線設定持久化 ----

#[tauri::command]
pub async fn list_saved_connections(app: AppHandle) -> AppResult<Vec<PersistedConnection>> {
    store::load_all(&app).await
}

#[tauri::command]
pub async fn save_connection(app: AppHandle, config: ConnectionConfig) -> AppResult<()> {
    // secret 進 keychain。空字串 = 不變動（編輯連線時未重新輸入密碼則保留舊的）。
    if !config.password.is_empty() {
        store::kc_set(&config.id, &config.password)?;
    }
    if config.ssh_enabled {
        if !config.ssh_password.is_empty() {
            store::kc_set(&store::ssh_account(&config.id), &config.ssh_password)?;
        }
        if !config.ssh_passphrase.is_empty() {
            store::kc_set(
                &store::ssh_passphrase_account(&config.id),
                &config.ssh_passphrase,
            )?;
        }
    }
    store::upsert(&app, PersistedConnection::from(&config)).await
}

#[tauri::command]
pub async fn remove_saved_connection(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> AppResult<()> {
    state.manager.disconnect(&id).await;
    store::remove(&app, &id).await?;
    store::kc_delete(&id);
    store::kc_delete(&store::ssh_account(&id));
    store::kc_delete(&store::ssh_passphrase_account(&id));
    Ok(())
}

#[tauri::command]
pub async fn disconnect(state: State<'_, AppState>, id: String) -> AppResult<()> {
    state.manager.disconnect(&id).await;
    Ok(())
}

#[tauri::command]
pub async fn list_databases(
    state: State<'_, AppState>,
    id: String,
) -> AppResult<Vec<String>> {
    state.manager.list_databases(&id).await
}

#[tauri::command]
pub async fn list_tables(
    state: State<'_, AppState>,
    id: String,
    database: String,
) -> AppResult<Vec<TableInfo>> {
    state.manager.list_tables(&id, &database).await
}

#[tauri::command]
pub async fn table_columns(
    state: State<'_, AppState>,
    id: String,
    database: String,
    table: String,
) -> AppResult<Vec<ColumnInfo>> {
    state.manager.table_columns(&id, &database, &table).await
}

#[tauri::command]
pub async fn table_data(
    state: State<'_, AppState>,
    id: String,
    database: String,
    table: String,
    query: DataQuery,
) -> AppResult<PagedData> {
    state.manager.table_data(&id, &database, &table, &query).await
}

#[tauri::command]
pub async fn run_query(
    state: State<'_, AppState>,
    id: String,
    sql: String,
) -> AppResult<QueryResult> {
    state.manager.query(&id, &sql).await
}

#[tauri::command]
pub async fn update_cell(
    state: State<'_, AppState>,
    id: String,
    database: String,
    table: String,
    edit: CellEdit,
) -> AppResult<u64> {
    state.manager.update_cell(&id, &database, &table, &edit).await
}

#[tauri::command]
pub async fn insert_row(
    state: State<'_, AppState>,
    id: String,
    database: String,
    table: String,
    row: RowInsert,
) -> AppResult<u64> {
    state.manager.insert_row(&id, &database, &table, &row).await
}

#[tauri::command]
pub async fn delete_row(
    state: State<'_, AppState>,
    id: String,
    database: String,
    table: String,
    del: RowDelete,
) -> AppResult<u64> {
    state.manager.delete_row(&id, &database, &table, &del).await
}

#[tauri::command]
pub async fn pool_status(state: State<'_, AppState>, id: String) -> AppResult<PoolStatus> {
    state.manager.pool_status(&id)
}

#[tauri::command]
pub async fn key_detail(
    state: State<'_, AppState>,
    id: String,
    database: String,
    key: String,
) -> AppResult<Option<KeyDetail>> {
    state.manager.key_detail(&id, &database, &key).await
}

#[tauri::command]
pub async fn backup_detect_cli(kind: crate::db::DbKind) -> AppResult<bool> {
    Ok(backup::detect_cli(kind).await)
}

#[tauri::command]
pub async fn backup_run(
    config: ConnectionConfig,
    database: String,
    out_path: String,
) -> AppResult<BackupResult> {
    let mut config = config;
    hydrate_secrets(&mut config);
    backup::backup(&config, &database, &out_path).await
}

#[tauri::command]
pub async fn backup_restore(
    config: ConnectionConfig,
    database: String,
    in_path: String,
) -> AppResult<()> {
    let mut config = config;
    hydrate_secrets(&mut config);
    backup::restore(&config, &database, &in_path).await
}

// ---- 查詢效能分析 / 結構編輯 / ER 圖 ----

#[tauri::command]
pub async fn explain_query(
    state: State<'_, AppState>,
    id: String,
    sql: String,
) -> AppResult<QueryResult> {
    state.manager.explain(&id, &sql).await
}

#[tauri::command]
pub async fn alter_table(
    state: State<'_, AppState>,
    id: String,
    database: String,
    table: String,
    op: AlterOp,
) -> AppResult<()> {
    state.manager.alter_table(&id, &database, &table, &op).await
}

#[tauri::command]
pub async fn er_model(
    state: State<'_, AppState>,
    id: String,
    database: String,
) -> AppResult<ErModel> {
    state.manager.er_model(&id, &database).await
}

// ---- 資料匯出 ----

#[tauri::command]
pub async fn export_table(
    state: State<'_, AppState>,
    id: String,
    database: String,
    table: String,
    query: DataQuery,
    options: crate::export::ExportOptions,
    out_path: String,
) -> AppResult<crate::export::ExportResult> {
    crate::export::export(&state.manager, &id, &database, &table, &query, &options, &out_path).await
}

// ---- Redis 鍵結構編輯 ----

#[tauri::command]
pub async fn key_edit(
    state: State<'_, AppState>,
    id: String,
    database: String,
    key: String,
    edit: KeyEdit,
) -> AppResult<u64> {
    state.manager.key_edit(&id, &database, &key, &edit).await
}

// ---- 排程備份 + 備份歷史 ----

#[tauri::command]
pub async fn list_schedules(state: State<'_, AppState>) -> AppResult<Vec<BackupSchedule>> {
    Ok(state.schedules.lock().clone())
}

#[tauri::command]
pub async fn save_schedule(
    app: AppHandle,
    state: State<'_, AppState>,
    schedule: BackupSchedule,
) -> AppResult<BackupSchedule> {
    let mut sched = schedule;
    sched.next_run = scheduler::compute_next_run(&sched.cadence, chrono::Local::now());
    let snapshot = {
        let mut g = state.schedules.lock();
        g.retain(|s| s.id != sched.id);
        g.push(sched.clone());
        g.clone()
    };
    store::write_json(&app, scheduler::SCHEDULES_FILE, &snapshot).await?;
    Ok(sched)
}

#[tauri::command]
pub async fn remove_schedule(
    app: AppHandle,
    state: State<'_, AppState>,
    schedule_id: String,
) -> AppResult<()> {
    let snapshot = {
        let mut g = state.schedules.lock();
        g.retain(|s| s.id != schedule_id);
        g.clone()
    };
    store::write_json(&app, scheduler::SCHEDULES_FILE, &snapshot).await
}

#[tauri::command]
pub async fn toggle_schedule(
    app: AppHandle,
    state: State<'_, AppState>,
    schedule_id: String,
    enabled: bool,
) -> AppResult<BackupSchedule> {
    let (snapshot, updated) = {
        let mut g = state.schedules.lock();
        let idx = g
            .iter()
            .position(|s| s.id == schedule_id)
            .ok_or_else(|| AppError::NotFound(schedule_id.clone()))?;
        let next = if enabled {
            scheduler::compute_next_run(&g[idx].cadence, chrono::Local::now())
        } else {
            g[idx].next_run
        };
        g[idx].enabled = enabled;
        g[idx].next_run = next;
        let updated = g[idx].clone();
        (g.clone(), updated)
    };
    store::write_json(&app, scheduler::SCHEDULES_FILE, &snapshot).await?;
    Ok(updated)
}

#[tauri::command]
pub async fn run_schedule_now(
    app: AppHandle,
    state: State<'_, AppState>,
    schedule_id: String,
) -> AppResult<BackupHistoryEntry> {
    let sched = state
        .schedules
        .lock()
        .iter()
        .find(|s| s.id == schedule_id)
        .cloned()
        .ok_or_else(|| AppError::NotFound(schedule_id))?;
    Ok(scheduler::fire_one(&app, &sched).await)
}

#[tauri::command]
pub async fn list_backup_history(app: AppHandle) -> AppResult<Vec<BackupHistoryEntry>> {
    store::read_json(&app, scheduler::HISTORY_FILE).await
}

#[tauri::command]
pub async fn restore_from_history(app: AppHandle, entry_id: String) -> AppResult<()> {
    let hist: Vec<BackupHistoryEntry> =
        store::read_json(&app, scheduler::HISTORY_FILE).await?;
    let entry = hist
        .into_iter()
        .find(|e| e.id == entry_id)
        .ok_or_else(|| AppError::NotFound(entry_id))?;
    if entry.status != BackupStatus::Ok {
        return Err(AppError::Query("此筆為失敗紀錄，無法還原".into()));
    }
    let cfg = store::load_connection(&app, &entry.connection_id).await?;
    backup::restore(&cfg, &entry.database, &entry.path).await
}

#[tauri::command]
pub async fn clear_history(app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    let _g = state.history_lock.lock().await;
    store::write_json(&app, scheduler::HISTORY_FILE, &Vec::<BackupHistoryEntry>::new()).await
}
