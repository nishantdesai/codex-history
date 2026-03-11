use std::collections::hash_map::DefaultHasher;
use std::env;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use chrono::Utc;
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
}
