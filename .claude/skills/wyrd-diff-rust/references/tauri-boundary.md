# Tauri Boundary

`app/src-tauri` owns the desktop host and integration between the SvelteKit UI
and local Rust services.

## Responsibilities

- Start and manage the local bridge expected by the desktop workflow.
- Keep desktop lifecycle behavior explicit.
- Expose only necessary commands to the frontend.
- Keep command handlers thin and route durable behavior through shared Rust
  crates where possible.
- Keep machine-specific paths and development tokens out of committed config.

## Command Design

- Use typed command inputs and outputs.
- Validate user-provided paths and identifiers before use.
- Return user-actionable errors instead of panics.
- Avoid long blocking operations directly inside command handlers without a
  blocking strategy or progress model.
- Do not duplicate API or core business logic in Tauri commands.

## Frontend Integration

- Keep the API URL, token, and desktop bridge assumptions consistent with
  `mise.toml`, `README.md`, and `app/src/lib/api.ts`.
- Preserve browser-only debugging workflows when changing desktop startup.
- Make startup failures distinguishable from backend API failures.

## Packaging

Before changing icons, app identifiers, permissions, or Tauri config, inspect:

- `app/src-tauri/tauri.conf.json`
- `app/src-tauri/Cargo.toml`
- `app/package.json`

Do not change packaging metadata as part of backend logic work unless the user
asked for it.
