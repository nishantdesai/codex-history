# Changelog

All notable released changes to this project will be documented in this file.

## [Unreleased]

## [0.1.0] - 2026-03-12

Initial public release.

### Added

- Read-only local history commands for listing, showing, grepping, and inspecting Codex threads.
- Optional SQLite FTS index with `index build`, `index refresh`, and `search --fresh`.
- Export support for JSON, Markdown, and prompt-pack handoff formats.
- Privacy redaction for obvious secrets and home-directory-style paths in human-readable output, while preserving valid JSON output.
- Human-readable grouped search output plus compact markdown-table output for `grep` and `search`.
- Stable `codex-history --version` output for package and Homebrew validation.
- Release packaging scripts, checksum generation, and GitHub Actions workflow scaffolding for GitHub Releases.
