use std::path::PathBuf;
use std::process::Command;

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_codex-history"))
        .args(args)
        .output()
        .expect("binary should run")
}

fn run_with_fixture_root(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_codex-history"))
        .args(args)
        .env(
            "CODEX_HISTORY_HOME",
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/local_history/sample_root"),
        )
        .output()
        .expect("binary should run")
}

#[test]
fn top_level_help_goes_to_stdout() {
    let output = run(&["--help"]);
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("USAGE:"));
    assert!(output.stderr.is_empty());
}

#[test]
fn command_help_goes_to_stdout_after_global_flags() {
    let output = run(&["--json", "search", "--help"]);
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("codex-history search"));
    assert!(stdout.contains("search <query>"));
    assert!(output.stderr.is_empty());
}

#[test]
fn invalid_usage_goes_to_stderr_with_exit_code_2() {
    let output = run(&["show", "thr_123", "extra"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("error: unexpected argument: extra"));
    assert!(stderr.contains("Run `codex-history --help` for usage."));
}

#[test]
fn auto_backend_behaves_like_local_in_phase_one() {
    let local = run_with_fixture_root(&["--backend", "local", "list"]);
    let auto = run_with_fixture_root(&["--backend", "auto", "list"]);

    assert_eq!(local.status.code(), Some(0));
    assert_eq!(auto.status.code(), Some(0));
    assert_eq!(local.stdout, auto.stdout);
    assert_eq!(local.stderr, auto.stderr);
}
