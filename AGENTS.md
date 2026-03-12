# AGENTS.md

## Purpose

This repository builds `codex-history`, a Rust CLI for querying locally accessible Codex session history.

## Source of truth

When making architectural or scope decisions, follow:
1. `docs/SPEC.md`
2. `README.md`
3. `docs/RELEASING.md`

If these documents conflict, prefer the more conservative interpretation and do not widen scope.

## Core implementation decision

Use **direct local Codex history/session-log parsing** as the primary source of truth.

Do **not** implement the App Server adapter in the initial build.
Keep the code structure compatible with a later adapter, but defer that work until after the local-first tool is working.

## Working rules

- Keep the tool read-only
- Do not add any feature that mutates Codex history
- Do not depend on undocumented private endpoints
- Prefer local backend first and primary
- Keep App Server integration deferred until later
- Keep the index opt-in
- Use synthetic fixtures only
- Do not check in real history data, indexes, or machine-specific paths

## Code quality rules

- prefer small, reviewable commits
- add tests for each feature
- keep modules narrowly scoped
- use explicit errors and typed results
- avoid introducing async complexity unless clearly necessary
- preserve unknown item/event variants instead of discarding them

## Output and UX rules

- stdout for successful command output
- stderr for diagnostics and errors
- never mix prose into `--json` output
- make `--help` text concise and accurate
- keep `--json` and `--ndjson` as the machine-facing interfaces for search/grep
- keep human-readable `search`/`grep` output aligned with the implemented modes:
  default output is grouped by thread with per-hit full text blocks, and `--compact` is a markdown-table-style one-line view that should not deliberately truncate thread name, cwd, or preview text

## Packaging rules

- binary name must remain `codex-history`
- repository license is MIT unless the user explicitly changes it
- release artifacts must be compatible with the Homebrew tap at `github.com/nishantdesai/homebrew-tap`

## Documentation rules

- keep README, SPEC, and help text aligned with implemented behavior, not future phases

## When in doubt

Do the smallest implementation that satisfies the spec and keeps the repository publishable as open source from the first commit.
