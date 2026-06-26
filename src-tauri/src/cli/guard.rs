//! 唯讀 SQL 守門。`query` / `explain` 送 manager 前先過此檢查，擋下非查詢語句，
//! 兌現「只查詢不刪除」。逐句（`;` 切）取首個有效關鍵字（跳過前導註解），
//! 只放行查詢類語句。建議搭配唯讀 DB 帳號作第二道防線。

use crate::error::{AppError, AppResult};

/// 允許的語句開頭關鍵字（皆為唯讀 / 查詢類）。
const ALLOWED: &[&str] = &[
    "select", "with", "show", "describe", "desc", "explain", "pragma", "use", "values", "table",
];

pub fn ensure_read_only(sql: &str) -> AppResult<()> {
    for stmt in sql.split(';') {
        let kw = first_keyword(stmt);
        if kw.is_empty() {
            continue; // 空句 / 純註解
        }
        if !ALLOWED.contains(&kw.as_str()) {
            return Err(AppError::Query(format!(
                "CLI 為唯讀模式，僅允許查詢語句（偵測到 `{kw}`）"
            )));
        }
    }
    Ok(())
}

/// 取語句的第一個關鍵字（小寫），跳過前導空白與行 / 區塊註解。
fn first_keyword(stmt: &str) -> String {
    let mut s = stmt.trim_start();
    loop {
        if let Some(rest) = s.strip_prefix("--") {
            match rest.find('\n') {
                Some(i) => s = rest[i + 1..].trim_start(),
                None => return String::new(),
            }
        } else if let Some(rest) = s.strip_prefix("/*") {
            match rest.find("*/") {
                Some(i) => s = rest[i + 2..].trim_start(),
                None => return String::new(),
            }
        } else {
            break;
        }
    }
    s.split(|c: char| c.is_whitespace() || c == '(')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::ensure_read_only;

    #[test]
    fn allows_select_and_friends() {
        assert!(ensure_read_only("select * from t").is_ok());
        assert!(ensure_read_only("  SELECT 1").is_ok());
        assert!(ensure_read_only("WITH x AS (select 1) select * from x").is_ok());
        assert!(ensure_read_only("show tables").is_ok());
        assert!(ensure_read_only("-- c\nselect 1").is_ok());
        assert!(ensure_read_only("/* c */ explain select 1").is_ok());
    }

    #[test]
    fn blocks_writes() {
        assert!(ensure_read_only("delete from t").is_err());
        assert!(ensure_read_only("update t set x=1").is_err());
        assert!(ensure_read_only("insert into t values (1)").is_err());
        assert!(ensure_read_only("drop table t").is_err());
        assert!(ensure_read_only("select 1; delete from t").is_err());
    }
}
