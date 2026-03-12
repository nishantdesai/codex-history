use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::path::Path;

use rusqlite::{params, params_from_iter, Connection};
use serde::Serialize;

use crate::index::ingest::search_documents_for_thread;
use crate::index::manifest::{
    build_manifest_record, classify_thread, ThreadFreshness, ThreadManifestRecord,
};
use crate::index::schema::{doctor, open_connection};
use crate::model::ThreadDetail;
use crate::search_scope::SearchScope;

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

#[derive(Debug, Clone, PartialEq)]
pub struct IndexedThreadInfo {
    pub thread_id: String,
    pub name: Option<String>,
    pub preview: Option<String>,
    pub cwd: Option<String>,
}

pub fn search_index(
    path: &Path,
    query: &str,
    limit: usize,
    scope: SearchScope,
) -> Result<Vec<SearchResult>, String> {
    let conn = open_search_connection(path)?;
    execute_search(&conn, query, limit, 0, scope)
}

pub fn load_index_thread_info(
    path: &Path,
    thread_ids: &[String],
) -> Result<HashMap<String, IndexedThreadInfo>, String> {
    if thread_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let conn = open_search_connection(path)?;
    let placeholders = vec!["?"; thread_ids.len()].join(", ");
    let sql = format!(
        "SELECT thread_id, name, preview, cwd FROM threads WHERE thread_id IN ({placeholders})"
    );
    let mut statement = conn
        .prepare(&sql)
        .map_err(|error| format!("failed to prepare thread info query: {error}"))?;
    let rows = statement
        .query_map(params_from_iter(thread_ids.iter()), |row| {
            Ok(IndexedThreadInfo {
                thread_id: row.get(0)?,
                name: row.get(1)?,
                preview: row.get(2)?,
                cwd: row.get(3)?,
            })
        })
        .map_err(|error| format!("failed to execute thread info query: {error}"))?;

    let mut info = HashMap::new();
    for row in rows {
        let row = row.map_err(|error| format!("failed to decode thread info row: {error}"))?;
        info.insert(row.thread_id.clone(), row);
    }

    Ok(info)
}

pub fn search_with_fresh_overlay(
    path: &Path,
    query: &str,
    limit: usize,
    scope: SearchScope,
    current_details: &[ThreadDetail],
    manifest: &HashMap<String, ThreadManifestRecord>,
) -> Result<Vec<SearchResult>, String> {
    let expanded_limit = expanded_limit(limit);
    let mut changed_thread_ids = HashSet::new();
    let mut changed_details = Vec::new();

    for detail in current_details {
        let current_manifest = build_manifest_record(detail, "");
        match classify_thread(&current_manifest, manifest.get(&current_manifest.thread_id)) {
            ThreadFreshness::Unchanged => {}
            ThreadFreshness::New | ThreadFreshness::Changed => {
                changed_thread_ids.insert(detail.summary.thread_id.clone());
                changed_details.push(detail);
            }
        }
    }

    if changed_thread_ids.is_empty() {
        return search_index(path, query, limit, scope);
    }

    let conn = open_search_connection(path)?;
    let mut results = collect_index_results_excluding_threads(
        &conn,
        query,
        expanded_limit,
        &changed_thread_ids,
        scope,
    )?;
    results.extend(search_local_details(
        query,
        &changed_details,
        expanded_limit,
        scope,
    ));
    Ok(merge_results(results, limit))
}

fn open_search_connection(path: &Path) -> Result<Connection, String> {
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

    open_connection(path)
}

fn execute_search(
    conn: &Connection,
    query: &str,
    limit: usize,
    offset: usize,
    scope: SearchScope,
) -> Result<Vec<SearchResult>, String> {
    let trimmed_query = query.trim();
    if trimmed_query.is_empty() {
        return Err("search query cannot be empty".into());
    }

    let fts_query = to_fts_query(trimmed_query);
    let like_query = trimmed_query.to_lowercase();
    let kind_filter = scope.search_kind_sql();
    let sql = format!(
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
              AND sd.kind IN ({kind_filter})
            ORDER BY heuristic_rank DESC, fts_rank ASC, sd.updated_at DESC, sd.doc_id ASC
            LIMIT ?3
            OFFSET ?4
            "
    );
    let mut statement = conn
        .prepare(&sql)
        .map_err(|error| format!("failed to prepare search query: {error}"))?;

    let rows = statement
        .query_map(
            params![fts_query, like_query, limit as i64, offset as i64],
            |row| {
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
            },
        )
        .map_err(|error| format!("failed to execute search query: {error}"))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("failed to decode search results: {error}"))
}

fn collect_index_results_excluding_threads(
    conn: &Connection,
    query: &str,
    desired_count: usize,
    excluded_thread_ids: &HashSet<String>,
    scope: SearchScope,
) -> Result<Vec<SearchResult>, String> {
    let page_size = desired_count.max(1);
    let mut offset = 0;
    let mut results = Vec::new();

    loop {
        let page = execute_search(conn, query, page_size, offset, scope)?;
        if page.is_empty() {
            break;
        }

        let fetched = page.len();
        results.extend(
            page.into_iter()
                .filter(|result| !excluded_thread_ids.contains(&result.thread_id)),
        );

        if results.len() >= desired_count || fetched < page_size {
            break;
        }

        offset += fetched;
    }

    Ok(results)
}

fn search_local_details(
    query: &str,
    details: &[&ThreadDetail],
    limit: usize,
    scope: SearchScope,
) -> Vec<SearchResult> {
    let trimmed_query = query.trim();
    if trimmed_query.is_empty() {
        return Vec::new();
    }

    let tokens = query_tokens(trimmed_query);
    let mut results = Vec::new();

    for detail in details {
        for doc in search_documents_for_thread(detail) {
            if !scope.includes_search_kind(&doc.kind) {
                continue;
            }
            let searchable = format!("{}\n{}", doc.title.as_deref().unwrap_or_default(), doc.text);
            let searchable_tokens = query_tokens(&searchable);
            let searchable_token_set = searchable_tokens.iter().cloned().collect::<HashSet<_>>();
            if !tokens
                .iter()
                .all(|token| searchable_token_set.contains(token))
            {
                continue;
            }

            let phrase_match = contains_token_sequence(&searchable_tokens, &tokens);
            let kind_boost = if matches!(doc.kind.as_str(), "command_execution" | "file_change") {
                200.0
            } else if matches!(doc.kind.as_str(), "thread_name" | "thread_preview") {
                100.0
            } else {
                0.0
            };
            let token_hits = tokens
                .iter()
                .filter(|token| searchable_token_set.contains(*token))
                .count() as f64;

            results.push(SearchResult {
                thread_id: doc.thread_id.clone(),
                turn_id: doc.turn_id.clone(),
                kind: doc.kind.clone(),
                text: excerpt_text(&doc.text),
                score: if phrase_match { 1000.0 } else { 0.0 } + kind_boost + token_hits,
                updated_at: doc.updated_at.clone(),
                cwd: doc.cwd.clone(),
            });
        }
    }

    merge_results(results, limit)
}

fn merge_results(results: Vec<SearchResult>, limit: usize) -> Vec<SearchResult> {
    let mut seen = HashSet::new();
    let mut unique = Vec::new();

    for result in results {
        let key = result_identity(&result);
        if seen.insert(key) {
            unique.push(result);
        }
    }

    unique.sort_by(compare_search_results);
    unique.truncate(limit);
    unique
}

fn compare_search_results(left: &SearchResult, right: &SearchResult) -> Ordering {
    right
        .score
        .partial_cmp(&left.score)
        .unwrap_or(Ordering::Equal)
        .then_with(|| right.updated_at.cmp(&left.updated_at))
        .then_with(|| left.thread_id.cmp(&right.thread_id))
        .then_with(|| left.turn_id.cmp(&right.turn_id))
        .then_with(|| left.kind.cmp(&right.kind))
        .then_with(|| left.text.cmp(&right.text))
}

fn result_identity(result: &SearchResult) -> (String, Option<String>, String, String) {
    (
        result.thread_id.clone(),
        result.turn_id.clone(),
        result.kind.clone(),
        result.text.clone(),
    )
}

fn query_tokens(query: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();

    for ch in query.chars() {
        if ch.is_alphanumeric() {
            current.extend(ch.to_lowercase());
        } else if !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

fn expanded_limit(limit: usize) -> usize {
    limit.saturating_mul(5).max(limit.saturating_add(20))
}

fn to_fts_query(query: &str) -> String {
    let tokens: Vec<_> = query_tokens(query)
        .into_iter()
        .map(|token| format!("\"{}\"", token.replace('"', "\"\"")))
        .collect();

    if tokens.is_empty() {
        format!("\"{}\"", query.replace('"', "\"\""))
    } else {
        tokens.join(" AND ")
    }
}

fn contains_token_sequence(haystack: &[String], needle: &[String]) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
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
    use crate::index::manifest::load_manifest;
    use crate::search_scope::SearchScope;

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

        let results =
            search_index(&path, "leftover argv", 10, SearchScope::default()).expect("search");
        assert!(!results.is_empty());
        assert_eq!(results[0].thread_id, "thr_simple");
        assert_eq!(results[0].kind, "agent_message");
        assert!(results[0].text.contains("leftover argv"));

        std::fs::remove_file(path).expect("cleanup db");
        env::remove_var("CODEX_HISTORY_HOME");
    }

    #[test]
    fn fresh_overlay_filters_stale_index_results_for_changed_threads() {
        let _guard = env_lock().lock().expect("lock");
        env::set_var("CODEX_HISTORY_HOME", fixture_root("sample_root"));
        let backend = LocalBackend::discover();
        let path = temp_db_path("search-fresh");
        build_local_index(&backend, &path).expect("build index");

        let mut details = backend.list_thread_details().expect("thread details");
        let simple = details
            .iter_mut()
            .find(|detail| detail.summary.thread_id == "thr_simple")
            .expect("thr_simple");
        simple.summary.updated_at = Some(simple.summary.created_at + chrono::Duration::minutes(5));

        let manifest = {
            let conn = open_connection(&path).expect("open db");
            load_manifest(&conn).expect("manifest")
        };
        let results = search_with_fresh_overlay(
            &path,
            "help ok",
            10,
            SearchScope {
                include_thinking: false,
                include_tools: true,
            },
            &details,
            &manifest,
        )
        .expect("fresh search");
        let simple_hits = results
            .iter()
            .filter(|result| result.thread_id == "thr_simple" && result.kind == "command_execution")
            .count();
        assert_eq!(simple_hits, 1);

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
