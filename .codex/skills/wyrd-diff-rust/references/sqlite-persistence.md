# SQLite Persistence

`crates/wyrd-diff-core` and `migrations/` own durable local state.

## Schema Changes

- Put schema changes in ordered migration files under `migrations/`.
- Preserve existing user data unless the task explicitly requires a reset path.
- Keep migration SQL deterministic and idempotent where the migration runner
  expects it.
- Update Rust persistence code and tests with the migration.

## rusqlite Access

- Use parameters instead of string interpolation.
- Keep row mapping explicit and typed.
- Use transactions for multi-step writes that must succeed or fail together.
- Avoid holding transactions longer than necessary.
- Keep database errors wrapped with operation context.

## Data Modeling

- Store durable identifiers and timestamps consistently.
- Keep exported JSONL and hook-recorded trajectory data stable.
- Avoid stuffing structured data into opaque JSON unless the schema is
  intentionally flexible.
- Make delete/update behavior explicit; local review memory should not vanish as
  a side effect of convenience paths.

## Testing

- Prefer tempfile-backed SQLite databases for persistence tests.
- Test migrations, inserts, reads, updates, and stable failure cases.
- Use single-threaded tests when shared paths, ports, global state, or database
  files can collide.
