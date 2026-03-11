use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusqlite::{params, Transaction};
use serde::Serialize;

use crate::backend::local::LocalBackend;
use crate::index::manifest::{
    build_manifest_record, classify_thread, current_timestamp, ensure_index_parent_dir,
    load_manifest, manifest_watermark, store_manifest_record, ThreadFreshness,
    ThreadManifestRecord, INDEX_SCHEMA_VERSION, INDEX_SOURCE_BACKEND_LOCAL,
};
use crate::index::schema::{doctor, open_connection, rebuild_schema};
use crate::model::{Item, ThreadDetail};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IndexBuildReport {
    pub path: PathBuf,
    pub schema_version: i64,
    pub source_backend: String,
    pub built_at: String,
    pub threads: u64,
    pub turns: u64,
    pub items: u64,
    pub search_docs: u64,
    pub manifest_rows: u64,
    pub watermark: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IndexRefreshReport {
    pub path: PathBuf,
    pub schema_version: i64,
    pub source_backend: String,
    pub refreshed_at: String,
    pub new_threads: u64,
    pub changed_threads: u64,
    pub unchanged_threads: u64,
    pub indexed_threads: u64,
    pub manifest_rows: u64,
    pub watermark: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SearchDocument {
    pub thread_id: String,
    pub turn_id: Option<String>,
    pub kind: String,
    pub title: Option<String>,
    pub text: String,
    pub updated_at: String,
    pub cwd: Option<String>,
}

#[derive(Debug, Default)]
struct IndexCounts {
    threads: u64,
    turns: u64,
    items: u64,
    search_docs: u64,
    manifest_rows: u64,
}

pub fn build_local_index(backend: &LocalBackend, path: &Path) -> Result<IndexBuildReport, String> {
    ensure_index_parent_dir(path)?;
    let mut conn = open_connection(path)?;
    rebuild_schema(&conn)?;

    let built_at = current_timestamp();
    let details = backend.list_thread_details()?;
    let watermark = details_watermark(&details);
    let tx = conn
        .transaction()
        .map_err(|error| format!("failed to begin index transaction: {error}"))?;
    let mut counts = IndexCounts::default();

    for detail in &details {
        upsert_thread_detail(&tx, detail, &built_at, &mut counts)?;
    }

    finalize_index_meta(&tx, &built_at, watermark.as_deref(), details.len() as u64)?;
    tx.commit()
        .map_err(|error| format!("failed to commit index transaction: {error}"))?;

    Ok(IndexBuildReport {
        path: path.to_path_buf(),
        schema_version: INDEX_SCHEMA_VERSION,
        source_backend: INDEX_SOURCE_BACKEND_LOCAL.to_string(),
        built_at,
        threads: counts.threads,
        turns: counts.turns,
        items: counts.items,
        search_docs: counts.search_docs,
        manifest_rows: counts.manifest_rows,
        watermark,
    })
}

pub fn refresh_local_index(
    backend: &LocalBackend,
    path: &Path,
) -> Result<IndexRefreshReport, String> {
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

    let mut conn = open_connection(path)?;
    let existing_manifest = load_manifest(&conn)?;
    let refreshed_at = current_timestamp();
    let details = backend.list_thread_details()?;
    let watermark = details_watermark(&details);
    let tx = conn
        .transaction()
        .map_err(|error| format!("failed to begin refresh transaction: {error}"))?;

    let mut counts = IndexCounts::default();
    let mut refresh_report = IndexRefreshReport {
        path: path.to_path_buf(),
        schema_version: INDEX_SCHEMA_VERSION,
        source_backend: INDEX_SOURCE_BACKEND_LOCAL.to_string(),
        refreshed_at: refreshed_at.clone(),
        new_threads: 0,
        changed_threads: 0,
        unchanged_threads: 0,
        indexed_threads: 0,
        manifest_rows: 0,
        watermark: watermark.clone(),
    };

    for detail in &details {
        let current_manifest = build_manifest_record(detail, &refreshed_at);
        match classify_thread(
            &current_manifest,
            existing_manifest.get(&current_manifest.thread_id),
        ) {
            ThreadFreshness::New => {
                upsert_thread_detail(&tx, detail, &refreshed_at, &mut counts)?;
                refresh_report.new_threads += 1;
                refresh_report.indexed_threads += 1;
            }
            ThreadFreshness::Changed => {
                delete_thread_detail(&tx, &current_manifest.thread_id)?;
                upsert_thread_detail(&tx, detail, &refreshed_at, &mut counts)?;
                refresh_report.changed_threads += 1;
                refresh_report.indexed_threads += 1;
            }
            ThreadFreshness::Unchanged => {
                refresh_report.unchanged_threads += 1;
            }
        }
    }

    finalize_index_meta(
        &tx,
        &refreshed_at,
        watermark.as_deref(),
        details.len() as u64,
    )?;
    tx.commit()
        .map_err(|error| format!("failed to commit refresh transaction: {error}"))?;

    let conn = open_connection(path)?;
    refresh_report.manifest_rows = load_manifest(&conn)?.len() as u64;
    Ok(refresh_report)
}

pub(crate) fn load_manifest_snapshot(
    path: &Path,
) -> Result<HashMap<String, ThreadManifestRecord>, String> {
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

    let conn = open_connection(path)?;
    load_manifest(&conn)
}

pub(crate) fn search_documents_for_thread(detail: &ThreadDetail) -> Vec<SearchDocument> {
    let summary = &detail.summary;
    let updated_at = summary
        .updated_at
        .unwrap_or(summary.created_at)
        .to_rfc3339();
    let cwd = summary.cwd.as_ref().map(|path| path.display().to_string());
    let mut docs = Vec::new();

    if let Some(name) = summary.name.as_deref().filter(|value| !value.is_empty()) {
        docs.push(SearchDocument {
            thread_id: summary.thread_id.clone(),
            turn_id: None,
            kind: "thread_name".into(),
            title: Some(name.to_string()),
            text: name.to_string(),
            updated_at: updated_at.clone(),
            cwd: cwd.clone(),
        });
    }

    if let Some(preview) = summary.preview.as_deref().filter(|value| !value.is_empty()) {
        docs.push(SearchDocument {
            thread_id: summary.thread_id.clone(),
            turn_id: None,
            kind: "thread_preview".into(),
            title: summary.name.clone(),
            text: preview.to_string(),
            updated_at: updated_at.clone(),
            cwd: cwd.clone(),
        });
    }

    for turn in &detail.turns {
        for item in &turn.items {
            let Some(text) = item_search_text(item) else {
                continue;
            };
            docs.push(SearchDocument {
                thread_id: summary.thread_id.clone(),
                turn_id: Some(turn.turn_id.clone()),
                kind: item.kind().to_string(),
                title: item_search_title(item),
                text,
                updated_at: updated_at.clone(),
                cwd: cwd.clone(),
            });
        }
    }

    docs
}

fn upsert_thread_detail(
    tx: &Transaction<'_>,
    detail: &ThreadDetail,
    indexed_at: &str,
    counts: &mut IndexCounts,
) -> Result<(), String> {
    let summary = &detail.summary;
    let cwd = summary.cwd.as_ref().map(|path| path.display().to_string());

    tx.execute(
        "
        INSERT INTO threads (
            thread_id, name, preview, created_at, updated_at, cwd,
            source_kind, model_provider, ephemeral, status
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
        ",
        params![
            summary.thread_id,
            summary.name,
            summary.preview,
            summary.created_at.to_rfc3339(),
            summary.updated_at.map(|value| value.to_rfc3339()),
            cwd,
            summary.source_kind,
            summary.model_provider,
            summary.ephemeral,
            summary.status,
        ],
    )
    .map_err(|error| format!("failed to insert thread {}: {error}", summary.thread_id))?;
    counts.threads += 1;

    for turn in &detail.turns {
        tx.execute(
            "
            INSERT INTO turns (thread_id, turn_id, status, started_at, completed_at)
            VALUES (?1, ?2, ?3, ?4, ?5)
            ",
            params![
                summary.thread_id,
                turn.turn_id,
                turn.status,
                turn.started_at.map(|value| value.to_rfc3339()),
                turn.completed_at.map(|value| value.to_rfc3339()),
            ],
        )
        .map_err(|error| {
            format!(
                "failed to insert turn {} for thread {}: {error}",
                turn.turn_id, summary.thread_id
            )
        })?;
        counts.turns += 1;

        for (item_index, item) in turn.items.iter().enumerate() {
            let item_text = item_search_text(item);
            let item_json = serde_json::to_string(item)
                .map_err(|error| format!("failed to serialize item {}: {error}", item.kind()))?;
            tx.execute(
                "
                INSERT INTO items (thread_id, turn_id, item_index, kind, text, json)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                ",
                params![
                    summary.thread_id,
                    turn.turn_id,
                    item_index as i64,
                    item.kind(),
                    item_text,
                    item_json,
                ],
            )
            .map_err(|error| {
                format!(
                    "failed to insert item {} for thread {} turn {}: {error}",
                    item.kind(),
                    summary.thread_id,
                    turn.turn_id
                )
            })?;
            counts.items += 1;
        }
    }

    for doc in search_documents_for_thread(detail) {
        insert_search_doc(tx, &doc, counts)?;
    }

    store_manifest_record(tx, &build_manifest_record(detail, indexed_at))?;
    counts.manifest_rows += 1;
    Ok(())
}

fn insert_search_doc(
    tx: &Transaction<'_>,
    doc: &SearchDocument,
    counts: &mut IndexCounts,
) -> Result<(), String> {
    tx.execute(
        "
        INSERT INTO search_docs (thread_id, turn_id, item_id, kind, title, text, updated_at, cwd)
        VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6, ?7)
        ",
        params![
            doc.thread_id,
            doc.turn_id,
            doc.kind,
            doc.title,
            doc.text,
            doc.updated_at,
            doc.cwd,
        ],
    )
    .map_err(|error| {
        format!(
            "failed to insert search doc for thread {}: {error}",
            doc.thread_id
        )
    })?;
    let doc_id = tx.last_insert_rowid();
    tx.execute(
        "
        INSERT INTO search_docs_fts (rowid, title, text, kind, thread_id, turn_id, cwd)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        ",
        params![
            doc_id,
            doc.title,
            doc.text,
            doc.kind,
            doc.thread_id,
            doc.turn_id,
            doc.cwd,
        ],
    )
    .map_err(|error| {
        format!(
            "failed to insert FTS search doc for thread {}: {error}",
            doc.thread_id
        )
    })?;
    counts.search_docs += 1;
    Ok(())
}

fn delete_thread_detail(tx: &Transaction<'_>, thread_id: &str) -> Result<(), String> {
    tx.execute(
        "
        DELETE FROM search_docs_fts
        WHERE rowid IN (SELECT doc_id FROM search_docs WHERE thread_id = ?1)
        ",
        params![thread_id],
    )
    .map_err(|error| format!("failed to delete FTS docs for thread {thread_id}: {error}"))?;
    tx.execute(
        "DELETE FROM search_docs WHERE thread_id = ?1",
        params![thread_id],
    )
    .map_err(|error| format!("failed to delete search docs for thread {thread_id}: {error}"))?;
    tx.execute("DELETE FROM items WHERE thread_id = ?1", params![thread_id])
        .map_err(|error| format!("failed to delete items for thread {thread_id}: {error}"))?;
    tx.execute("DELETE FROM turns WHERE thread_id = ?1", params![thread_id])
        .map_err(|error| format!("failed to delete turns for thread {thread_id}: {error}"))?;
    tx.execute(
        "DELETE FROM threads WHERE thread_id = ?1",
        params![thread_id],
    )
    .map_err(|error| format!("failed to delete thread {thread_id}: {error}"))?;
    tx.execute(
        "DELETE FROM thread_manifest WHERE thread_id = ?1",
        params![thread_id],
    )
    .map_err(|error| format!("failed to delete manifest for thread {thread_id}: {error}"))?;
    Ok(())
}

fn finalize_index_meta(
    tx: &Transaction<'_>,
    indexed_at: &str,
    watermark: Option<&str>,
    tracked_threads: u64,
) -> Result<(), String> {
    set_meta_tx(tx, "schema_version", &INDEX_SCHEMA_VERSION.to_string())?;
    set_meta_tx(tx, "last_build_at", indexed_at)?;
    set_meta_tx(tx, "last_refresh_at", indexed_at)?;
    set_meta_tx(tx, "source_backend", INDEX_SOURCE_BACKEND_LOCAL)?;
    set_meta_tx(tx, "last_manifest_watermark", watermark.unwrap_or(""))?;
    set_meta_tx(tx, "tracked_thread_count", &tracked_threads.to_string())
}

fn set_meta_tx(tx: &Transaction<'_>, key: &str, value: &str) -> Result<(), String> {
    tx.execute(
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

fn details_watermark(details: &[ThreadDetail]) -> Option<String> {
    let manifest: Vec<_> = details
        .iter()
        .map(|detail| build_manifest_record(detail, ""))
        .collect();
    manifest_watermark(manifest.iter())
}

fn item_search_text(item: &Item) -> Option<String> {
    let texts = match item {
        Item::UserMessage(message) | Item::AgentMessage(message) => {
            message.text.clone().into_iter().collect::<Vec<_>>()
        }
        Item::CommandExecution(command) => {
            let mut values = Vec::new();
            if let Some(command_text) = &command.command {
                values.push(command_text.clone());
            }
            if let Some(output) = &command.output {
                values.push(output.clone());
            }
            if let Some(cwd) = &command.cwd {
                values.push(cwd.display().to_string());
            }
            values
        }
        Item::FileChange(change) => {
            let mut values = Vec::new();
            if let Some(path) = &change.path {
                values.push(path.display().to_string());
            }
            if let Some(summary) = &change.summary {
                values.push(summary.clone());
            }
            if let Some(change_type) = &change.change_type {
                values.push(change_type.clone());
            }
            values
        }
        Item::ReasoningSummary(summary) => summary.text.clone().into_iter().collect(),
        Item::WebSearch(search) => {
            let mut values = Vec::new();
            if let Some(query) = &search.query {
                values.push(query.clone());
            }
            if let Some(title) = &search.title {
                values.push(title.clone());
            }
            if let Some(url) = &search.url {
                values.push(url.clone());
            }
            values
        }
        Item::McpToolCall(call) => {
            let mut values = Vec::new();
            if let Some(server) = &call.server {
                values.push(server.clone());
            }
            if let Some(tool) = &call.tool {
                values.push(tool.clone());
            }
            if let Some(arguments) = &call.arguments {
                values.push(arguments.to_string());
            }
            values
        }
        Item::Other(other) => other
            .data
            .values()
            .filter_map(value_as_text)
            .collect::<Vec<_>>(),
    };

    if texts.is_empty() {
        None
    } else {
        Some(texts.join("\n\n"))
    }
}

fn item_search_title(item: &Item) -> Option<String> {
    match item {
        Item::CommandExecution(command) => command.command.clone(),
        Item::FileChange(change) => change
            .path
            .as_ref()
            .map(|path| path.display().to_string())
            .or_else(|| change.summary.clone()),
        Item::WebSearch(search) => search.title.clone().or_else(|| search.query.clone()),
        Item::McpToolCall(call) => call.tool.clone().or_else(|| call.server.clone()),
        Item::Other(other) => Some(other.kind.clone()),
        _ => None,
    }
}

fn value_as_text(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(value) => Some(value.clone()),
        serde_json::Value::Number(value) => Some(value.to_string()),
        serde_json::Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::path::PathBuf;
    use std::sync::{Mutex, OnceLock};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;
    use crate::index::manifest::load_manifest;
    use crate::index::schema::{doctor, get_meta};

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
    fn build_local_index_populates_core_tables() {
        let _guard = env_lock().lock().expect("lock");
        env::set_var("CODEX_HISTORY_HOME", fixture_root("sample_root"));
        let backend = LocalBackend::discover();
        let path = temp_db_path("index-build");

        let report = build_local_index(&backend, &path).expect("build index");
        assert_eq!(report.schema_version, INDEX_SCHEMA_VERSION);
        assert_eq!(report.threads, 3);
        assert_eq!(report.manifest_rows, 3);
        assert!(report.turns >= 5);
        assert!(report.items >= 8);
        assert!(report.search_docs >= 8);
        assert!(report.watermark.is_some());

        let doctor = doctor(&path).expect("doctor index");
        assert!(doctor.healthy);
        assert_eq!(doctor.threads, report.threads);
        assert_eq!(doctor.thread_manifest, report.manifest_rows);

        let conn = open_connection(&path).expect("open db");
        assert_eq!(
            get_meta(&conn, "tracked_thread_count").expect("tracked thread count"),
            Some("3".into())
        );

        std::fs::remove_file(path).expect("cleanup db");
        env::remove_var("CODEX_HISTORY_HOME");
    }

    #[test]
    fn refresh_skips_unchanged_threads_and_updates_manifest_watermark() {
        let _guard = env_lock().lock().expect("lock");
        env::set_var("CODEX_HISTORY_HOME", fixture_root("sample_root"));
        let backend = LocalBackend::discover();
        let path = temp_db_path("index-refresh");

        build_local_index(&backend, &path).expect("build index");
        let report = refresh_local_index(&backend, &path).expect("refresh index");
        assert_eq!(report.new_threads, 0);
        assert_eq!(report.changed_threads, 0);
        assert_eq!(report.unchanged_threads, 3);
        assert_eq!(report.indexed_threads, 0);

        let conn = open_connection(&path).expect("open db");
        assert_eq!(load_manifest(&conn).expect("manifest").len(), 3);
        assert_eq!(
            get_meta(&conn, "last_manifest_watermark").expect("watermark"),
            report.watermark
        );

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
