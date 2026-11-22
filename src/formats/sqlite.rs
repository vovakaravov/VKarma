use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct SqliteTableInfo {
    pub name: String,
    pub kind: String,
    pub row_count: Option<u64>,
    pub sql: String,
}

#[derive(Debug, Clone)]
pub struct SqliteColumnInfo {
    pub name: String,
    pub col_type: String,
    pub not_null: bool,
    pub pk: bool,
}

#[derive(Debug, Clone)]
pub struct SqliteQueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Option<String>>>,
    pub duration_ms: u128,
}

#[derive(Debug, Clone)]
pub struct SqliteMetadata {
    pub tables: Vec<SqliteTableInfo>,
}

pub fn quote_ident(name: &str) -> String {
    let escaped = name.replace('"', "\"\"");
    format!("\"{escaped}\"")
}

fn json_cell_to_opt(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::Null => None,
        serde_json::Value::String(s) => Some(s.clone()),
        other => Some(other.to_string()),
    }
}

pub fn run_sqlite_json(
    path: &Path,
    sql: &str,
    readonly: bool,
) -> Result<SqliteQueryResult, String> {
    let start = Instant::now();
    let mut cmd = Command::new("sqlite3");
    if readonly {
        cmd.arg("-readonly");
    }
    cmd.arg("-json").arg(path).arg(sql);

    let out = cmd
        .output()
        .map_err(|e| format!("Failed to run `sqlite3`: {e}"))?;
    let duration_ms = start.elapsed().as_millis();

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            format!("sqlite3 exited with status {}", out.status)
        } else {
            stderr
        });
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Ok(SqliteQueryResult {
            columns: Vec::new(),
            rows: Vec::new(),
            duration_ms,
        });
    }

    let arr: Vec<serde_json::Map<String, serde_json::Value>> =
        serde_json::from_str(trimmed).map_err(|e| format!("Failed to parse SQLite JSON: {e}"))?;

    let mut columns: Vec<String> = Vec::new();
    for obj in &arr {
        for key in obj.keys() {
            if !columns.contains(key) {
                columns.push(key.clone());
            }
        }
    }

    let mut rows = Vec::with_capacity(arr.len());
    for obj in arr {
        let row = columns
            .iter()
            .map(|c| obj.get(c).and_then(json_cell_to_opt))
            .collect();
        rows.push(row);
    }

    Ok(SqliteQueryResult {
        columns,
        rows,
        duration_ms,
    })
}

pub fn load_metadata(path: &Path) -> Result<SqliteMetadata, String> {
    let res = run_sqlite_json(
        path,
        "SELECT name, type, sql FROM sqlite_master WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite_%' ORDER BY type, name;",
        true,
    )?;

    let name_ix = res.columns.iter().position(|c| c == "name").unwrap_or(0);
    let type_ix = res.columns.iter().position(|c| c == "type").unwrap_or(1);
    let sql_ix = res.columns.iter().position(|c| c == "sql").unwrap_or(2);

    let mut tables = Vec::with_capacity(res.rows.len());
    for (ix, row) in res.rows.iter().enumerate() {
        let name = row.get(name_ix).and_then(|v| v.clone()).unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        let kind = row
            .get(type_ix)
            .and_then(|v| v.clone())
            .unwrap_or_else(|| "table".to_string());
        let sql = row
            .get(sql_ix)
            .and_then(|v| v.clone())
            .unwrap_or_default();

        let row_count = if ix < 50 {
            let count_sql = format!("SELECT COUNT(*) AS cnt FROM {};", quote_ident(&name));
            run_sqlite_json(path, &count_sql, true)
                .ok()
                .and_then(|r| r.rows.first().cloned())
                .and_then(|r| r.first().cloned().flatten())
                .and_then(|s| s.parse::<u64>().ok())
        } else {
            None
        };

        tables.push(SqliteTableInfo {
            name,
            kind,
            row_count,
            sql,
        });
    }

    Ok(SqliteMetadata { tables })
}

pub fn load_table_page(
    path: &Path,
    table: &str,
    limit: usize,
    offset: usize,
) -> Result<(Vec<SqliteColumnInfo>, SqliteQueryResult), String> {
    let pragma_sql = format!("PRAGMA table_info({});", quote_ident(table));
    let pragma_res = run_sqlite_json(path, &pragma_sql, true)?;

    let cname_ix = pragma_res
        .columns
        .iter()
        .position(|c| c == "name")
        .unwrap_or(1);
    let ctype_ix = pragma_res
        .columns
        .iter()
        .position(|c| c == "type")
        .unwrap_or(2);
    let nn_ix = pragma_res
        .columns
        .iter()
        .position(|c| c == "notnull")
        .unwrap_or(3);
    let pk_ix = pragma_res
        .columns
        .iter()
        .position(|c| c == "pk")
        .unwrap_or(5);

    let mut cols_info = Vec::with_capacity(pragma_res.rows.len());
    for row in &pragma_res.rows {
        let name = row
            .get(cname_ix)
            .and_then(|v| v.clone())
            .unwrap_or_default();
        let col_type = row
            .get(ctype_ix)
            .and_then(|v| v.clone())
            .unwrap_or_default();
        let not_null = row
            .get(nn_ix)
            .and_then(|v| v.as_deref())
            .is_some_and(|s| s != "0");
        let pk = row
            .get(pk_ix)
            .and_then(|v| v.as_deref())
            .is_some_and(|s| s != "0");
        cols_info.push(SqliteColumnInfo {
            name,
            col_type,
            not_null,
            pk,
        });
    }

    let select_sql = format!(
        "SELECT * FROM {} LIMIT {} OFFSET {};",
        quote_ident(table),
        limit.max(1),
        offset
    );
    let mut data_res = run_sqlite_json(path, &select_sql, true)?;
    if data_res.columns.is_empty() && !cols_info.is_empty() {
        data_res.columns = cols_info.iter().map(|c| c.name.clone()).collect();
    }

    Ok((cols_info, data_res))
}

pub fn export_result_csv(result: &SqliteQueryResult, out_path: &Path) -> Result<()> {
    let mut writer = csv::Writer::from_path(out_path)
        .with_context(|| format!("Failed to create {}", out_path.display()))?;
    writer.write_record(&result.columns)?;
    for row in &result.rows {
        let cells: Vec<&str> = row
            .iter()
            .map(|c| c.as_deref().unwrap_or(""))
            .collect();
        writer.write_record(&cells)?;
    }
    writer.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queries_metadata_pages_and_exports_csv_from_real_sqlite_db() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("notes.sqlite");

        let init_sql = "\
CREATE TABLE tasks (id INTEGER PRIMARY KEY, title TEXT NOT NULL, note TEXT);
INSERT INTO tasks (title, note) VALUES ('Ship Wave 3', NULL), ('Write tests', 'passing');
CREATE VIEW open_tasks AS SELECT id, title FROM tasks WHERE note IS NULL;";

        let status = Command::new("sqlite3")
            .arg(&db_path)
            .arg(init_sql)
            .status()
            .unwrap();
        assert!(status.success());

        let meta = load_metadata(&db_path).unwrap();
        assert_eq!(meta.tables.len(), 2);
        let tbl = meta.tables.iter().find(|t| t.name == "tasks").unwrap();
        assert_eq!(tbl.row_count, Some(2));

        let (cols, res) = load_table_page(&db_path, "tasks", 50, 0).unwrap();
        assert_eq!(cols.len(), 3);
        assert!(cols[0].pk);
        assert!(cols[1].not_null);
        assert_eq!(res.rows.len(), 2);
        assert_eq!(res.rows[0][2], None);
        assert_eq!(res.rows[1][2].as_deref(), Some("passing"));

        let csv_path = tmp.path().join("out.csv");
        export_result_csv(&res, &csv_path).unwrap();
        let csv_text = std::fs::read_to_string(&csv_path).unwrap();
        assert!(csv_text.contains("id,title,note"));
        assert!(csv_text.contains("Ship Wave 3"));
    }
}
