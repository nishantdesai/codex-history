use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HistoryRootCandidate {
    pub path: PathBuf,
    pub source: String,
    pub exists: bool,
}

pub fn discover_history_roots() -> Vec<HistoryRootCandidate> {
    let mut roots = Vec::new();

    if let Some(root) = env::var_os("CODEX_HISTORY_HOME") {
        let path = PathBuf::from(root);
        roots.push(HistoryRootCandidate {
            exists: path.exists(),
            path,
            source: "CODEX_HISTORY_HOME".into(),
        });
        return roots;
    }

    let mut seen = BTreeSet::new();

    if let Some(home) = env::var_os("HOME").map(PathBuf::from) {
        for (path, source) in [
            (home.join(".codex/history"), "default:~/.codex/history"),
            (home.join(".codex/sessions"), "default:~/.codex/sessions"),
            (
                home.join(".local/share/codex/history"),
                "default:~/.local/share/codex/history",
            ),
            (
                home.join("Library/Application Support/Codex/history"),
                "default:~/Library/Application Support/Codex/history",
            ),
            (
                home.join("Library/Application Support/Codex/sessions"),
                "default:~/Library/Application Support/Codex/sessions",
            ),
        ] {
            if seen.insert(path.clone()) {
                roots.push(HistoryRootCandidate {
                    exists: path.exists(),
                    path,
                    source: source.into(),
                });
            }
        }
    }

    roots
}

pub fn collect_session_log_files(root: &Path) -> (Vec<PathBuf>, Vec<String>) {
    let mut files = Vec::new();
    let mut warnings = Vec::new();
    walk(root, &mut files, &mut warnings);
    files.sort();
    (files, warnings)
}

fn walk(path: &Path, files: &mut Vec<PathBuf>, warnings: &mut Vec<String>) {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) => {
            warnings.push(format!("cannot inspect {}: {error}", path.display()));
            return;
        }
    };

    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        return;
    }

    if file_type.is_file() {
        if path.extension().and_then(|value| value.to_str()) == Some("jsonl") {
            files.push(path.to_path_buf());
        }
        return;
    }

    if !file_type.is_dir() {
        return;
    }

    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) => {
            warnings.push(format!("cannot read directory {}: {error}", path.display()));
            return;
        }
    };

    for entry in entries {
        match entry {
            Ok(entry) => walk(&entry.path(), files, warnings),
            Err(error) => warnings.push(format!(
                "cannot read directory entry under {}: {error}",
                path.display()
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    #[test]
    fn collects_jsonl_files_recursively_and_skips_other_extensions() {
        let root = temp_dir("collect_jsonl");
        fs::create_dir_all(root.join("nested")).expect("create nested dir");
        fs::write(root.join("root.jsonl"), "{}\n").expect("write root jsonl");
        fs::write(root.join("nested/turns.jsonl"), "{}\n").expect("write nested jsonl");
        fs::write(root.join("nested/ignore.txt"), "nope").expect("write non-jsonl");

        let (files, warnings) = collect_session_log_files(&root);
        assert!(warnings.is_empty());
        assert_eq!(files.len(), 2);
        assert!(files.iter().any(|path| path.ends_with("root.jsonl")));
        assert!(files.iter().any(|path| path.ends_with("turns.jsonl")));

        fs::remove_dir_all(root).expect("cleanup");
    }

    fn temp_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        env::temp_dir().join(format!("codex-history-{label}-{nanos}"))
    }
}
