use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

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

fn run_with_home(args: &[&str], home: &PathBuf) -> std::process::Output {
    run_with_home_and_root(args, home, &fixture_root())
}

fn run_with_home_and_root(
    args: &[&str],
    home: &PathBuf,
    history_root: &Path,
) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_codex-history"))
        .args(args)
        .env("CODEX_HISTORY_HOME", history_root)
        .env("HOME", home)
        .output()
        .expect("binary should run")
}

fn temp_home(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("codex-history-home-{label}-{nanos}"));
    std::fs::create_dir_all(&path).expect("create temp home");
    path
}

fn temp_history_root(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("codex-history-history-{label}-{nanos}"));
    copy_dir_all(&fixture_root(), &root).expect("copy fixture root");
    root
}

fn copy_dir_all(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let destination = to.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_all(&entry.path(), &destination)?;
        } else {
            fs::copy(entry.path(), destination)?;
        }
    }
    Ok(())
}

fn write_new_thread_fixture(
    history_root: &Path,
    file_name: &str,
    thread_id: &str,
    query_text: &str,
) {
    write_new_thread_fixture_at(
        history_root,
        file_name,
        thread_id,
        query_text,
        "2026-03-11T15:00:00Z",
    );
}

fn write_new_thread_fixture_at(
    history_root: &Path,
    file_name: &str,
    thread_id: &str,
    query_text: &str,
    timestamp: &str,
) {
    let content = format!(
        concat!(
            "{{\"timestamp\":\"{timestamp}\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"{thread_id}\",\"timestamp\":\"{timestamp}\",\"cwd\":\"/workspace/overlay\",\"originator\":\"codex_cli_rs\",\"source\":\"fixture\",\"model_provider\":\"openai\"}}}}\n",
            "{{\"timestamp\":\"{timestamp}\",\"type\":\"event_msg\",\"payload\":{{\"type\":\"task_started\",\"turn_id\":\"turn_overlay_1\"}}}}\n",
            "{{\"timestamp\":\"{timestamp}\",\"type\":\"turn_context\",\"payload\":{{\"turn_id\":\"turn_overlay_1\",\"cwd\":\"/workspace/overlay\",\"model\":\"gpt-5-codex\"}}}}\n",
            "{{\"timestamp\":\"{timestamp}\",\"type\":\"event_msg\",\"payload\":{{\"type\":\"user_message\",\"turn_id\":\"turn_overlay_1\",\"message\":\"{query_text}\",\"images\":[],\"local_images\":[],\"text_elements\":[]}}}}\n",
            "{{\"timestamp\":\"{timestamp}\",\"type\":\"event_msg\",\"payload\":{{\"type\":\"task_complete\",\"turn_id\":\"turn_overlay_1\",\"last_agent_message\":\"{query_text}\"}}}}\n"
        ),
        timestamp = timestamp,
        thread_id = thread_id,
        query_text = query_text
    );
    fs::write(history_root.join("sessions").join(file_name), content).expect("write new thread");
}

fn replace_in_file(path: &Path, from: &str, to: &str) {
    let content = fs::read_to_string(path).expect("read file");
    let updated = content.replace(from, to);
    assert_ne!(content, updated, "replacement should change file");
    fs::write(path, updated).expect("write updated file");
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
    let output = run(&["--json", "grep", "leftover argv"]);
    assert!(output.status.success());

    let matches: serde_json::Value = serde_json::from_slice(&output.stdout).expect("json output");
    let entries = matches.as_array().expect("array");
    assert!(!entries.is_empty());
    assert!(entries.iter().any(|entry| entry["kind"] == "agent_message"));

    let shell_output = run(&["--json", "grep", "--include-tools", "help ok"]);
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
fn grep_human_output_groups_matches_by_thread() {
    let output = run(&["grep", "leftover argv"]);
    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    assert!(stdout.contains("1. thread_id: thr_simple"));
    assert!(stdout.contains("first prompt: Please inspect the parser regression."));
    assert!(stdout.contains("cwd: /workspace/sample"));
    assert!(stdout.contains("hits: 1"));
    assert!(stdout.contains("occurrences: 2"));
    assert!(stdout.contains("matched in: assistant"));
    assert!(stdout.contains("preview: I found the leftover argv issue."));
    assert!(!stdout.contains('\t'));
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

#[test]
fn index_build_creates_db_and_doctor_reports_counts() {
    let home = temp_home("index-build");

    let build_output = run_with_home(&["--json", "index", "build"], &home);
    assert!(build_output.status.success());
    let build_report: serde_json::Value =
        serde_json::from_slice(&build_output.stdout).expect("json output");
    assert_eq!(build_report["threads"], 3);
    assert!(build_report["search_docs"].as_u64().expect("search docs") >= 8);

    let index_path = build_report["path"].as_str().expect("index path");
    assert!(std::path::Path::new(index_path).exists());

    let doctor_output = run_with_home(&["--json", "index", "doctor"], &home);
    assert!(doctor_output.status.success());
    let doctor_report: serde_json::Value =
        serde_json::from_slice(&doctor_output.stdout).expect("json output");
    assert_eq!(doctor_report["exists"], true);
    assert_eq!(doctor_report["healthy"], true);
    assert_eq!(doctor_report["threads"], 3);

    std::fs::remove_dir_all(home).expect("cleanup temp home");
}

#[test]
fn search_requires_index_build_first() {
    let home = temp_home("search-missing-index");

    let output = run_with_home(&["search", "leftover argv"], &home);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("run `codex-history index build` first"));

    std::fs::remove_dir_all(home).expect("cleanup temp home");
}

#[test]
fn missing_index_error_sanitizes_home_path_in_stderr() {
    let home = temp_home("search-missing-index-redacted");

    let output = run_with_home(&["search", "help ok"], &home);
    assert_eq!(output.status.code(), Some(1));

    let stderr = String::from_utf8(output.stderr).expect("utf8 stderr");
    assert!(!stderr.contains(&home.display().to_string()));
    if cfg!(target_os = "macos") {
        assert!(stderr.contains("~/Library/Application Support/codex-history/index.sqlite"));
    } else {
        assert!(stderr.contains("~/.local/share/codex-history/index.sqlite"));
    }

    std::fs::remove_dir_all(home).expect("cleanup temp home");
}

#[test]
fn search_reads_ranked_results_from_index() {
    let home = temp_home("search-index");

    let build_output = run_with_home(&["--json", "index", "build"], &home);
    assert!(build_output.status.success());

    let search_output = run_with_home(&["--json", "search", "leftover argv"], &home);
    assert!(search_output.status.success());
    let results: serde_json::Value =
        serde_json::from_slice(&search_output.stdout).expect("json output");
    let entries = results.as_array().expect("results array");
    assert!(!entries.is_empty());
    assert_eq!(entries[0]["thread_id"], "thr_simple");
    assert_eq!(entries[0]["kind"], "agent_message");
    assert!(entries[0]["text"]
        .as_str()
        .expect("result text")
        .contains("leftover argv"));

    std::fs::remove_dir_all(home).expect("cleanup temp home");
}

#[test]
fn search_human_output_groups_matches_by_thread() {
    let home = temp_home("search-human");

    let build_output = run_with_home(&["--json", "index", "build"], &home);
    assert!(build_output.status.success());

    let output = run_with_home(&["search", "leftover argv"], &home);
    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    assert!(stdout.contains("1. thread_id: thr_simple"));
    assert!(stdout.contains("first prompt: Please inspect the parser regression."));
    assert!(stdout.contains("cwd: /workspace/sample"));
    assert!(stdout.contains("hits: 1"));
    assert!(stdout.contains("occurrences: 2"));
    assert!(stdout.contains("matched in: assistant"));
    assert!(stdout.contains("best score:"));
    assert!(stdout.contains("preview:"));
    assert!(stdout.contains("leftover argv"));
    assert!(!stdout.contains('\t'));

    std::fs::remove_dir_all(home).expect("cleanup temp home");
}

#[test]
fn list_uses_session_index_thread_name_when_available() {
    let home = temp_home("session-index-name");
    let codex_home = home.join(".codex");
    std::fs::create_dir_all(&codex_home).expect("create codex home");
    std::fs::write(
        codex_home.join("session_index.jsonl"),
        "{\"id\":\"thr_simple\",\"thread_name\":\"Named From Session Index\",\"updated_at\":\"2026-03-12T00:00:00Z\"}\n",
    )
    .expect("write session index");

    let output = run_with_home(&["list"], &home);
    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    assert!(stdout.contains("thr_simple\tNamed From Session Index\t"));

    std::fs::remove_dir_all(home).expect("cleanup temp home");
}

#[test]
fn index_refresh_upserts_changed_and_new_threads_while_skipping_unchanged() {
    let home = temp_home("index-refresh");
    let history_root = temp_history_root("index-refresh");

    let build_output = run_with_home_and_root(&["--json", "index", "build"], &home, &history_root);
    assert!(build_output.status.success());

    replace_in_file(
        &history_root.join("sessions").join("thr_simple.jsonl"),
        "\"output\":\"ok\"",
        "\"output\":\"refresh unique output\"",
    );
    write_new_thread_fixture(
        &history_root,
        "thr_refresh_new.jsonl",
        "thr_refresh_new",
        "refresh overlay query",
    );

    let refresh_output =
        run_with_home_and_root(&["--json", "index", "refresh"], &home, &history_root);
    assert!(refresh_output.status.success());
    let report: serde_json::Value =
        serde_json::from_slice(&refresh_output.stdout).expect("json output");
    assert_eq!(report["new_threads"], 1);
    assert_eq!(report["changed_threads"], 1);
    assert_eq!(report["unchanged_threads"], 2);
    assert_eq!(report["indexed_threads"], 2);
    assert!(report["watermark"].is_string());

    let changed_search = run_with_home_and_root(
        &[
            "--json",
            "search",
            "--include-tools",
            "refresh unique output",
        ],
        &home,
        &history_root,
    );
    assert!(changed_search.status.success());
    let changed_results: serde_json::Value =
        serde_json::from_slice(&changed_search.stdout).expect("json output");
    assert!(changed_results
        .as_array()
        .expect("array")
        .iter()
        .any(|entry| entry["thread_id"] == "thr_simple"));

    let new_search = run_with_home_and_root(
        &["--json", "search", "refresh overlay query"],
        &home,
        &history_root,
    );
    assert!(new_search.status.success());
    let new_results: serde_json::Value =
        serde_json::from_slice(&new_search.stdout).expect("json output");
    assert!(new_results
        .as_array()
        .expect("array")
        .iter()
        .any(|entry| entry["thread_id"] == "thr_refresh_new"));

    std::fs::remove_dir_all(home).expect("cleanup temp home");
    std::fs::remove_dir_all(history_root).expect("cleanup temp history");
}

#[test]
fn search_fresh_merges_overlay_results_without_duplicates() {
    let home = temp_home("search-fresh");
    let history_root = temp_history_root("search-fresh");

    let build_output = run_with_home_and_root(&["--json", "index", "build"], &home, &history_root);
    assert!(build_output.status.success());

    replace_in_file(
        &history_root.join("sessions").join("thr_simple.jsonl"),
        "\"message\":\"I found the leftover argv issue.\"",
        "\"message\":\"I found the leftover argv issue and kept help ok.\"",
    );
    write_new_thread_fixture(
        &history_root,
        "thr_overlay_new.jsonl",
        "thr_overlay_new",
        "help ok",
    );

    let plain_output = run_with_home_and_root(
        &["--json", "search", "--include-tools", "help ok"],
        &home,
        &history_root,
    );
    assert!(plain_output.status.success());
    let plain_results: serde_json::Value =
        serde_json::from_slice(&plain_output.stdout).expect("json output");
    assert!(!plain_results
        .as_array()
        .expect("array")
        .iter()
        .any(|entry| entry["thread_id"] == "thr_overlay_new"));

    let fresh_output = run_with_home_and_root(
        &["--json", "search", "--fresh", "help ok"],
        &home,
        &history_root,
    );
    assert!(fresh_output.status.success());
    let fresh_results: serde_json::Value =
        serde_json::from_slice(&fresh_output.stdout).expect("json output");
    let entries = fresh_results.as_array().expect("array");
    assert!(entries
        .iter()
        .any(|entry| entry["thread_id"] == "thr_overlay_new"));
    assert_eq!(
        entries
            .iter()
            .filter(|entry| entry["thread_id"] == "thr_simple" && entry["kind"] == "agent_message")
            .count(),
        1
    );

    let fresh_tool_output = run_with_home_and_root(
        &["--json", "search", "--fresh", "--include-tools", "help ok"],
        &home,
        &history_root,
    );
    assert!(fresh_tool_output.status.success());
    let fresh_tool_results: serde_json::Value =
        serde_json::from_slice(&fresh_tool_output.stdout).expect("json output");
    let tool_entries = fresh_tool_results.as_array().expect("array");
    assert_eq!(
        tool_entries
            .iter()
            .filter(
                |entry| entry["thread_id"] == "thr_simple" && entry["kind"] == "command_execution"
            )
            .count(),
        1
    );

    std::fs::remove_dir_all(home).expect("cleanup temp home");
    std::fs::remove_dir_all(history_root).expect("cleanup temp history");
}

#[test]
fn search_fresh_does_not_match_overlay_substrings_as_tokens() {
    let home = temp_home("search-fresh-substring");
    let history_root = temp_history_root("search-fresh-substring");

    let build_output = run_with_home_and_root(&["--json", "index", "build"], &home, &history_root);
    assert!(build_output.status.success());

    write_new_thread_fixture(
        &history_root,
        "thr_overlay_substring.jsonl",
        "thr_overlay_substring",
        "helped okay",
    );

    let fresh_output = run_with_home_and_root(
        &["--json", "search", "--fresh", "help ok"],
        &home,
        &history_root,
    );
    assert!(fresh_output.status.success());
    let fresh_results: serde_json::Value =
        serde_json::from_slice(&fresh_output.stdout).expect("json output");
    assert!(!fresh_results
        .as_array()
        .expect("array")
        .iter()
        .any(|entry| entry["thread_id"] == "thr_overlay_substring"));

    std::fs::remove_dir_all(home).expect("cleanup temp home");
    std::fs::remove_dir_all(history_root).expect("cleanup temp history");
}

#[test]
fn search_fresh_keeps_fetching_index_hits_after_filtering_changed_threads() {
    let home = temp_home("search-fresh-backfill");
    let history_root = temp_history_root("search-fresh-backfill");

    for index in 0..130 {
        write_new_thread_fixture_at(
            &history_root,
            &format!("thr_changed_{index:03}.jsonl"),
            &format!("thr_changed_{index:03}"),
            "limit filler",
            "2026-03-11T15:00:00Z",
        );
    }
    write_new_thread_fixture_at(
        &history_root,
        "thr_unchanged_keeper.jsonl",
        "thr_unchanged_keeper",
        "limit filler",
        "2026-03-11T14:00:00Z",
    );

    let build_output = run_with_home_and_root(&["--json", "index", "build"], &home, &history_root);
    assert!(build_output.status.success());

    for index in 0..130 {
        replace_in_file(
            &history_root
                .join("sessions")
                .join(format!("thr_changed_{index:03}.jsonl")),
            "limit filler",
            "stale replacement",
        );
    }

    let fresh_output = run_with_home_and_root(
        &["--json", "search", "--fresh", "limit filler"],
        &home,
        &history_root,
    );
    assert!(fresh_output.status.success());
    let fresh_results: serde_json::Value =
        serde_json::from_slice(&fresh_output.stdout).expect("json output");
    assert!(fresh_results
        .as_array()
        .expect("array")
        .iter()
        .any(|entry| entry["thread_id"] == "thr_unchanged_keeper"));

    std::fs::remove_dir_all(home).expect("cleanup temp home");
    std::fs::remove_dir_all(history_root).expect("cleanup temp history");
}
