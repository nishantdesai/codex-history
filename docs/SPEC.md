# codex-history — OSS-first build spec

## 1. Overview

`codex-history` is a read-only CLI for querying **locally accessible Codex session history** and exporting it in automation-friendly formats.

The tool must:
- list threads/sessions
- show thread details and turn history
- support fast full-text search over indexed history
- support direct transcript scanning without an index
- remain useful when the latest threads are newer than the local index
- be open-source-ready from the first commit
- be distributable via Homebrew tap at `github.com/nishantdesai/homebrew-tap`

The primary design assumption is that the tool reads Codex history **directly from local persisted session logs**. App Server remains a possible later adapter, but it is not part of the primary implementation path for the first build.

---

## 2. Product statement

Build a Rust CLI that:
- reads Codex history via **direct local session-log parsing**
- supports an **opt-in local search index** for fast repeated full-text search
- can optionally do a **freshness overlay** so searches include newer/unindexed threads
- keeps App Server integration as a later optional adapter, not a first-release dependency

This tool is **not** a replacement for Codex App Server. It is a history/search/export layer on top of Codex local history.

---

## 3. Goals

### Primary goals
- Read and normalize locally accessible Codex history from disk
- Search transcript content quickly using an opt-in index
- Include latest changed threads even when not yet indexed
- Export results in machine-friendly JSON and human-friendly Markdown
- Be safe to open source from commit 1
- Ship with Homebrew distribution support via `nishantdesai/homebrew-tap`

### Non-goals for v1
- Editing or mutating Codex history
- Deleting/archiving/unarchiving threads
- Depending on App Server
- Relying on undocumented private OpenAI endpoints
- Syncing user history anywhere
- Running as a background daemon

---

## 4. Scope and truthfulness requirements

All public docs, help text, and README must use wording equivalent to:

> A read-only CLI for locally accessible Codex session history, with search, export, and optional indexing.

Do **not** claim:
- “all Codex sessions everywhere”
- “global user history”
- cloud-only or server-side history guarantees

---

## 5. Codex integration model

### 5.1 Primary history source

The CLI uses **local persisted Codex history** as the primary source of truth.

### 5.2 Future adapter

App Server may be added later as an optional backend adapter, but it is **not required** for v1 and should not shape the first implementation.

### 5.3 Why local-first

- The tool’s main value is history retrieval and search.
- Direct local log parsing gives maximum access to raw history.
- The tool stays offline, read-only, and simple to reason about.
- There is no dependency on spawning or talking to a server process.
- Search/indexing naturally sit on top of local persisted history.

### 5.4 Backend modes for v1

Support these modes in design and code structure:
- `local` — active and implemented in v1
- `auto` — equivalent to local in v1
- `app-server` — reserved for later, may return a clear not-yet-implemented message if exposed early, or be omitted until implemented

Recommended first-release behavior:
- implement `local`
- optionally parse `auto` as an alias for `local`
- defer `app-server` implementation entirely until later

---

## 6. Search and indexing model

### 6.1 Core principle

The tool must support search in two layers:

1. **Indexed search** over a local opt-in index for speed
2. **Freshness overlay** over newer/changed threads not yet indexed

### 6.2 Why index exists

Codex history is stored as local session logs, but those raw logs are not themselves an ergonomic full-text search interface. Therefore the CLI must build its own search capability.

### 6.3 Index command

The CLI must provide an explicit index lifecycle:

```bash
codex-history index build
codex-history index refresh
codex-history index doctor
codex-history index drop
```

Indexing is **opt-in**. Search must still function without an index, though it may be slower.

### 6.4 Index backend

Use a local SQLite database with FTS5.

The SQLite DB is a **search index owned by this tool**, not the primary history store.

Suggested default path:
- `~/.local/share/codex-history/index.sqlite` on Linux
- `~/Library/Application Support/codex-history/index.sqlite` on macOS
- platform-appropriate equivalent on Windows

Also support `CODEX_HISTORY_HOME` override.

### 6.5 Freshness overlay

When search runs with an existing index, the tool must still be able to include newer/changed threads.

Recommended behavior:
- maintain `last_indexed_updated_at`
- maintain a per-thread manifest with last-seen `thread_id`, `updated_at`, and a content fingerprint/hash
- at search time, optionally perform a lightweight freshness check
- fetch newer or changed threads from local logs
- search them in-memory
- merge overlay matches with indexed matches

Suggested defaults:
- `search` uses index only by default for speed
- `search --fresh` performs freshness overlay
- optional future config to make fresh overlay default

### 6.6 Freshness detection

The tool must not assume that `last_indexed_updated_at` alone proves nothing has changed. It must do a cheap enumeration step against the source of truth:

- in local mode: scan thread/session files and compare file metadata and/or fingerprints against the manifest

App Server-based freshness detection may be added later if that adapter is implemented.

---

## 7. CLI surface

## 7.1 Commands

### `list`
List sessions/threads.

Examples:
```bash
codex-history list
codex-history list --json
```

### `show <thread-id>`
Show thread metadata and optionally full turns.

Examples:
```bash
codex-history show thr_123
codex-history show thr_123 --include-turns
codex-history show thr_123 --json
```

### `search <query>`
Search across history.

Examples:
```bash
codex-history search "sqlite3_open_v2"
codex-history search "SwiftData migration" --fresh
codex-history search --include-tools "cargo test"
codex-history search --compact "deadlock"
codex-history search "No such module" --json
```

### `grep <pattern>`
Literal or regex transcript search without ranking.

Examples:
```bash
codex-history grep "fatal error:"
codex-history grep --regex "error\s+Domain=.*Code="
codex-history grep --include-thinking "planner"
codex-history grep --compact "deadlock"
```

### `export <thread-id>`
Export a thread.

Examples:
```bash
codex-history export thr_123 --format markdown
codex-history export thr_123 --format json
codex-history export thr_123 --format prompt-pack
```

### `index build`
Build the local index from scratch.

### `index refresh`
Refresh changed/new threads only.

### `index doctor`
Check index integrity and staleness.

### `index drop`
Reserved for later. It is scaffolded in the CLI design but not implemented in the current release-preparation state.

### `doctor`
Check Codex history roots and index paths.

## 7.2 Backend flags

Support:
- `--backend local`
- `--backend auto`

Optional later:
- `--backend app-server`

Default: `local` or `auto` mapped to local.

## 7.3 Output flags

Support:
- `--json`
- `--ndjson`
- `--no-color`
- `--quiet`
- `--verbose`
- `--include-turns`
- `--fresh`
- `--include-thinking`
- `--include-tools`
- `--compact`
- `--version`

### CLI behavior contract
- success payloads go to stdout
- diagnostics/errors go to stderr
- `--json` output must never be mixed with prose
- non-zero exit codes on hard failures
- optional `--exit-code-on-no-results`

---

## 8. Canonical data model

Normalize all source data into stable internal structs.

### 8.1 ThreadSummary
- `thread_id: String`
- `name: Option<String>`
- `preview: Option<String>`
- `created_at: DateTime`
- `updated_at: Option<DateTime>`
- `cwd: Option<PathBuf>`
- `source_kind: Option<String>`
- `model_provider: Option<String>`
- `ephemeral: Option<bool>`
- `status: Option<String>`

### 8.2 ThreadDetail
- all ThreadSummary fields
- `turns: Vec<Turn>`
- `items_count: usize`
- `commands_count: usize`
- `files_changed_count: usize`

### 8.3 Turn
- `turn_id: String`
- `status: String`
- `started_at: Option<DateTime>`
- `completed_at: Option<DateTime>`
- `items: Vec<Item>`

### 8.4 Item
Tagged union with variants such as:
- `user_message`
- `agent_message`
- `command_execution`
- `file_change`
- `reasoning_summary`
- `web_search`
- `mcp_tool_call`
- `other`

### 8.5 SearchDocument
Flattened indexed representation:
- `thread_id`
- `turn_id`
- `kind`
- `text`
- `cwd`
- `updated_at`
- `path_refs`
- `command_refs`
- `rank_metadata`

---

## 9. Local-source integration details

### 9.1 Behavior

Local mode reads persisted Codex history directly from disk.

Implementation must:
- discover likely history root(s)
- walk session files safely
- parse JSONL incrementally
- tolerate malformed lines
- avoid crashing on partial corruption

### 9.2 Parser design

- use streaming parsing, not full-file load when unnecessary
- preserve unknown event types
- isolate parser from search/index logic
- make no destructive writes to Codex files

### 9.3 Corruption handling

If a file contains malformed JSONL lines:
- skip bad lines
- mark thread as degraded
- continue indexing usable content

---

## 10. Search ranking

For `search`, implement a simple ranking model in v1:

Priority order:
1. exact phrase match
2. command/path match
3. thread title/preview match
4. recent `updated_at`
5. cwd match

For `grep`, no ranking is required.

---

## 11. Export formats

Support at least:
- `json`
- `markdown`
- `prompt-pack`

### 11.1 JSON
Schema-stable and machine-friendly.

### 11.2 Markdown
Human-readable with sections:
- metadata
- turns
- commands
- file changes
- extracted notes

### 11.3 Prompt-pack
Compact automation handoff format:
- objective
- key context
- commands seen
- files touched
- notable errors
- useful follow-ups

---

## 12. Privacy and security requirements

This section is mandatory for first commit.

### 12.1 Default posture
- read-only access only
- no telemetry
- no network calls
- no background daemon
- no upload/sync behavior

### 12.2 Redaction
Default human-readable output must redact or sanitize:
- home directory prefixes when practical
- API keys, bearer tokens, cookies, JWT-like strings
- secret-like environment variable values

Raw/no-redact mode may be added later, but is not required in first commit.

### 12.3 Test fixtures
- use synthetic thread/session fixtures only
- no real transcripts
- no local machine paths in tests/docs
- no checked-in generated indexes from real data

---

## 13. OSS repo requirements from first commit

Repository must include:
- `README.md`
- `LICENSE`
- `CONTRIBUTING.md`
- `CODE_OF_CONDUCT.md`
- `SECURITY.md`
- `CHANGELOG.md`
- `Justfile` or `Makefile`
- CI workflow
- synthetic fixtures
- formatter/linter config
- secret-scan step in CI if practical

### 13.1 License
Recommendation: MIT.

### 13.2 README requirements
README must clearly state:
- scope
- local-first implementation
- index is opt-in
- app-server support is deferred/optional later
- privacy posture
- Homebrew install instructions via `nishantdesai/tap/codex-history`

---

## 14. Distribution requirements

### 14.1 Build language
Use Rust.

Reasons:
- matches Codex ecosystem expectations
- produces strong single-binary CLI distribution story
- good filesystem/JSON/SQLite support

### 14.2 Binary name
Binary: `codex-history`

### 14.3 Homebrew distribution
The project must be designed for release binaries consumable by a Homebrew tap hosted at:
- `github.com/nishantdesai/homebrew-tap`

Recommended packaging model:
- GitHub Releases publish macOS binaries/tarballs
- tap repo contains a formula named `codex-history.rb`
- direct install command:

```bash
brew install nishantdesai/tap/codex-history
```

The tool repo should document that the tap lives separately at `nishantdesai/homebrew-tap`.

### 14.4 Release artifact expectations
Each release should publish at least:
- macOS arm64 archive
- macOS x86_64 archive if supported
- SHA256 checksums

Optional later:
- Linux tarballs

---

## 15. Suggested Rust architecture

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
  model/
    thread.rs
    turn.rs
    item.rs
    export.rs
  parser/
    jsonl.rs
  index/
    mod.rs
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
    time.rs
    hash.rs
```

### 15.1 Suggested crates
- `clap`
- `serde`
- `serde_json`
- `rusqlite`
- `walkdir`
- `chrono`
- `regex`
- `anyhow`
- `thiserror`
- `ignore`
- `directories` or `etcetera`

---

## 16. Index schema suggestion

### 16.1 Core tables
- `threads`
- `turns`
- `items`
- `search_docs`
- `thread_manifest`
- `index_meta`

### 16.2 FTS
Use FTS5 virtual table over:
- thread name
- thread preview
- user messages
- agent messages
- command strings
- command output excerpts
- file paths
- error text

### 16.3 Manifest table
Store per-thread freshness metadata:
- `thread_id`
- `last_seen_updated_at`
- `content_fingerprint`
- `last_indexed_at`
- `source_backend`

### 16.4 Index metadata
Store:
- schema version
- build time
- last refresh time
- source mode used for last refresh

---

## 17. Command semantics

### `index build`
- rebuild index from scratch
- read from local history
- emit summary counts

### `index refresh`
- cheap enumeration of current threads/files
- detect new/changed threads using timestamp + thread ID + fingerprint where possible
- upsert only changed threads
- delete index entries for threads no longer present only if behavior is explicitly designed and documented; otherwise leave tombstoning for later

### `search`
- if index exists: search index
- if `--fresh`: perform freshness overlay and merge results
- if index missing: fallback to direct scan or instruct user that indexed search is faster

### `show`
- direct source read is acceptable
- no need to route through index

### `list`
- direct source read is acceptable
- no need to route through index

---

## 18. Merge behavior for fresh overlay

When `search --fresh` is used:
1. query the index
2. enumerate new/changed threads since manifest watermark
3. fetch changed thread content from local logs
4. search changed thread documents in-memory using same tokenization/search semantics where practical
5. merge and de-duplicate by `thread_id + turn_id + kind + snippet`
6. sort by ranking rules

---

## 19. Testing requirements

### 19.1 Unit tests
- parser correctness
- redaction correctness
- index schema migrations
- query ranking basics
- export rendering

### 19.2 Fixture tests
Synthetic fixtures only:
- simple thread
- multi-turn thread
- command-heavy thread
- malformed JSONL thread
- sensitive-string fixture for redaction
- changed-thread refresh fixture

### 19.3 Integration tests
- local backend end-to-end with temporary fixture root
- index build + search + refresh
- future app-server adapter tests may be added later with mocked JSON-RPC transcripts

---

## 20. Milestone plan

## Milestone 1 — OSS repo bootstrap
Acceptance criteria:
- repo skeleton exists
- docs/licensing/governance files exist
- CI passes
- `doctor` works

## Milestone 2 — Local backend
Acceptance criteria:
- `list` works from synthetic local fixtures
- `show` works from synthetic local fixtures
- parser tolerates malformed lines

## Milestone 3 — Indexing
Acceptance criteria:
- `index build` creates SQLite index
- `search` uses index
- `grep` works without index
- `index doctor` validates schema/version

## Milestone 4 — Refresh and freshness overlay
Acceptance criteria:
- `index refresh` updates only changed/new threads
- `search --fresh` merges overlay results
- changed-thread detection uses manifest correctly

## Milestone 5 — Packaging
Acceptance criteria:
- release binary artifacts produced in CI
- Homebrew formula template ready for `nishantdesai/homebrew-tap`
- installation docs verified

## Milestone 6 — App Server adapter later
Acceptance criteria for later work only:
- optional adapter can be added behind backend abstraction without changing core models or index design

---

## 21. Homebrew tap handoff requirements

The tool repo must produce release artifacts in a form that a separate tap can consume.

Expected follow-up files/work:
- `Formula/codex-history.rb` in `nishantdesai/homebrew-tap`
- release archive URLs
- SHA256 values
- release notes

Suggested formula expectations:
- downloads GitHub Release archive
- installs binary `codex-history`
- includes basic `test do` block such as `codex-history --version`

---

## 22. Explicit implementation decisions

1. **Rust CLI**
2. **Read-only tool**
3. **Local persisted history as primary source**
4. **App Server adapter deferred until later**
5. **Opt-in SQLite FTS index via `index` subcommands**
6. **Freshness overlay for latest unindexed threads**
7. **Homebrew-tap-ready release packaging**
8. **OSS hygiene from first commit**

---

## 23. Nice-to-have later

- App Server adapter
- persistent daemon/watch mode
- live tail of active threads
- semantic search
- richer prompt-pack extraction
- `recent`, `stats`, `recall`, `lessons`
- shell completions
- man page generation

---

## 24. Short build brief for Codex

Build a Rust CLI named `codex-history` with `list`, `show`, `search`, `grep`, `export`, `doctor`, and `index` subcommands. Use direct local Codex history JSONL parsing as the primary source of truth. Add an opt-in SQLite FTS5 index with `index build`, `index refresh`, `index doctor`, and `index drop`. Search should use the index when present and support a `--fresh` overlay that checks for newer or changed local threads and merges those matches into the results. Package the project for release binaries that can be installed through the Homebrew tap at `github.com/nishantdesai/homebrew-tap`. Keep the repository open-source-ready from first commit with MIT licensing, governance files, CI, synthetic fixtures only, and a clear privacy/read-only posture. App Server support, if added later, must be isolated behind a backend adapter and must not change the local-first core architecture.
