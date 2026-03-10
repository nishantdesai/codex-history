# IMPLEMENTATION_PLAN.md

## Objective

Build `codex-history`, a Rust CLI for querying locally accessible Codex session history.

Follow `docs/SPEC.md` as the source of truth. This document is the execution plan and build order.

## Delivery priorities

1. OSS-ready repository from the first commit
2. Local backend first and primary
3. Index build/search/refresh
4. Export formats and packaging
5. App Server backend only later, behind an adapter

## Hard constraints

- Read-only only
- Synthetic fixtures only
- No private or undocumented OpenAI endpoints
- No background daemon
- No mandatory App Server dependency
- Do not widen scope beyond `docs/SPEC.md`

## Phase 0 — Repository bootstrap

### Create
- `Cargo.toml`
- `src/main.rs`
- `README.md`
- `docs/SPEC.md`
- `LICENSE`
- `CONTRIBUTING.md`
- `CODE_OF_CONDUCT.md`
- `SECURITY.md`
- `CHANGELOG.md`
- `.gitignore`
- `.github/workflows/ci.yml`
- `fixtures/`
- `rustfmt.toml`
- `clippy.toml` if needed

### Acceptance criteria
- `cargo check` passes
- CI runs formatting, clippy, tests
- `codex-history --help` works
- repository is publishable as open source immediately

## Phase 1 — CLI skeleton

### Subcommands to scaffold
- `list`
- `show`
- `search`
- `grep`
- `export`
- `doctor`
- `index build`
- `index refresh`
- `index doctor`
- `index drop`

### Backend flag
Add global `--backend local|auto`

Note: `auto` should behave the same as `local` in the first implementation.

### Output flags
Add support for:
- `--json`
- `--ndjson`
- `--quiet`
- `--verbose`
- `--no-color`

### Acceptance criteria
- CLI parses all planned commands and flags
- help text is accurate and concise

## Phase 2 — Canonical models

### Implement modules
- `src/model/thread.rs`
- `src/model/turn.rs`
- `src/model/item.rs`
- `src/model/export.rs`

### Requirements
- Define stable internal thread/turn/item structs
- Preserve unknown item kinds in a generic variant
- Keep models independent of parser implementation details

### Acceptance criteria
- unit tests for serde and model conversions

## Phase 3 — Local backend

### Implement
- `src/backend/local.rs`
- `src/parser/jsonl.rs`
- path discovery helpers in `src/util/paths.rs`

### Behavior
- discover local Codex history roots
- walk candidate files safely
- parse JSONL incrementally
- tolerate malformed lines
- normalize to canonical models

### Initial command support
- `list`
- `show`
- `grep`
- `doctor`

### Acceptance criteria
- local fixtures support list/show
- malformed fixture does not crash parsing
- grep works without any index

## Phase 4 — Search index foundation

### Implement
- `src/index/schema.rs`
- `src/index/ingest.rs`
- `src/index/query.rs`
- `src/index/manifest.rs`

### SQLite schema
Create:
- `threads`
- `turns`
- `items`
- `search_docs`
- `thread_manifest`
- `index_meta`
- FTS5 virtual table(s)

### Command support
- `index build`
- `index doctor`
- `search`

### Acceptance criteria
- `index build` creates DB and populates counts
- `search` returns ranked results from index
- `index doctor` checks schema version and DB presence

## Phase 5 — Refresh logic

### Implement
- incremental refresh based on thread manifest
- watermark and per-thread freshness tracking
- content fingerprint/hash support where practical

### Command support
- `index refresh`
- `search --fresh` overlay path

### Acceptance criteria
- changed fixture thread triggers targeted upsert
- unchanged fixture thread is skipped
- fresh overlay merges indexed and unindexed results without duplicates

## Phase 6 — Exporters

### Implement
- JSON export
- Markdown export
- Prompt-pack export

### Acceptance criteria
- `export <thread-id> --format json`
- `export <thread-id> --format markdown`
- `export <thread-id> --format prompt-pack`
- fixture snapshots validate output shape

## Phase 7 — Privacy and redaction

### Implement
- `src/redact/mod.rs`
- token-like masking
- JWT-like masking
- bearer/api-key masking
- home path sanitization in human-readable output where practical

### Acceptance criteria
- regression tests for secret masking
- JSON mode remains structurally valid after redaction where applied

## Phase 8 — Packaging and releases

### Implement
- `--version`
- release workflow producing archives
- checksum generation
- documentation for Homebrew tap consumption

### Acceptance criteria
- release artifacts are suitable for `nishantdesai/homebrew-tap`
- install docs validated manually

## Phase 9 — App Server adapter later

Not part of the first implementation.

Only after the local-first tool is working and released, App Server may be added behind a backend adapter without changing the core models or index design.

Potential later work:
- `src/backend/app_server.rs`
- child-process stdio JSON-RPC client
- initialize/initialized handshake
- thread enumeration and reading

## Suggested file tree

```text
src/
  main.rs
  cli/
    mod.rs
    commands/
      list.rs
      show.rs
      search.rs
      grep.rs
      export.rs
      doctor.rs
      index_build.rs
      index_refresh.rs
      index_doctor.rs
      index_drop.rs
  app/
    orchestrator.rs
    config.rs
  backend/
    mod.rs
    local.rs
    app_server.rs   # later
  parser/
    jsonl.rs
  model/
    thread.rs
    turn.rs
    item.rs
    export.rs
  index/
    schema.rs
    ingest.rs
    query.rs
    manifest.rs
  redact/
    mod.rs
  output/
    table.rs
    json.rs
    markdown.rs
  util/
    paths.rs
    hash.rs
    time.rs
fixtures/
  simple/
  malformed/
  changed_thread/
```

## Suggested crates
- `clap`
- `serde`
- `serde_json`
- `rusqlite`
- `walkdir`
- `chrono`
- `regex`
- `anyhow`
- `thiserror`
- `directories` or `etcetera`

## Working style for Codex

- Make small, reviewable commits
- Keep modules cohesive and narrow
- Prefer explicit errors over magic fallback behavior
- Add tests for each new capability
- Do not over-engineer daemon/watch behavior
- Keep README install and usage docs current as features land
- Do not start App Server work during the initial build unless explicitly asked later

## Definition of done for first usable release

The project is ready for first public release when all of the following are true:
- local backend works for `list`, `show`, `grep`
- index build/search/refresh work
- `search --fresh` works
- export formats work
- CI is green
- release archives can be used by Homebrew tap
