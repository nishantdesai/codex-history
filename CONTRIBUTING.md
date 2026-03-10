# Contributing

Thanks for contributing to `codex-history`.

## Development

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

## Guardrails

- Keep the project read-only.
- Use synthetic fixtures only.
- Keep scope aligned with `docs/SPEC.md`.
