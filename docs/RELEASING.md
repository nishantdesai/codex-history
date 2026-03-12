# Releasing

This repository prepares GitHub Release archives for a separate Homebrew tap at `nishantdesai/homebrew-tap`.

The tap repository is not updated here. This repository is responsible for producing archives, checksums, and release notes material that the tap can consume.

## Version output

Release validation assumes the binary prints a stable version string:

```bash
codex-history --version
```

Expected shape:

```text
codex-history 0.1.0
```

## Release artifacts

Each release should publish at least:
- `codex-history-v<version>-aarch64-apple-darwin.tar.gz`
- `codex-history-v<version>-x86_64-apple-darwin.tar.gz`
- `SHA256SUMS`

Each archive contains:
- `codex-history`
- `README.md`
- `LICENSE`

## Local packaging

Build a native release archive:

```bash
./scripts/package-release.sh "$(rustc -vV | sed -n 's/^host: //p')"
```

Generate checksums for the archives in `dist/`:

```bash
./scripts/generate-checksums.sh
```

Equivalent Make targets:

```bash
make package-release
make release-checksums
```

## Manual validation

Validate the archive layout locally before publishing:

```bash
archive="$(./scripts/package-release.sh "$(rustc -vV | sed -n 's/^host: //p')")"
tmpdir="$(mktemp -d)"
tar -xzf "$archive" -C "$tmpdir"
"$tmpdir"/codex-history-v$(sed -nE 's/^version = "([^"]+)"/\1/p' Cargo.toml | head -n 1)-$(rustc -vV | sed -n 's/^host: //p')/codex-history --version
```

The extracted directory should contain a top-level `codex-history` binary with no nested `bin/` directory so Homebrew formula installation can use `bin.install "codex-history"` directly.

## GitHub Actions release workflow

The repository includes `.github/workflows/release.yml`:
- on tag pushes matching `v*`, it builds macOS archives, generates `SHA256SUMS`, and publishes those files to the GitHub Release
- on `workflow_dispatch`, it builds archives and checksums as workflow artifacts without publishing a release

## Homebrew tap handoff

After publishing a release from the main repo:

1. copy the GitHub Release archive URLs
2. copy the SHA256 values from `SHA256SUMS`
3. update the formula in `nishantdesai/homebrew-tap`
4. validate a fresh `brew install codex-history`

The tap update itself is intentionally out of scope for this repository.
