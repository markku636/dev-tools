//! 唯讀 SQL 守門。`query` / `explain` 送 manager 前先過此檢查，擋下非查詢語句，
//! 兌現「只查詢不刪除」。逐句（`;` 切）取首個有效關鍵字（跳過前導註解），
//! 只放行查詢類語句。建議搭配唯讀 DB 帳號作第二道防線。

use crate::error::{AppError, AppResult};

/// 允許的語句開頭關鍵字（皆為唯讀 / 查詢類）。
const ALLOWED: &[&str] = &[
    "select", "with", "show", "describe", "desc", "explain", "pragma", "use", "values", "table",
];

/// 可寫 CTE 偵測用的寫入關鍵字。
const CTE_WRITE: &[&str] = &["insert", "update", "delete", "merge"];

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
        // PostgreSQL 可寫 CTE：`WITH x AS (DELETE …) …` 首關鍵字為 with（被允許），
        // 但實際會改資料。含寫入關鍵字即擋下（寧可多擋）。
        if kw == "with" {
            let lower = stmt.to_ascii_lowercase();
            if let Some(w) = CTE_WRITE.iter().find(|w| contains_keyword(&lower, w)) {
                return Err(AppError::Query(format!(
                    "CLI 為唯讀模式，偵測到可寫 CTE（含 `{w}`）"
                )));
            }
        }
    }
    Ok(())
}

/// 以「字界」判斷 haystack（已小寫）是否含關鍵字 word（避免 `deleted_at` 誤判為 `delete`）。
fn contains_keyword(haystack: &str, word: &str) -> bool {
    let bytes = haystack.as_bytes();
    let is_word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let mut start = 0;
    while let Some(pos) = haystack[start..].find(word) {
        let i = start + pos;
        let before_ok = i == 0 || !is_word(bytes[i - 1]);
        let after = i + word.len();
        let after_ok = after >= bytes.len() || !is_word(bytes[after]);
        if before_ok && after_ok {
            return true;
        }
        start = i + 1;
    }
    false
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

    #[test]
    fn blocks_writable_cte_but_allows_readonly_cte() {
        // 可寫 CTE 應被擋（首關鍵字 with 雖被允許，但實際改資料）。
        assert!(ensure_read_only("WITH d AS (DELETE FROM t RETURNING *) SELECT * FROM d").is_err());
        assert!(ensure_read_only("with u as (UPDATE t SET a=1 RETURNING id) select * from u").is_err());
        assert!(ensure_read_only("WITH i AS (INSERT INTO t VALUES (1) RETURNING *) SELECT * FROM i").is_err());
        // 唯讀 CTE 照常放行；欄名含 delete 字根（deleted_at）不誤判。
        assert!(ensure_read_only("WITH x AS (SELECT deleted_at FROM t) SELECT * FROM x").is_ok());
    }
}
