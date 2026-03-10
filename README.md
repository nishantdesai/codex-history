# codex-history

A read-only CLI for locally accessible Codex session history, with search, export, and optional indexing.

## Current status

Phase 0 repository bootstrap is in place.

Current behavior:
- the Rust crate builds and passes CI checks
- `codex-history --help` works
- the CLI parses the planned top-level commands and global flags
- subcommands are still scaffolds and currently print `not implemented`

Implemented command surface today:

```bash
codex-history --help
codex-history list
codex-history show <thread-id>
codex-history search <query>
codex-history grep <pattern>
codex-history export <thread-id> --format markdown
codex-history doctor
codex-history index build
```

The parser is intentionally strict:
- malformed command lines return errors
- invalid flag combinations return non-zero exit codes
- top-level help works after normal global flag orderings

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

Indexing is planned and will remain opt-in.

Planned commands:

```bash
codex-history index build
codex-history index refresh
codex-history index doctor
codex-history index drop
```

Later phases will add a local SQLite FTS5 index and freshness overlay support:

```bash
codex-history search "sqlite3_open_v2" --fresh
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
codex-history list
codex-history index --help
```

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
- default human-readable output should sanitize obvious secrets where practical

## Status

Early-stage OSS project.

Current phase:
- Phase 0 repository bootstrap

Next planned phases:
- CLI skeleton behavior behind the parsed command surface
- local backend
- index build/search/refresh
- export formats
- Homebrew-tap-ready release packaging

App Server support is deferred for later.

## License

MIT
