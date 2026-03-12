# `codex-history`

<div align="center">
  <strong>Read-only CLI for locally accessible Codex session history.</strong>
  <br />
  Search transcripts, inspect threads, export handoffs, and build an optional local index.
</div>

## Quick Start

Homebrew support is being prepared through the separate `nishantdesai/homebrew-tap` repository.
Because this main repository is still private, the tap is scaffolded but not yet installable from a public release.

When the first public release is published, users will be able to install directly with:

```bash
brew install nishantdesai/tap/codex-history
```

From source today:

```bash
cargo build --release
./target/release/codex-history --help
```

## What It Does

- reads local Codex history only
- never mutates or uploads your history
- supports direct transcript scanning with `grep`
- supports ranked indexed search with `search`
- exports threads as JSON, Markdown, or prompt-pack handoff format
- redacts obvious secrets in human-readable output and keeps JSON output structurally valid

## Command Surface

```bash
codex-history --help
codex-history --version
codex-history list
codex-history show <thread-id>
codex-history show --include-turns <thread-id>
codex-history search <query>
codex-history search --fresh <query>
codex-history search --include-thinking <query>
codex-history search --include-tools <query>
codex-history search --compact <query>
codex-history grep <pattern>
codex-history grep --regex <pattern>
codex-history grep --include-thinking <pattern>
codex-history grep --include-tools <pattern>
codex-history grep --compact <pattern>
codex-history export <thread-id> --format json
codex-history export <thread-id> --format markdown
codex-history export <thread-id> --format prompt-pack
codex-history doctor
codex-history index build
codex-history index refresh
codex-history index doctor
```

## Search Modes

`search` is the indexed path.
After `index build`, it is generally much faster than transcript scanning.

`grep` is the direct local scan path.
It does not require an index, but it has to walk and parse local history data each time.

Default search scope is conservative:

- `grep` and `search` include user and assistant message content by default
- `--include-thinking` opts into reasoning content
- `--include-tools` opts into tool and command content

Human-readable output modes:

- default output groups results by thread and prints full matched text blocks per hit
- `--compact` prints one markdown-table row per hit with source, thread, cwd, and preview
- `--json` and `--ndjson` remain the machine-facing interfaces

Thread display names come from parsed session metadata when present, and may also be filled from `~/.codex/session_index.jsonl`.

## Examples

```bash
codex-history search "sqlite3_open_v2"
codex-history search --fresh "deadlock"
codex-history search --include-tools "cargo test"
codex-history search --compact "deadlock"

codex-history grep "leftover argv"
codex-history grep --include-thinking "planner"
codex-history grep --compact "deadlock"

codex-history export thr_123 --format markdown
codex-history show --include-turns thr_123
```

## Packaging And Releases

Phase 8 packaging and release preparation is in progress in the main repository.

Implemented release-prep pieces:

- stable `codex-history --version` output
- native release archive packaging script
- SHA256 checksum generation script
- GitHub Actions release workflow for macOS archives and checksums
- maintainer release notes in [docs/RELEASING.md](docs/RELEASING.md)

Build a native release archive and checksums locally:

```bash
make package-release
make release-checksums
```

See [docs/RELEASING.md](docs/RELEASING.md) for archive layout, local validation, and the handoff flow to `nishantdesai/homebrew-tap`.

## Current Status

- local-first backend is implemented
- export formats are implemented
- privacy and redaction are implemented
- packaging and release preparation is underway
- App Server support is still deferred

## Privacy Posture

- read-only only
- no telemetry
- no background daemon
- synthetic fixtures only in tests
- obvious secrets are redacted conservatively in human-readable output
- home-directory-style paths are sanitized where practical
- redacted JSON output remains structurally valid

## Repository Docs

- [docs/SPEC.md](docs/SPEC.md) — architecture and scope
- [docs/RELEASING.md](docs/RELEASING.md) — release archives, checksums, and Homebrew-tap handoff
- [AGENTS.md](AGENTS.md) — repo-specific instructions for agents

## License

MIT
