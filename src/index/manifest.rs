use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::env;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use chrono::Utc;
use rusqlite::{params, Connection};
use serde::Serialize;

use crate::model::ThreadDetail;

pub const INDEX_SCHEMA_VERSION: i64 = 1;
pub const INDEX_SOURCE_BACKEND_LOCAL: &str = "local";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ThreadManifestRecord {
    pub thread_id: String,
    pub last_seen_updated_at: String,
    pub content_fingerprint: String,
    pub last_indexed_at: String,
    pub source_backend: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadFreshness {
    New,
    Changed,
    Unchanged,
}

pub fn default_index_path() -> PathBuf {
    if let Some(home) = env::var_os("HOME").map(PathBuf::from) {
        return default_index_path_from_home(&home);
    }

    PathBuf::from("index.sqlite")
}

pub fn default_index_path_from_home(home: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support/codex-history/index.sqlite")
    } else {
        home.join(".local/share/codex-history/index.sqlite")
    }
}

pub fn ensure_index_parent_dir(path: &Path) -> Result<(), String> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };

    std::fs::create_dir_all(parent).map_err(|error| {
        format!(
            "failed to create index directory {}: {error}",
            parent.display()
        )
    })
}

pub fn build_manifest_record(detail: &ThreadDetail, indexed_at: &str) -> ThreadManifestRecord {
    ThreadManifestRecord {
        thread_id: detail.summary.thread_id.clone(),
        last_seen_updated_at: detail
            .summary
            .updated_at
            .unwrap_or(detail.summary.created_at)
            .to_rfc3339(),
        content_fingerprint: thread_fingerprint(detail),
        last_indexed_at: indexed_at.to_string(),
        source_backend: INDEX_SOURCE_BACKEND_LOCAL.to_string(),
    }
}

pub fn current_timestamp() -> String {
    Utc::now().to_rfc3339()
}

pub fn thread_fingerprint(detail: &ThreadDetail) -> String {
    let serialized =
        serde_json::to_string(detail).expect("thread detail should serialize for fingerprinting");
    let mut hasher = DefaultHasher::new();
    serialized.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

pub fn load_manifest(conn: &Connection) -> Result<HashMap<String, ThreadManifestRecord>, String> {
    let mut statement = conn
        .prepare(
            "
            SELECT
                thread_id,
                last_seen_updated_at,
                content_fingerprint,
                last_indexed_at,
                source_backend
            FROM thread_manifest
            ",
        )
        .map_err(|error| format!("failed to prepare thread manifest query: {error}"))?;

    let rows = statement
        .query_map([], |row| {
            Ok(ThreadManifestRecord {
                thread_id: row.get(0)?,
                last_seen_updated_at: row.get(1)?,
                content_fingerprint: row.get(2)?,
                last_indexed_at: row.get(3)?,
                source_backend: row.get(4)?,
            })
        })
        .map_err(|error| format!("failed to load thread manifest rows: {error}"))?;

    let mut records = HashMap::new();
    for row in rows {
        let record =
            row.map_err(|error| format!("failed to decode thread manifest row: {error}"))?;
        records.insert(record.thread_id.clone(), record);
    }
    Ok(records)
}

pub fn store_manifest_record(
    conn: &Connection,
    record: &ThreadManifestRecord,
) -> Result<(), String> {
    conn.execute(
        "
        INSERT INTO thread_manifest (
            thread_id,
            last_seen_updated_at,
            content_fingerprint,
            last_indexed_at,
            source_backend
        ) VALUES (?1, ?2, ?3, ?4, ?5)
        ON CONFLICT(thread_id) DO UPDATE SET
            last_seen_updated_at = excluded.last_seen_updated_at,
            content_fingerprint = excluded.content_fingerprint,
            last_indexed_at = excluded.last_indexed_at,
            source_backend = excluded.source_backend
        ",
        params![
            record.thread_id,
            record.last_seen_updated_at,
            record.content_fingerprint,
            record.last_indexed_at,
            record.source_backend,
        ],
    )
    .map_err(|error| {
        format!(
            "failed to store thread manifest for {}: {error}",
            record.thread_id
        )
    })?;
    Ok(())
}

pub fn classify_thread(
    current: &ThreadManifestRecord,
    previous: Option<&ThreadManifestRecord>,
) -> ThreadFreshness {
    match previous {
        None => ThreadFreshness::New,
        Some(previous)
            if previous.last_seen_updated_at == current.last_seen_updated_at
                && previous.content_fingerprint == current.content_fingerprint =>
        {
            ThreadFreshness::Unchanged
        }
        Some(_) => ThreadFreshness::Changed,
    }
}

pub fn manifest_watermark<'a, I>(records: I) -> Option<String>
where
    I: IntoIterator<Item = &'a ThreadManifestRecord>,
{
    records
        .into_iter()
        .map(|record| record.last_seen_updated_at.as_str())
        .max()
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::*;
    use crate::model::{ThreadSummary, Turn};

    #[test]
    fn default_index_path_uses_platform_layout() {
        let home = PathBuf::from("/tmp/codex-history-home");
        let path = default_index_path_from_home(&home);

        if cfg!(target_os = "macos") {
            assert_eq!(
                path,
                PathBuf::from("/tmp/codex-history-home/Library/Application Support/codex-history/index.sqlite")
            );
        } else {
            assert_eq!(
                path,
                PathBuf::from("/tmp/codex-history-home/.local/share/codex-history/index.sqlite")
            );
        }
    }

    #[test]
    fn thread_fingerprint_changes_with_content() {
        let base = ThreadDetail {
            summary: ThreadSummary {
                thread_id: "thr_1".into(),
                name: Some("Example".into()),
                preview: Some("preview".into()),
                created_at: Utc.with_ymd_and_hms(2026, 3, 11, 9, 0, 0).unwrap(),
                updated_at: None,
                cwd: None,
                source_kind: None,
                model_provider: None,
                ephemeral: None,
                status: Some("completed".into()),
            },
            turns: vec![Turn {
                turn_id: "turn_1".into(),
                status: "completed".into(),
                started_at: None,
                completed_at: None,
                items: Vec::new(),
            }],
            items_count: 0,
            commands_count: 0,
            files_changed_count: 0,
        };

        let mut changed = base.clone();
        changed.summary.preview = Some("different".into());

        assert_ne!(thread_fingerprint(&base), thread_fingerprint(&changed));
    }

    #[test]
    fn classify_thread_uses_timestamp_and_fingerprint() {
        let base = ThreadManifestRecord {
            thread_id: "thr_1".into(),
            last_seen_updated_at: "2026-03-11T09:00:00+00:00".into(),
            content_fingerprint: "aaa".into(),
            last_indexed_at: "2026-03-11T09:01:00+00:00".into(),
            source_backend: INDEX_SOURCE_BACKEND_LOCAL.into(),
        };

        assert_eq!(classify_thread(&base, None), ThreadFreshness::New);
        assert_eq!(
            classify_thread(&base, Some(&base)),
            ThreadFreshness::Unchanged
        );

        let mut changed = base.clone();
        changed.content_fingerprint = "bbb".into();
        assert_eq!(
            classify_thread(&changed, Some(&base)),
            ThreadFreshness::Changed
        );

        let mut timestamp_changed = base.clone();
        timestamp_changed.last_seen_updated_at = "2026-03-11T09:05:00+00:00".into();
        assert_eq!(
            classify_thread(&timestamp_changed, Some(&base)),
            ThreadFreshness::Changed
        );
    }
}
