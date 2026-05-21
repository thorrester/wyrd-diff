# AGENTS.md

This repository is `wyrd-diff`, a local review, decision, and code trajectory
memory tool for branch-based engineering work. It stores review comments,
private notes, durable decisions, agent handoff packets, accepted fix
trajectory, and exports in a local SQLite database while target repositories
stay clean.

## First Steps

Before editing:

1. Read `README.md` for the product workflow.
2. Check `mise.toml` for canonical commands.
3. Inspect the nearest existing implementation and tests.
4. Check relevant manifests before relying on versions or features:
   `Cargo.toml`, crate `Cargo.toml` files, `package.json`,
   `app/package.json`, and `app/src-tauri/Cargo.toml`.
5. Preserve the dirty worktree. Do not revert unrelated user changes.

Use local patterns before adding new abstractions or dependencies.

## Repo Skills

Codex repo-local skills live under `.codex/skills/`. Claude repo-local skills
live under `.claude/skills/`. The same two skills are mirrored in both
locations.

- Use `.codex/skills/wyrd-diff-rust/SKILL.md` or
  `.claude/skills/wyrd-diff-rust/SKILL.md` before editing Rust backend, Axum
  API, Tauri desktop shell, CLI, MCP, SQLite persistence, migrations, async
  runtime, error handling, or performance-sensitive Rust logic under `crates/`,
  `app/src-tauri/`, or `migrations/`.
- Use `.codex/skills/wyrd-diff-ui/SKILL.md` or
  `.claude/skills/wyrd-diff-ui/SKILL.md` before editing SvelteKit, Svelte 5,
  TypeScript, styling, routes, UI state, frontend performance, or Tauri
  frontend integration under `app/src/` or `app/static/`.

Load only the reference files relevant to the task.

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
- `app/src`: SvelteKit UI, routes, components, frontend API helpers, and
  interaction behavior.
- `migrations`: SQLite schema evolution.

Keep durable state and invariants in `wyrd-diff-core`. Keep API, CLI, MCP, and
Tauri layers thin unless they own the behavior being changed.

## Rust Rules

- Borrow before cloning.
- Avoid unnecessary allocation, serialization, and parsing in hot paths.
- Use typed domain values instead of stringly contracts where practical.
- Keep request handlers and Tauri commands thin.
- Do not block async request paths without an explicit blocking strategy.
- Do not create ad hoc Tokio runtimes in libraries or handlers.
- Use `thiserror` in libraries and `anyhow` in binaries.
- Avoid `unwrap()` in non-test code for filesystem, database, network, parsing,
  environment, user input, or process execution.

## UI Rules

- Build dense, modern, high-signal developer workflow screens.
- Avoid marketing-page composition inside app workflows.
- Keep review sessions, diffs, comments, notes, decisions, and trajectory state
  easy to scan and inspect.
- Use Svelte 5 runes intentionally.
- Keep TypeScript strict and typed at component, route, and API boundaries.
- Keep backend access behind the existing API/Tauri boundary.
- Include loading, empty, error, disabled, hover, active, and focus states.
- Ensure text fits inside controls on mobile and desktop.

## Commands

Install and run:

```bash
mise install
mise run db:migrate
mise run dev
```

Debug surfaces:

```bash
mise run dev:ui
mise run dev:api
mise run dev:mcp
```

Primary verification:

```bash
mise run check
```

Targeted Rust checks:

```bash
cargo test --locked -p <crate> <test_name> -- --nocapture --test-threads=1
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
```

Frontend checks:

```bash
pnpm --dir app check
pnpm --dir app lint
pnpm --dir app format:check
pnpm --dir app build
```

Use the narrowest meaningful check while iterating. Do not claim verification
passed unless you ran it.

## Constraints

- Do not commit local session IDs, development tokens, or machine-specific
  paths.
- Do not add dependencies unless the task requires it and the existing stack
  cannot reasonably solve the problem.
- Do not refactor unrelated code while implementing a focused change.
- Do not delete or rewrite generated or user-created files unless the task
  explicitly requires it.
