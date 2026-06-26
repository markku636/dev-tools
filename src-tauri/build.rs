fn main() {
    // 只有 GUI build 需要 Tauri context / 嵌入前端 dist（generate_context!）。
    // CLI（--no-default-features，無 gui feature）跳過，避免要求 ../dist 存在。
    if std::env::var_os("CARGO_FEATURE_GUI").is_some() {
        tauri_build::build();
    }
}
