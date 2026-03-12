# codex-history

A read-only CLI for locally accessible Codex session history, with search, export, and optional indexing.

## Current status

Phase 6 export formats are in place.

Current behavior:
- the Rust crate builds and passes CI checks
- `codex-history --help` works
- the CLI supports local-history `list`, `show`, `grep`, and `doctor`
- `index build` creates an opt-in local SQLite FTS index from local session history
- `index refresh` upserts only new or changed threads using manifest tracking
- `index doctor` reports index presence, schema version, and core row counts
- `search <query>` reads ranked results from the local index
- `search --fresh <query>` overlays newer or changed local threads on top of the index
- `grep` and `search` search only user and assistant message content by default
- `grep --include-thinking` and `search --include-thinking` opt into reasoning content
- `grep --include-tools` and `search --include-tools` opt into command/tool content
- default human-readable `grep` and `search` output is grouped by thread, shows `thread_id`, prefers a thread name when available, and otherwise falls back to the first user prompt
- `export <thread-id> --format <json|markdown|prompt-pack>` renders canonical thread detail in three deterministic export formats
- command-specific help is available with `codex-history <command> --help`

Implemented command surface today:

```bash
codex-history --help
codex-history list
codex-history show <thread-id>
codex-history show --include-turns <thread-id>
codex-history search <query>
codex-history search --fresh <query>
codex-history search --include-thinking <query>
codex-history search --include-tools <query>
codex-history grep <pattern>
codex-history grep --regex <pattern>
codex-history grep --include-thinking <pattern>
codex-history grep --include-tools <pattern>
codex-history export <thread-id> --format json
codex-history export <thread-id> --format markdown
codex-history export <thread-id> --format prompt-pack
codex-history doctor
codex-history index build
codex-history index refresh
codex-history index doctor
```

The parser is intentionally strict:
- malformed command lines return errors
- invalid flag combinations return non-zero exit codes
- top-level help works after normal global flag orderings
- command help goes to stdout and invalid usage goes to stderr

## What it does not do

- mutate Codex history
- delete or archive sessions
- sync your history anywhere
- require Codex App Server to always be running

## Implementation approach

`codex-history` is **local-first**.

The intended implementation reads Codex history directly from local persisted session logs, builds optional local search/indexing on top of that, and keeps any App Server integration as possible later work behind a separate adapter.

## Backend modes

Current behavior:
- `local` — accepted by the CLI
- `auto` — accepted by the CLI and currently treated the same as `local`

Possible later work:
- `app-server` — optional adapter, not part of the initial build

## Indexing

Indexing is implemented and remains opt-in.

Current commands:

```bash
codex-history index build
codex-history index refresh
codex-history index doctor
```

`search` is the indexed path and is generally much faster after `index build`.
`grep` is a direct local transcript scan and does not require an index.

Current export commands:

```bash
codex-history export thr_123 --format json
codex-history export thr_123 --format markdown
codex-history export thr_123 --format prompt-pack
```

## Installation

### Homebrew tap

Planned for a later release phase via `nishantdesai/homebrew-tap`.

### From source

```bash
cargo build --release
```

## Usage

Current examples:

```bash
codex-history --help
codex-history show --help
codex-history export thr_123 --format markdown
codex-history index build
codex-history search "sqlite3_open_v2"
codex-history search --fresh "sqlite3_open_v2"
codex-history search --include-tools "cargo test"
codex-history grep "leftover argv"
codex-history grep --include-thinking "planner"
```

Human-readable `grep` and `search` results are grouped by thread and show:
- `thread_id`
- `name` when available
- `first prompt` when no thread name is available
- `cwd`
- hit and occurrence counts
- matched content source, such as `user` or `assistant`
- a compact preview snippet

Thread names come from parsed session metadata when present, and may also be filled from `~/.codex/session_index.jsonl` for display.

## Suggested repository docs

- `docs/SPEC.md` — architecture and scope
- `IMPLEMENTATION_PLAN.md` — execution plan for Codex
- `HOMEBREW_TAP_PLAN.md` — release and formula handoff
- `AGENTS.md` — repo-specific instructions for agents

## Privacy posture

- read-only only
- no telemetry
- no background daemon
- synthetic fixtures only in tests
- default human-readable output sanitizes obvious secrets conservatively where practical
- home-directory-style paths are sanitized in human-readable output where practical
- redacted JSON output remains structurally valid

## Status

Early-stage OSS project.

Current phase:
- Phase 6 export formats

Next planned phases:
- privacy and redaction
- Homebrew-tap-ready release packaging

App Server support is deferred for later.

## License

MIT
