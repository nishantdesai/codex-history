#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="${1:-}"
OUT_DIR="${2:-"$ROOT_DIR/dist"}"

if [[ -z "$TARGET" ]]; then
  echo "usage: scripts/package-release.sh <target-triple> [output-dir]" >&2
  exit 1
fi

VERSION="${VERSION:-$(sed -nE 's/^version = "([^"]+)"/\1/p' "$ROOT_DIR/Cargo.toml" | head -n 1)}"
if [[ -z "$VERSION" ]]; then
  echo "failed to determine package version from Cargo.toml" >&2
  exit 1
fi

ARCHIVE_STEM="codex-history-v${VERSION}-${TARGET}"
STAGING_DIR="$OUT_DIR/$ARCHIVE_STEM"
ARCHIVE_PATH="$OUT_DIR/$ARCHIVE_STEM.tar.gz"

mkdir -p "$OUT_DIR"
rm -rf "$STAGING_DIR"
rm -f "$ARCHIVE_PATH"

cargo build --locked --release --target "$TARGET" --manifest-path "$ROOT_DIR/Cargo.toml"

mkdir -p "$STAGING_DIR"
cp "$ROOT_DIR/target/$TARGET/release/codex-history" "$STAGING_DIR/"
cp "$ROOT_DIR/README.md" "$STAGING_DIR/"
cp "$ROOT_DIR/LICENSE" "$STAGING_DIR/"

tar -C "$OUT_DIR" -czf "$ARCHIVE_PATH" "$ARCHIVE_STEM"

echo "$ARCHIVE_PATH"
