# Tauri Frontend Integration

The frontend runs in a Tauri desktop app and can also run in browser-only
debugging workflows.

## Boundary Rules

- Keep backend access behind `app/src/lib/api.ts` or the established Tauri/API
  boundary.
- Do not expose local tokens or machine-specific paths unnecessarily.
- Treat desktop startup failure, API unavailable, authentication failure, and
  malformed response as distinct UI states.
- Preserve `mise run dev`, `mise run dev:ui`, and the desktop workflow described
  in `README.md`.

## Desktop UX

- Account for app-window sizes, not just full browser screens.
- Keep critical actions reachable in narrower windows.
- Avoid layouts that require horizontal scrolling except for code or diff
  surfaces where it is expected.
- Provide copyable diagnostics when desktop/backend startup fails.

## Configuration

Before changing integration assumptions, inspect:

- `app/src/lib/api.ts`
- `app/src-tauri/tauri.conf.json`
- `app/src-tauri/src/main.rs`
- `mise.toml`

Do not change Tauri config as part of a UI-only task unless the behavior
requires it.
