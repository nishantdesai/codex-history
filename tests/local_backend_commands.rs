use std::path::PathBuf;
use std::process::Command;

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_codex-history"))
        .args(args)
        .env("CODEX_HISTORY_HOME", fixture_root())
        .output()
        .expect("binary should run")
}

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/local_history/sample_root")
}

#[test]
fn list_reads_threads_from_local_fixture_root() {
    let output = run(&["--json", "list"]);
    assert!(output.status.success());

    let threads: serde_json::Value = serde_json::from_slice(&output.stdout).expect("json output");
    let thread_ids: Vec<_> = threads
        .as_array()
        .expect("array")
        .iter()
        .map(|entry| entry["thread_id"].as_str().expect("thread_id"))
        .collect();

    assert_eq!(
        thread_ids
            .iter()
            .filter(|thread_id| **thread_id == "thr_simple")
            .count(),
        1
    );
    assert!(thread_ids.contains(&"thr_simple"));
    assert!(thread_ids.contains(&"thr_command"));
    assert!(thread_ids.contains(&"thr_malformed"));
}

#[test]
fn show_returns_thread_detail_from_fixture() {
    let output = run(&["--json", "show", "--include-turns", "thr_simple"]);
    assert!(output.status.success());

    let detail: serde_json::Value = serde_json::from_slice(&output.stdout).expect("json output");
    assert_eq!(detail["thread_id"], "thr_simple");
    assert_eq!(detail["turns"].as_array().expect("turn array").len(), 3);
    assert_eq!(detail["commands_count"], 2);
}

#[test]
fn grep_works_without_an_index() {
    let output = run(&["--json", "grep", "cargo test"]);
    assert!(output.status.success());

    let matches: serde_json::Value = serde_json::from_slice(&output.stdout).expect("json output");
    let entries = matches.as_array().expect("array");
    assert!(!entries.is_empty());
    assert!(entries
        .iter()
        .any(|entry| entry["kind"] == "command_execution"));

    let shell_output = run(&["--json", "grep", "help ok"]);
    assert!(shell_output.status.success());
    let shell_matches: serde_json::Value =
        serde_json::from_slice(&shell_output.stdout).expect("json output");
    assert!(shell_matches
        .as_array()
        .expect("array")
        .iter()
        .any(|entry| entry["thread_id"] == "thr_simple" && entry["kind"] == "command_execution"));
}

#[test]
fn doctor_reports_local_history_discovery_information() {
    let output = run(&["--json", "doctor"]);
    assert!(output.status.success());

    let report: serde_json::Value = serde_json::from_slice(&output.stdout).expect("json output");
    assert_eq!(report["parsed_threads"], 3);
    assert!(report["malformed_files"].as_u64().expect("malformed files") >= 2);
    assert!(report["malformed_lines"].as_u64().expect("malformed lines") >= 2);
    assert_eq!(report["roots"].as_array().expect("roots").len(), 1);
    assert!(report["warnings"]
        .as_array()
        .expect("warnings")
        .iter()
        .any(|warning| warning
            .as_str()
            .expect("warning str")
            .contains("thr_missing_meta.jsonl: missing valid thread metadata")));
}

#[test]
fn show_ndjson_emits_single_compact_json_line() {
    let output = run(&["--ndjson", "show", "--include-turns", "thr_simple"]);
    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let lines: Vec<_> = stdout.lines().collect();
    assert_eq!(lines.len(), 1);
    let detail: serde_json::Value = serde_json::from_str(lines[0]).expect("json line");
    assert_eq!(detail["thread_id"], "thr_simple");
}

#[test]
fn doctor_ndjson_emits_single_compact_json_line() {
    let output = run(&["--ndjson", "doctor"]);
    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let lines: Vec<_> = stdout.lines().collect();
    assert_eq!(lines.len(), 1);
    let report: serde_json::Value = serde_json::from_str(lines[0]).expect("json line");
    assert_eq!(report["parsed_threads"], 3);
}
