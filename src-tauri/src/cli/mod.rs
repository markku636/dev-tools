//! db-kit 命令列介面（唯讀查詢 + 匯出）。
//!
//! 直接重用核心層（`manager` / `store` / `export` / `backup` / `conn_crypto`），
//! 不經過 Tauri commands，因此可在 `--no-default-features`（無 gui）下單獨編譯。
//! 對資料庫唯讀：只開放查詢 / 瀏覽 / 匯出，`query` 另過唯讀守門（見 `guard`）。

mod args;
mod dispatch;
mod guard;
mod render;
mod resolve;

use std::process::ExitCode;

use clap::Parser;

/// CLI 進入點。bin shim 在 tokio runtime 上 `block_on` 此函式。
/// 錯誤印到 stderr 並回非零 exit code。
pub async fn run() -> ExitCode {
    let cli = args::Cli::parse();
    match dispatch::dispatch(cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
