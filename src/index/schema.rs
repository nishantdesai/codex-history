use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use crate::index::manifest::INDEX_SCHEMA_VERSION;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IndexDoctorReport {
    pub path: PathBuf,
    pub exists: bool,
    pub schema_version_expected: i64,
    pub schema_version: Option<i64>,
    pub healthy: bool,
    pub threads: u64,
    pub turns: u64,
    pub items: u64,
    pub search_docs: u64,
    pub thread_manifest: u64,
    pub issues: Vec<String>,
}

pub fn open_connection(path: &Path) -> Result<Connection, String> {
    let conn = Connection::open(path)
        .map_err(|error| format!("failed to open index database {}: {error}", path.display()))?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")
        .map_err(|error| {
            format!(
                "failed to configure index database {}: {error}",
                path.display()
            )
        })?;
    Ok(conn)
}

pub fn initialize_schema(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS threads (
            thread_id TEXT PRIMARY KEY,
            name TEXT,
            preview TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT,
            cwd TEXT,
            source_kind TEXT,
            model_provider TEXT,
            ephemeral INTEGER,
            status TEXT
        );

        CREATE TABLE IF NOT EXISTS turns (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            thread_id TEXT NOT NULL REFERENCES threads(thread_id) ON DELETE CASCADE,
            turn_id TEXT NOT NULL,
            status TEXT NOT NULL,
            started_at TEXT,
            completed_at TEXT,
            UNIQUE(thread_id, turn_id)
        );

        CREATE TABLE IF NOT EXISTS items (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            thread_id TEXT NOT NULL REFERENCES threads(thread_id) ON DELETE CASCADE,
            turn_id TEXT NOT NULL,
            item_index INTEGER NOT NULL,
            kind TEXT NOT NULL,
            text TEXT,
            json TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS search_docs (
            doc_id INTEGER PRIMARY KEY AUTOINCREMENT,
            thread_id TEXT NOT NULL REFERENCES threads(thread_id) ON DELETE CASCADE,
            turn_id TEXT,
            item_id INTEGER REFERENCES items(id) ON DELETE CASCADE,
            kind TEXT NOT NULL,
            title TEXT,
            text TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            cwd TEXT
        );

        CREATE TABLE IF NOT EXISTS thread_manifest (
            thread_id TEXT PRIMARY KEY,
            last_seen_updated_at TEXT NOT NULL,
            content_fingerprint TEXT NOT NULL,
            last_indexed_at TEXT NOT NULL,
            source_backend TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS index_meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE VIRTUAL TABLE IF NOT EXISTS search_docs_fts USING fts5(
            title,
            text,
            kind UNINDEXED,
            thread_id UNINDEXED,
            turn_id UNINDEXED,
            cwd UNINDEXED
        );
        ",
    )
    .map_err(|error| format!("failed to initialize index schema: {error}"))?;

    set_meta(conn, "schema_version", &INDEX_SCHEMA_VERSION.to_string())?;
    Ok(())
}

pub fn rebuild_schema(conn: &Connection) -> Result<(), String> {
    initialize_schema(conn)?;
    conn.execute_batch(
        "
        DELETE FROM search_docs_fts;
        DELETE FROM search_docs;
        DELETE FROM items;
        DELETE FROM turns;
        DELETE FROM threads;
        DELETE FROM thread_manifest;
        DELETE FROM index_meta;
        ",
    )
    .map_err(|error| format!("failed to reset index schema: {error}"))?;
    set_meta(conn, "schema_version", &INDEX_SCHEMA_VERSION.to_string())
}

pub fn set_meta(conn: &Connection, key: &str, value: &str) -> Result<(), String> {
    conn.execute(
        "
        INSERT INTO index_meta (key, value)
        VALUES (?1, ?2)
        ON CONFLICT(key) DO UPDATE SET value = excluded.value
        ",
        params![key, value],
    )
    .map_err(|error| format!("failed to update index metadata `{key}`: {error}"))?;
    Ok(())
}

pub fn get_meta(conn: &Connection, key: &str) -> Result<Option<String>, String> {
    conn.query_row(
        "SELECT value FROM index_meta WHERE key = ?1",
        params![key],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .map_err(|error| format!("failed to read index metadata `{key}`: {error}"))
}

pub fn doctor(path: &Path) -> Result<IndexDoctorReport, String> {
    if !path.exists() {
        return Ok(IndexDoctorReport {
            path: path.to_path_buf(),
            exists: false,
            schema_version_expected: INDEX_SCHEMA_VERSION,
            schema_version: None,
            healthy: false,
            threads: 0,
            turns: 0,
            items: 0,
            search_docs: 0,
            thread_manifest: 0,
            issues: vec!["index database does not exist".into()],
        });
    }

    let conn = open_connection(path)?;

    let mut issues = Vec::new();
    let schema_version = if table_exists(&conn, "index_meta")? {
        get_meta(&conn, "schema_version")?.and_then(|value| value.parse::<i64>().ok())
    } else {
        issues.push("missing table index_meta".into());
        None
    };
    if schema_version != Some(INDEX_SCHEMA_VERSION) {
        issues.push(format!(
            "schema version mismatch: expected {}, found {}",
            INDEX_SCHEMA_VERSION,
            schema_version
                .map(|value| value.to_string())
                .unwrap_or_else(|| "(missing)".into())
        ));
    }

    for table in [
        "threads",
        "turns",
        "items",
        "search_docs",
        "thread_manifest",
        "search_docs_fts",
    ] {
        if !table_exists(&conn, table)? {
            if table == "search_docs_fts" {
                issues.push("missing FTS table search_docs_fts".into());
            } else {
                issues.push(format!("missing table {table}"));
            }
        }
    }

    Ok(IndexDoctorReport {
        path: path.to_path_buf(),
        exists: true,
        schema_version_expected: INDEX_SCHEMA_VERSION,
        schema_version,
        healthy: issues.is_empty(),
        threads: safe_table_count(&conn, "threads")?,
        turns: safe_table_count(&conn, "turns")?,
        items: safe_table_count(&conn, "items")?,
        search_docs: safe_table_count(&conn, "search_docs")?,
        thread_manifest: safe_table_count(&conn, "thread_manifest")?,
        issues,
    })
}

fn table_exists(conn: &Connection, name: &str) -> Result<bool, String> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name = ?1)",
        params![name],
        |row| row.get::<_, i64>(0),
    )
    .map(|value| value == 1)
    .map_err(|error| format!("failed to inspect table `{name}`: {error}"))
}

fn table_count(conn: &Connection, table: &str) -> Result<u64, String> {
    let query = match table {
        "threads" => "SELECT COUNT(*) FROM threads",
        "turns" => "SELECT COUNT(*) FROM turns",
        "items" => "SELECT COUNT(*) FROM items",
        "search_docs" => "SELECT COUNT(*) FROM search_docs",
        "thread_manifest" => "SELECT COUNT(*) FROM thread_manifest",
        other => return Err(format!("unsupported table count request: {other}")),
    };

    conn.query_row(query, [], |row| row.get::<_, u64>(0))
        .map_err(|error| format!("failed to count rows for `{table}`: {error}"))
}

fn safe_table_count(conn: &Connection, table: &str) -> Result<u64, String> {
    if table_exists(conn, table)? {
        table_count(conn, table)
    } else {
        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;
    use crate::index::manifest::INDEX_SCHEMA_VERSION;

    #[test]
    fn initialize_schema_creates_expected_tables() {
        let path = temp_db_path("schema");
        let conn = open_connection(&path).expect("open connection");
        initialize_schema(&conn).expect("initialize schema");

        assert!(table_exists(&conn, "threads").expect("threads table"));
        assert!(table_exists(&conn, "turns").expect("turns table"));
        assert!(table_exists(&conn, "items").expect("items table"));
        assert!(table_exists(&conn, "search_docs").expect("search_docs table"));
        assert!(table_exists(&conn, "thread_manifest").expect("thread_manifest table"));
        assert!(table_exists(&conn, "index_meta").expect("index_meta table"));
        assert!(table_exists(&conn, "search_docs_fts").expect("fts table"));
        assert_eq!(
            get_meta(&conn, "schema_version").expect("schema version"),
            Some(INDEX_SCHEMA_VERSION.to_string())
        );

        std::fs::remove_file(path).expect("cleanup");
    }

    #[test]
    fn doctor_reports_missing_database() {
        let path = temp_db_path("missing");
        let report = doctor(&path).expect("doctor report");

        assert!(!report.exists);
        assert!(!report.healthy);
        assert!(report
            .issues
            .iter()
            .any(|issue| issue.contains("does not exist")));
    }

    fn temp_db_path(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        env::temp_dir().join(format!("codex-history-{label}-{nanos}.sqlite"))
    }
}
