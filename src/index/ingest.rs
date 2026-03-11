use std::path::{Path, PathBuf};

use rusqlite::{params, Transaction};
use serde::Serialize;

use crate::backend::local::LocalBackend;
use crate::index::manifest::{
    build_manifest_record, current_timestamp, ensure_index_parent_dir, INDEX_SCHEMA_VERSION,
    INDEX_SOURCE_BACKEND_LOCAL,
};
use crate::index::schema::{open_connection, rebuild_schema};
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
    let tx = conn
        .transaction()
        .map_err(|error| format!("failed to begin index transaction: {error}"))?;
    let mut counts = IndexCounts::default();

    for detail in &details {
        insert_thread_detail(&tx, detail, &built_at, &mut counts)?;
    }

    set_meta_tx(&tx, "schema_version", &INDEX_SCHEMA_VERSION.to_string())?;
    set_meta_tx(&tx, "last_build_at", &built_at)?;
    set_meta_tx(&tx, "last_refresh_at", &built_at)?;
    set_meta_tx(&tx, "source_backend", INDEX_SOURCE_BACKEND_LOCAL)?;
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
    })
}

fn insert_thread_detail(
    tx: &Transaction<'_>,
    detail: &ThreadDetail,
    indexed_at: &str,
    counts: &mut IndexCounts,
) -> Result<(), String> {
    let summary = &detail.summary;
    let updated_at = summary
        .updated_at
        .unwrap_or(summary.created_at)
        .to_rfc3339();
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

    if let Some(name) = summary.name.as_deref().filter(|value| !value.is_empty()) {
        insert_search_doc(
            tx,
            &summary.thread_id,
            None,
            None,
            "thread_name",
            Some(name),
            name,
            &updated_at,
            summary.cwd.as_ref().map(|path| path.display().to_string()),
            counts,
        )?;
    }

    if let Some(preview) = summary.preview.as_deref().filter(|value| !value.is_empty()) {
        insert_search_doc(
            tx,
            &summary.thread_id,
            None,
            None,
            "thread_preview",
            summary.name.as_deref(),
            preview,
            &updated_at,
            summary.cwd.as_ref().map(|path| path.display().to_string()),
            counts,
        )?;
    }

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

            let item_id = tx.last_insert_rowid();
            if let Some(text) = item_text.as_deref().filter(|value| !value.is_empty()) {
                insert_search_doc(
                    tx,
                    &summary.thread_id,
                    Some(turn.turn_id.as_str()),
                    Some(item_id),
                    item.kind(),
                    item_search_title(item).as_deref(),
                    text,
                    &updated_at,
                    summary.cwd.as_ref().map(|path| path.display().to_string()),
                    counts,
                )?;
            }
        }
    }

    let manifest = build_manifest_record(detail, indexed_at);
    tx.execute(
        "
        INSERT INTO thread_manifest (
            thread_id,
            last_seen_updated_at,
            content_fingerprint,
            last_indexed_at,
            source_backend
        ) VALUES (?1, ?2, ?3, ?4, ?5)
        ",
        params![
            manifest.thread_id,
            manifest.last_seen_updated_at,
            manifest.content_fingerprint,
            manifest.last_indexed_at,
            manifest.source_backend,
        ],
    )
    .map_err(|error| {
        format!(
            "failed to insert thread manifest for {}: {error}",
            summary.thread_id
        )
    })?;
    counts.manifest_rows += 1;

    Ok(())
}

fn insert_search_doc(
    tx: &Transaction<'_>,
    thread_id: &str,
    turn_id: Option<&str>,
    item_id: Option<i64>,
    kind: &str,
    title: Option<&str>,
    text: &str,
    updated_at: &str,
    cwd: Option<String>,
    counts: &mut IndexCounts,
) -> Result<(), String> {
    tx.execute(
        "
        INSERT INTO search_docs (thread_id, turn_id, item_id, kind, title, text, updated_at, cwd)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        ",
        params![thread_id, turn_id, item_id, kind, title, text, updated_at, cwd],
    )
    .map_err(|error| format!("failed to insert search doc for thread {thread_id}: {error}"))?;
    let doc_id = tx.last_insert_rowid();

    tx.execute(
        "
        INSERT INTO search_docs_fts (rowid, title, text, kind, thread_id, turn_id, cwd)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        ",
        params![doc_id, title, text, kind, thread_id, turn_id, cwd],
    )
    .map_err(|error| format!("failed to insert FTS search doc for thread {thread_id}: {error}"))?;
    counts.search_docs += 1;
    Ok(())
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
    use crate::index::schema::doctor;

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

        let doctor = doctor(&path).expect("doctor index");
        assert!(doctor.healthy);
        assert_eq!(doctor.threads, report.threads);
        assert_eq!(doctor.thread_manifest, report.manifest_rows);

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
