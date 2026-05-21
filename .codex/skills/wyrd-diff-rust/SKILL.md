---
name: wyrd-diff-rust
description: Repo-local wyrd-diff skill for high-performance Rust backend, Axum API, Tauri desktop shell, CLI, MCP, SQLite persistence, async runtime, error handling, observability, migrations, and other non-UI Rust work under crates/, app/src-tauri/, and migrations/. Use before editing Rust logic or performance-sensitive backend/desktop behavior in this repository.
---

# Wyrd Diff Rust

Use this skill before touching Rust backend, desktop, persistence, CLI, MCP,
API, migration, runtime, or performance-sensitive logic in `wyrd-diff`.

This skill is repo-specific. Keep terminology, paths, crate names, contracts,
and workflow assumptions aligned with this repository.

## First Pass

Before editing:

1. Read `README.md` for the product workflow.
2. Read `mise.toml` for canonical commands.
3. Check `Cargo.toml` and the owning crate manifest before relying on
   dependency or feature behavior.
4. Inspect the nearest implementation and tests in the owning crate.
5. Preserve the dirty worktree. Do not revert unrelated user changes.

Use local patterns before introducing a new abstraction.

## Ownership Boundaries

- `crates/wyrd-diff-core`: domain models, SQLite persistence, git/diff logic,
  export behavior, review sessions, comments, notes, decisions, and durable
  trajectory records.
- `crates/wyrd-diff-api`: Axum/local HTTP contracts, server state, route
  handlers, local bridge behavior, and API error normalization.
- `crates/wyrd-diff-cli`: human-facing commands, development workflows, hook
  entrypoints, and process orchestration.
- `crates/wyrd-diff-mcp`: agent-facing MCP tools and structured context
  exposure.
- `app/src-tauri`: Tauri desktop shell, command boundary, app lifecycle, and
  local bridge startup integration.
- `migrations`: SQLite schema evolution and persistence compatibility.

If behavior crosses surfaces, keep durable state and invariants in
`wyrd-diff-core`, expose typed API behavior through `wyrd-diff-api`, then keep
CLI, MCP, and Tauri layers thin.

## Core Rules

- Borrow before cloning. Treat every `.clone()` as an ownership decision.
- Avoid repeated allocation, serialization, and parsing in hot paths.
- Use typed domain values instead of raw strings where invalid states matter.
- Keep request handlers and Tauri commands thin; delegate durable behavior to
  core APIs.
- Use async for IO boundaries, but keep pure computation synchronous.
- Do not block async request paths without an explicit blocking strategy.
- Do not create ad hoc Tokio runtimes in libraries or request handlers.
- Use `thiserror` in libraries and `anyhow` in binaries.
- Avoid `unwrap()` in non-test code for filesystem, database, network, parsing,
  environment, user input, or process execution.
- Use structured `tracing` where diagnostics matter.

## References

Load only the relevant files:

- Rust ownership, allocation, traits, and API shape:
  `references/rust-performance.md`
- Axum routes, server state, request/response contracts, and local API behavior:
  `references/axum-api.md`
- Tauri shell, command boundaries, bridge lifecycle, and frontend integration:
  `references/tauri-boundary.md`
- SQLite migrations, rusqlite access, transactions, and durability:
  `references/sqlite-persistence.md`
- Tokio, blocking work, concurrency, cancellation, and long-running processes:
  `references/async-runtime.md`
- Errors, observability, logs, and user-facing diagnostics:
  `references/errors-observability.md`
- Test selection and repo verification commands:
  `references/testing-verification.md`

## Verification

Prefer narrow checks while iterating:

```bash
cargo test --locked -p <crate> <test_name> -- --nocapture --test-threads=1
```

Use broader gates before completion when relevant:

```bash
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
mise run check
```

For migration-specific work, also run:

```bash
mise run db:migrate
```

Do not claim verification passed unless you ran it.
