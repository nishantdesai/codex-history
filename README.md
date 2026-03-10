# codex-history

A read-only CLI for locally accessible Codex session history, with search, export, and optional indexing.

## What it does

`codex-history` helps you:
- list Codex threads/sessions
- inspect thread details and turns
- search history quickly with an opt-in local index
- export threads in machine-friendly and human-friendly formats
- include newer or changed threads with a freshness overlay

## What it does not do

- mutate Codex history
- delete or archive sessions
- sync your history anywhere
- require Codex App Server to always be running

## Current implementation approach

`codex-history` is **local-first**.

The first implementation reads Codex history directly from local persisted session logs, builds optional local search/indexing on top of that, and keeps any App Server integration as possible later work behind a separate adapter.

## Backend modes

Current behavior:
- `local` — parse local Codex history directly
- `auto` — currently behaves the same as `local`

Possible later work:
- `app-server` — optional adapter, not part of the initial build

## Indexing

Indexing is opt-in.

Use:

```bash
codex-history index build
codex-history index refresh
codex-history index doctor
codex-history index drop
```

The index uses local SQLite FTS5 for fast repeated search. When you want the newest results, use freshness overlay support:

```bash
codex-history search "sqlite3_open_v2" --fresh
```

## Installation

### Homebrew tap

```bash
brew tap nishantdesai/tap
brew install codex-history
```

### From source

```bash
cargo build --release
```

## Usage

```bash
codex-history list
codex-history show <thread-id>
codex-history grep "fatal error:"
codex-history search "SwiftData migration"
codex-history export <thread-id> --format markdown
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

The initial release target is:
- local backend
- index build/search/refresh
- export formats
- Homebrew-tap-ready release packaging

App Server support is deferred for later.

## License

MIT
