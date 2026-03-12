use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/local_history/sample_root")
}

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_codex-history"))
        .args(args)
        .env("CODEX_HISTORY_HOME", fixture_root())
        .output()
        .expect("binary should run")
}

fn run_with_home_and_root(args: &[&str], home: &Path, history_root: &Path) -> std::process::Output {
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
    let path = std::env::temp_dir().join(format!("codex-history-export-home-{label}-{nanos}"));
    fs::create_dir_all(&path).expect("create temp home");
    path
}

fn temp_history_root(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("codex-history-export-history-{label}-{nanos}"));
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

fn replace_in_file(path: &Path, from: &str, to: &str) {
    let content = fs::read_to_string(path).expect("read file");
    let updated = content.replace(from, to);
    assert_ne!(content, updated, "replacement should change file");
    fs::write(path, updated).expect("write updated file");
}

#[test]
fn export_json_outputs_canonical_document() {
    let output = run(&["export", "thr_command", "--format", "json"]);
    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let expected = r#"{
  "format": "json",
  "thread": {
    "thread_id": "thr_command",
    "preview": "Run grep without an index.",
    "created_at": "2026-03-11T10:00:00Z",
    "updated_at": "2026-03-11T10:10:00Z",
    "cwd": "/workspace/commands",
    "source_kind": "fixture",
    "model_provider": "openai",
    "status": "completed",
    "turns": [
      {
        "turn_id": "turn_command_1",
        "status": "completed",
        "started_at": "2026-03-11T10:00:01Z",
        "completed_at": "2026-03-11T10:05:00Z",
        "items": [
          {
            "kind": "user_message",
            "text": "Run grep without an index."
          },
          {
            "kind": "command_execution",
            "call_id": "call_command_1",
            "command": "rg -n \"cargo test\" src tests",
            "cwd": "/workspace/commands",
            "output": "src/lib.rs:10:cargo test"
          },
          {
            "kind": "reasoning_summary",
            "text": "Literal grep can work directly over session logs."
          }
        ]
      },
      {
        "turn_id": "turn_command_2",
        "status": "completed",
        "started_at": "2026-03-11T10:06:00Z",
        "completed_at": "2026-03-11T10:10:00Z",
        "items": [
          {
            "kind": "agent_message",
            "text": "Search the transcript directly before adding an index."
          }
        ]
      }
    ],
    "items_count": 4,
    "commands_count": 1,
    "files_changed_count": 0
  }
}
"#;

    assert_eq!(stdout, expected);
}

#[test]
fn export_markdown_outputs_human_readable_sections() {
    let output = run(&["export", "thr_command", "--format", "markdown"]);
    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let expected = r#"# Thread Export

## Metadata
- Thread ID: thr_command
- Name: (unnamed)
- Preview: Run grep without an index.
- Created At: 2026-03-11T10:00:00+00:00
- Updated At: 2026-03-11T10:10:00+00:00
- Status: completed
- Cwd: `/workspace/commands`
- Source Kind: fixture
- Model Provider: openai
- Turns: 2
- Items: 4
- Commands: 1
- Files Changed: 0

## Turns
### turn_command_1
- Status: completed
- Started At: 2026-03-11T10:00:01+00:00
- Completed At: 2026-03-11T10:05:00+00:00
- User Message: Run grep without an index.
- Command: `rg -n "cargo test" src tests`
- Command Cwd: `/workspace/commands`
- Command Output: src/lib.rs:10:cargo test
- Reasoning Summary: Literal grep can work directly over session logs.

### turn_command_2
- Status: completed
- Started At: 2026-03-11T10:06:00+00:00
- Completed At: 2026-03-11T10:10:00+00:00
- Agent Message: Search the transcript directly before adding an index.

## Commands
### Command 1
- Command: `rg -n "cargo test" src tests`
- Cwd: `/workspace/commands`
- Output: src/lib.rs:10:cargo test

## File Changes
- (none)

## Extracted Notes
- Literal grep can work directly over session logs.
- Search the transcript directly before adding an index.
"#;

    assert_eq!(stdout, expected);
}

#[test]
fn export_prompt_pack_outputs_compact_handoff() {
    let output = run(&["export", "thr_command", "--format", "prompt-pack"]);
    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let expected = r#"OBJECTIVE
Run grep without an index.

KEY CONTEXT
- thread_id: thr_command
- created_at: 2026-03-11T10:00:00+00:00
- updated_at: 2026-03-11T10:10:00+00:00
- status: completed
- cwd: /workspace/commands
- source_kind: fixture
- model_provider: openai
- turns: 2
- items: 4
- commands: 1
- files_changed: 0

COMMANDS SEEN
- rg -n "cargo test" src tests

FILES TOUCHED
- (none)

NOTABLE ERRORS
- (none)

USEFUL FOLLOW-UPS
- Literal grep can work directly over session logs.
- Search the transcript directly before adding an index.

"#;

    assert_eq!(stdout, expected);
}

#[test]
fn export_json_honors_global_ndjson_flag() {
    let output = run(&["--ndjson", "export", "thr_command", "--format", "json"]);
    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let line = stdout.trim_end_matches('\n');
    assert!(!line.contains('\n'));
    assert!(line.contains("\"format\":\"json\""));
    assert!(line.contains("\"thread_id\":\"thr_command\""));
}

#[test]
fn export_markdown_rejects_global_json_flag() {
    let output = run(&["--json", "export", "thr_command", "--format", "markdown"]);
    assert!(!output.status.success());

    let stderr = String::from_utf8(output.stderr).expect("utf8 stderr");
    assert!(stderr.contains("error: cannot combine --json with `export --format markdown`"));
}

#[test]
fn export_markdown_redacts_secrets_and_sanitizes_home_paths() {
    let home = temp_home("redaction-human");
    let history_root = temp_history_root("redaction-human");
    let thread_path = history_root.join("sessions").join("thr_command.jsonl");
    let home_project = home.join("project");
    let home_project_text = home_project.display().to_string();

    replace_in_file(&thread_path, "/workspace/commands", &home_project_text);
    replace_in_file(
        &thread_path,
        "Run grep without an index.",
        "Use api_key=plainsecret123456 and JWT eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkNvZGV4In0.signaturepayload1234567890",
    );
    replace_in_file(
        &thread_path,
        "src/lib.rs:10:cargo test",
        "Authorization: Bearer sk-live_1234567890abcdefghijklmnop ghp_123456789012345678901234567890123456",
    );

    let output = run_with_home_and_root(
        &["export", "thr_command", "--format", "markdown"],
        &home,
        &history_root,
    );
    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    assert!(stdout.contains("`~/project`"));
    assert!(!stdout.contains(&home_project_text));
    assert!(!stdout.contains("plainsecret123456"));
    assert!(!stdout.contains("signaturepayload1234567890"));
    assert!(!stdout.contains("sk-live_1234567890abcdefghijklmnop"));
    assert!(!stdout.contains("ghp_123456789012345678901234567890123456"));
    assert!(stdout.contains("api_key=[REDACTED]"));
    assert!(stdout.contains("Bearer [REDACTED]"));

    fs::remove_dir_all(home).expect("cleanup temp home");
    fs::remove_dir_all(history_root).expect("cleanup temp history");
}

#[test]
fn export_json_redaction_keeps_valid_json() {
    let home = temp_home("redaction-json");
    let history_root = temp_history_root("redaction-json");
    let thread_path = history_root.join("sessions").join("thr_command.jsonl");
    let home_project = home.join("project");

    replace_in_file(
        &thread_path,
        "Run grep without an index.",
        "Use api_key=plainsecret123456 and JWT eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkNvZGV4In0.signaturepayload1234567890",
    );
    replace_in_file(
        &thread_path,
        "src/lib.rs:10:cargo test",
        "Authorization: Bearer sk-live_1234567890abcdefghijklmnop",
    );
    replace_in_file(
        &thread_path,
        "/workspace/commands",
        &home_project.display().to_string(),
    );

    let output = run_with_home_and_root(
        &["export", "thr_command", "--format", "json"],
        &home,
        &history_root,
    );
    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let document: serde_json::Value = serde_json::from_str(&stdout).expect("valid json");
    let items = document["thread"]["turns"][0]["items"]
        .as_array()
        .expect("items array");

    assert_eq!(
        items[0]["text"],
        "Use api_key=[REDACTED] and JWT [REDACTED]"
    );
    assert_eq!(items[1]["output"], "Authorization: Bearer [REDACTED]");
    assert_eq!(
        document["thread"]["cwd"],
        serde_json::Value::String(home_project.display().to_string())
    );

    fs::remove_dir_all(home).expect("cleanup temp home");
    fs::remove_dir_all(history_root).expect("cleanup temp history");
}
