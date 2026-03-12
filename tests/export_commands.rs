use std::path::PathBuf;
use std::process::Command;

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
