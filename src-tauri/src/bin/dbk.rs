//! db-kit CLI 入口（唯讀查詢 + 匯出）。
//!
//! 刻意不設 `#![windows_subsystem = "windows"]`（與 GUI 的 main.rs 不同），
//! 以保留 console；在 tokio 多執行緒 runtime 上跑 `cli::run`。

use std::process::ExitCode;

fn main() -> ExitCode {
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("error: 無法建立 tokio runtime：{e}");
            return ExitCode::FAILURE;
        }
    };
    rt.block_on(db_kit_lib::cli::run())
}
