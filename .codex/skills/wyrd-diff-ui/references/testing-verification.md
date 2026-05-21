# Testing And Verification

Run checks from the repository root unless a command specifies `--dir app`.

## Frontend Checks

```bash
pnpm --dir app check
pnpm --dir app lint
pnpm --dir app format:check
pnpm --dir app build
```

Use these when changing Svelte components, route data, TypeScript contracts,
shared styles, or frontend helpers.

## Full Gate

```bash
mise run check
```

Run the full gate when frontend changes also touch Rust, Tauri, API contracts,
or repo-wide behavior.

## Manual UI Checks

For visual or interaction changes, inspect the UI when practical:

```bash
mise run dev:ui
```

Check:

- Text fits inside buttons, tabs, filters, cards, columns, and sidebars.
- Controls are keyboard reachable.
- Focus is visible.
- Loading, empty, error, disabled, selected, and hover states are reachable.
- Narrow app-window layouts remain usable.
- Data updates do not cause unnecessary layout shift.

Do not claim a check passed unless it was run.
