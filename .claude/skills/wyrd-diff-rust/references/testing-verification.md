# Testing And Verification

Use the narrowest check that proves the touched behavior while iterating, then a
broader gate when the change affects shared contracts.

## Targeted Rust Checks

```bash
cargo test --locked -p <crate> <test_name> -- --nocapture --test-threads=1
```

Use single-threaded tests when touching filesystem fixtures, SQLite files,
runtime state, ports, or environment variables.

## Common Gates

```bash
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
mise run check
```

## Repo Tasks

```bash
mise run db:migrate
mise run export:test
mise run dev:api
mise run dev:mcp
```

Use `mise run db:migrate` after migration changes. Use `mise run export:test`
after export or trajectory JSONL changes.

## Test Design

- Cover success, stable failures, and edge cases.
- Prefer local fixtures and temp directories.
- Do not require network credentials or live external services.
- Assert durable behavior at the core layer before testing every surface.
- Add API, CLI, MCP, or Tauri coverage when boundary behavior changes.
