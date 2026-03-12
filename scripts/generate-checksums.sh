#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${1:-"$ROOT_DIR/dist"}"
OUTPUT_FILE="${2:-"$OUT_DIR/SHA256SUMS"}"

if [[ ! -d "$OUT_DIR" ]]; then
  echo "output directory does not exist: $OUT_DIR" >&2
  exit 1
fi

archives=()
while IFS= read -r archive; do
  archives+=("$archive")
done < <(find "$OUT_DIR" -maxdepth 1 -type f -name 'codex-history-v*.tar.gz' | sort)

if [[ "${#archives[@]}" -eq 0 ]]; then
  echo "no release archives found in $OUT_DIR" >&2
  exit 1
fi

: > "$OUTPUT_FILE"
for archive in "${archives[@]}"; do
  checksum="$(shasum -a 256 "$archive" | awk '{print $1}')"
  printf '%s  %s\n' "$checksum" "$(basename "$archive")" >> "$OUTPUT_FILE"
done

echo "$OUTPUT_FILE"
