# codex-history

A read-only CLI for locally accessible Codex session history, with search, export, and optional indexing.

## Current status

Phase 4 search index foundation is in place.

Current behavior:
- the Rust crate builds and passes CI checks
- `codex-history --help` works
- the CLI supports local-history `list`, `show`, `grep`, and `doctor`
- `index build` creates an opt-in local SQLite FTS index from local session history
- `index doctor` reports index presence, schema version, and core row counts
- `search <query>` reads ranked results from the local index
- command-specific help is available with `codex-history <command> --help`

Implemented command surface today:

```bash
codex-history --help
codex-history list
codex-history show <thread-id>
codex-history show --include-turns <thread-id>
codex-history search <query>
codex-history grep <pattern>
codex-history grep --regex <pattern>
codex-history doctor
codex-history index build
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
codex-history index doctor
```

Later phases will add refresh and freshness-overlay behavior:

```bash
codex-history search "sqlite3_open_v2" --fresh
codex-history index refresh
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
codex-history index build
codex-history search "sqlite3_open_v2"
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
- Phase 4 search index foundation

Next planned phases:
- refresh logic
- export formats
- Homebrew-tap-ready release packaging

App Server support is deferred for later.

## License

MIT
