---
name: wyrd-diff-ui
description: Repo-local wyrd-diff skill for modern SvelteKit, Svelte 5, TypeScript, Tauri frontend integration, app UI, styling, route design, data-heavy developer workflow screens, interaction states, accessibility, and frontend performance under app/src/ and app/static/. Use before editing the wyrd-diff UI.
---

# Wyrd Diff UI

Use this skill before touching the SvelteKit/Tauri frontend under `app/src/`
or related UI assets under `app/static/`.

`wyrd-diff` is a local diff review tool that pipes a developer's review back
to the coding agent while it is still in session. The UI surfaces diffs,
line-level comments, queued feedback batches, agent dispatch state, and the
resulting fix trajectory. It should feel like a modern working surface for
repeated review use, not a marketing site.

## First Pass

Before editing:

1. Read `README.md` for the product workflow.
2. Read `mise.toml` and `app/package.json` for canonical commands.
3. Inspect `app/src/lib/api.ts` before changing backend communication.
4. Inspect the route entrypoint and nearest local component before creating new
   structure.
5. Preserve the dirty worktree. Do not revert unrelated user changes.

Use existing package choices and local patterns before adding abstractions.

## UI Principles

- Build dense, modern, high-signal developer workflow screens.
- Make review sessions, diffs, comments, notes, decisions, and trajectory state
  easy to scan and inspect.
- Keep hierarchy clear: route context, object identity, status, and primary
  actions should be obvious.
- Avoid landing-page composition inside app workflows.
- Avoid decoration that does not help inspection, review, decisions, recovery,
  or action.
- Include loading, empty, error, disabled, hover, active, and focus states.
- Ensure text fits inside controls on mobile and desktop.
- Keep keyboard behavior and visible focus intact.

## Frontend Rules

- Work inside the existing SvelteKit app; do not treat this as a generic
  frontend project.
- Use Svelte 5 runes intentionally.
- Use strict TypeScript and typed route data, API responses, component props,
  and local state.
- Keep backend access behind the existing API/Tauri boundary.
- Keep visual leaf components focused on rendering and interaction, not data
  fetching.
- Do not add new UI libraries unless the user explicitly asks.
- Use real application data shapes and states instead of placeholder marketing
  copy.

## Editing Order

When making UI changes, inspect in this order:

1. Route entrypoint: `app/src/routes/**/+page.svelte`, `+page.ts`,
   `+page.server.ts`, or layout files.
2. Shared client/API helper: `app/src/lib/api.ts`.
3. Nearby feature components or utilities under `app/src/lib/`.
4. App-wide styles and layout files.
5. Tauri boundary files only if the UI behavior requires desktop integration.

## References

Load only the relevant files:

- Routes, data flow, API boundaries, and SvelteKit structure:
  `references/sveltekit-architecture.md`
- Svelte 5 runes, TypeScript, props, derived state, and shared state:
  `references/svelte5-typescript.md`
- Modern visual design principles for this app:
  `references/modern-ui-design.md`
- Review/diff/decision/trajectory workflow UX:
  `references/data-workflow-ux.md`
- Tauri/frontend bridge behavior and desktop constraints:
  `references/tauri-frontend.md`
- Rendering, list, diff, chart, and interaction performance:
  `references/frontend-performance.md`
- Frontend verification commands and manual checks:
  `references/testing-verification.md`

## Verification

Prefer targeted frontend checks while iterating:

```bash
pnpm --dir app check
pnpm --dir app lint
pnpm --dir app format:check
```

Use the full repo gate when the change crosses Rust, Tauri, or API contracts:

```bash
mise run check
```

For visual changes, run or inspect the app when practical:

```bash
mise run dev:ui
```

Do not claim verification passed unless you ran it.
