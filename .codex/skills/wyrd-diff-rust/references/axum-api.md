# Axum API

`crates/wyrd-diff-api` owns the local HTTP bridge used by the desktop app and
browser debugging flows.

## Handler Shape

- Keep handlers thin.
- Extract typed path, query, state, and JSON inputs.
- Delegate durable behavior to `wyrd-diff-core`.
- Return typed response structs.
- Normalize errors at the API boundary.
- Do not open database connections, scan repositories, or spawn long-running
  processes directly inside route glue when a service/core helper should own it.

## State

- Put shared dependencies in explicit state structs.
- Share heavy clients, database handles, configuration, or bridge state through
  `Arc` only when shared ownership is real.
- Avoid `Arc<Mutex<T>>` in request hot paths unless mutation truly requires it.
- Keep auth/token checks near the route or middleware boundary.

## Contracts

- Keep request and response bodies typed and serde-backed.
- Avoid accepting arbitrary maps where named fields are known.
- Preserve stable field names once the frontend, CLI, or MCP surface depends on
  them.
- Return actionable errors for missing repos, invalid refs, missing sessions,
  database failures, and malformed input.

## Performance Notes

- Do not load full diffs, file contents, or trajectory exports unless the route
  needs them.
- Prefer paginated or scoped queries for list endpoints.
- Keep route-level serialization bounded and predictable.
- Avoid blocking git, filesystem, and SQLite work directly on async executor
  threads if it can become expensive.
