use std::path::Path;

use rusqlite::{params, Connection};
use serde::Serialize;

use crate::index::schema::{doctor, open_connection};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SearchResult {
    pub thread_id: String,
    pub turn_id: Option<String>,
    pub kind: String,
    pub text: String,
    pub score: f64,
    pub updated_at: String,
    pub cwd: Option<String>,
}

pub fn search_index(path: &Path, query: &str, limit: usize) -> Result<Vec<SearchResult>, String> {
    let report = doctor(path)?;
    if !report.exists {
        return Err(format!(
            "search index not found at {}; run `codex-history index build` first",
            path.display()
        ));
    }
    if !report.healthy {
        return Err(format!(
            "search index at {} is not healthy; run `codex-history index build` again",
            path.display()
        ));
    }

    let trimmed_query = query.trim();
    if trimmed_query.is_empty() {
        return Err("search query cannot be empty".into());
    }

    let conn = open_connection(path)?;
    execute_search(&conn, trimmed_query, limit)
}

fn execute_search(
    conn: &Connection,
    query: &str,
    limit: usize,
) -> Result<Vec<SearchResult>, String> {
    let fts_query = to_fts_query(query);
    let like_query = query.to_lowercase();
    let mut statement = conn
        .prepare(
            "
            SELECT
                sd.thread_id,
                sd.turn_id,
                sd.kind,
                sd.text,
                sd.updated_at,
                sd.cwd,
                (
                    CASE
                        WHEN instr(lower(COALESCE(sd.title, '')), ?2) > 0
                            OR instr(lower(sd.text), ?2) > 0 THEN 1000
                        ELSE 0
                    END
                    + CASE
                        WHEN sd.kind IN ('command_execution', 'file_change') THEN 200
                        ELSE 0
                    END
                    + CASE
                        WHEN sd.kind IN ('thread_name', 'thread_preview') THEN 100
                        ELSE 0
                    END
                ) AS heuristic_rank,
                bm25(search_docs_fts, 8.0, 1.0) AS fts_rank
            FROM search_docs_fts
            JOIN search_docs AS sd ON sd.doc_id = search_docs_fts.rowid
            WHERE search_docs_fts MATCH ?1
            ORDER BY heuristic_rank DESC, fts_rank ASC, sd.updated_at DESC, sd.doc_id ASC
            LIMIT ?3
            ",
        )
        .map_err(|error| format!("failed to prepare search query: {error}"))?;

    let rows = statement
        .query_map(params![fts_query, like_query, limit as i64], |row| {
            let heuristic_rank: f64 = row.get(6)?;
            let fts_rank: f64 = row.get(7)?;

            Ok(SearchResult {
                thread_id: row.get(0)?,
                turn_id: row.get(1)?,
                kind: row.get(2)?,
                text: excerpt_text(&row.get::<_, String>(3)?),
                updated_at: row.get(4)?,
                cwd: row.get(5)?,
                score: heuristic_rank - fts_rank,
            })
        })
        .map_err(|error| format!("failed to execute search query: {error}"))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("failed to decode search results: {error}"))
}

fn to_fts_query(query: &str) -> String {
    let tokens: Vec<_> = query
        .split_whitespace()
        .filter(|token| !token.is_empty())
        .map(|token| format!("\"{}\"", token.replace('"', "\"\"")))
        .collect();

    if tokens.is_empty() {
        format!("\"{}\"", query.replace('"', "\"\""))
    } else {
        tokens.join(" AND ")
    }
}

fn excerpt_text(text: &str) -> String {
    const LIMIT: usize = 240;
    if text.chars().count() <= LIMIT {
        return text.to_string();
    }

    let excerpt: String = text.chars().take(LIMIT).collect();
    format!("{excerpt}...")
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::path::PathBuf;
    use std::sync::{Mutex, OnceLock};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;
    use crate::backend::local::LocalBackend;
    use crate::index::ingest::build_local_index;

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn fixture_root(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/local_history")
            .join(name)
    }

    #[test]
    fn search_returns_ranked_results_from_index() {
        let _guard = env_lock().lock().expect("lock");
        env::set_var("CODEX_HISTORY_HOME", fixture_root("sample_root"));
        let backend = LocalBackend::discover();
        let path = temp_db_path("search-index");
        build_local_index(&backend, &path).expect("build index");

        let results = search_index(&path, "help ok", 10).expect("search");
        assert!(!results.is_empty());
        assert_eq!(results[0].thread_id, "thr_simple");
        assert_eq!(results[0].kind, "command_execution");
        assert!(results[0].text.contains("help ok"));

        std::fs::remove_file(path).expect("cleanup db");
        env::remove_var("CODEX_HISTORY_HOME");
    }

    fn temp_db_path(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        env::temp_dir().join(format!("codex-history-{label}-{nanos}.sqlite"))
    }
}
