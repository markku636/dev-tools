//! 資料匯出（Navicat 風格的多格式選項）。
//!
//! 尊重目前的篩選 / 排序 / AND·OR，將表格資料匯出成 CSV / TSV / JSON / SQL / Markdown。
//! 透過分頁逐批向 driver 取資料（避免一次撈爆），組好後一次寫檔。

use serde::{Deserialize, Serialize};

use crate::db::DataQuery;
use crate::error::{AppError, AppResult};
use crate::manager::ConnectionManager;

/// 匯出選項。
#[derive(Debug, Deserialize)]
pub struct ExportOptions {
    /// csv | tsv | json | sql | markdown
    pub format: String,
    #[serde(default = "yes")]
    pub include_header: bool,
    /// CSV/TSV 自訂分隔字元；未指定則 csv=","、tsv="\t"。
    #[serde(default)]
    pub delimiter: Option<String>,
    /// NULL 在 CSV/TSV 的呈現（預設空字串）。
    #[serde(default)]
    pub null_text: Option<String>,
    /// SQL 匯出的目標表名（預設用來源表名）。
    #[serde(default)]
    pub sql_table: Option<String>,
    /// true = 匯出全部符合列；false = 只匯出目前這一頁。
    #[serde(default = "yes")]
    pub all_rows: bool,
    /// 在檔首寫 UTF-8 BOM（方便 Excel 開 CSV）。
    #[serde(default)]
    pub bom: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Serialize)]
pub struct ExportResult {
    pub path: String,
    pub rows: u64,
    pub bytes: u64,
    pub format: String,
}

/// 一次取多少列。
const PAGE_SIZE: u32 = 2000;
/// 安全上限，避免極大表把記憶體撐爆。
const MAX_ROWS: usize = 1_000_000;

pub async fn export(
    manager: &ConnectionManager,
    id: &str,
    database: &str,
    table: &str,
    query: &DataQuery,
    opts: &ExportOptions,
    out_path: &str,
) -> AppResult<ExportResult> {
    let mut columns: Vec<String> = Vec::new();
    let mut rows: Vec<Vec<Option<String>>> = Vec::new();
    let mut page: u32 = 0;

    loop {
        let q = DataQuery {
            page,
            page_size: PAGE_SIZE,
            filters: query.filters.clone(),
            sorts: query.sorts.clone(),
            match_any: query.match_any,
        };
        let pd = manager.table_data(id, database, table, &q).await?;
        if page == 0 {
            columns = pd.columns.clone();
        }
        let fetched = pd.rows.len();
        rows.extend(pd.rows);

        if !opts.all_rows {
            break;
        }
        if fetched < PAGE_SIZE as usize {
            break;
        }
        if rows.len() >= MAX_ROWS {
            break;
        }
        if (rows.len() as u64) >= pd.total_rows {
            break;
        }
        page += 1;
    }

    let bytes = render(&columns, &rows, opts, table)?;
    tokio::fs::write(out_path, &bytes)
        .await
        .map_err(|e| AppError::Query(format!("寫入檔案失敗：{e}")))?;

    Ok(ExportResult {
        path: out_path.to_string(),
        rows: rows.len() as u64,
        bytes: bytes.len() as u64,
        format: opts.format.clone(),
    })
}

fn render(
    columns: &[String],
    rows: &[Vec<Option<String>>],
    opts: &ExportOptions,
    table: &str,
) -> AppResult<Vec<u8>> {
    match opts.format.as_str() {
        "csv" | "tsv" => {
            let delim = opts
                .delimiter
                .clone()
                .unwrap_or_else(|| if opts.format == "tsv" { "\t".into() } else { ",".into() });
            let nullt = opts.null_text.clone().unwrap_or_default();
            let mut out = String::new();
            if opts.bom {
                out.push('\u{FEFF}');
            }
            if opts.include_header {
                out.push_str(
                    &columns.iter().map(|c| csv_field(c, &delim)).collect::<Vec<_>>().join(&delim),
                );
                out.push_str("\r\n");
            }
            for row in rows {
                let line = row
                    .iter()
                    .map(|v| match v {
                        Some(s) => csv_field(s, &delim),
                        None => nullt.clone(),
                    })
                    .collect::<Vec<_>>()
                    .join(&delim);
                out.push_str(&line);
                out.push_str("\r\n");
            }
            Ok(out.into_bytes())
        }
        "json" => {
            let arr: Vec<serde_json::Map<String, serde_json::Value>> = rows
                .iter()
                .map(|row| {
                    let mut m = serde_json::Map::new();
                    for (i, c) in columns.iter().enumerate() {
                        let v = row.get(i).and_then(|x| x.clone());
                        m.insert(
                            c.clone(),
                            v.map(serde_json::Value::String).unwrap_or(serde_json::Value::Null),
                        );
                    }
                    m
                })
                .collect();
            serde_json::to_vec_pretty(&arr).map_err(|e| AppError::Query(e.to_string()))
        }
        "sql" => {
            let tbl = opts.sql_table.clone().unwrap_or_else(|| table.to_string());
            let qtbl = format!("`{}`", tbl.replace('`', "``"));
            let collist = columns
                .iter()
                .map(|c| format!("`{}`", c.replace('`', "``")))
                .collect::<Vec<_>>()
                .join(", ");
            let mut out = String::new();
            for row in rows {
                let vals = row
                    .iter()
                    .map(|v| match v {
                        Some(s) => sql_quote(s),
                        None => "NULL".to_string(),
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                out.push_str(&format!("INSERT INTO {qtbl} ({collist}) VALUES ({vals});\n"));
            }
            Ok(out.into_bytes())
        }
        "markdown" => {
            let mut out = String::new();
            out.push_str("| ");
            out.push_str(&columns.iter().map(|c| md_cell(c)).collect::<Vec<_>>().join(" | "));
            out.push_str(" |\n| ");
            out.push_str(&columns.iter().map(|_| "---").collect::<Vec<_>>().join(" | "));
            out.push_str(" |\n");
            for row in rows {
                out.push_str("| ");
                out.push_str(
                    &row.iter()
                        .map(|v| match v {
                            Some(s) => md_cell(s),
                            None => String::new(),
                        })
                        .collect::<Vec<_>>()
                        .join(" | "),
                );
                out.push_str(" |\n");
            }
            Ok(out.into_bytes())
        }
        other => Err(AppError::Query(format!("不支援的匯出格式：{other}"))),
    }
}

fn csv_field(s: &str, delim: &str) -> String {
    if s.contains(delim) || s.contains('"') || s.contains('\n') || s.contains('\r') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn sql_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

fn md_cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\r', "").replace('\n', " ")
}
