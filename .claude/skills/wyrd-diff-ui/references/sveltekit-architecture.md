# SvelteKit Architecture

The UI lives in `app/src/` and runs inside both browser-debug and Tauri desktop
workflows.

## Route Design

- Keep route parameters explicit and typed.
- Put route-level context near the top of the screen.
- Use nested layouts only when they simplify repeated structure.
- Keep object identity visible on detail pages.
- Preserve browser back behavior for drilldowns.

## Data Flow

- Inspect `app/src/lib/api.ts` before changing backend communication.
- Keep API response handling typed at the boundary.
- Normalize backend failures into UI states that distinguish missing data,
  invalid input, unavailable backend, and unexpected errors.
- Avoid browser code reaching around the established API/Tauri bridge.

## Component Boundaries

- Keep route files responsible for page composition and data orchestration.
- Keep visual components focused on rendering and local interaction.
- Move shared feature types to `app/src/lib/` only when multiple routes need
  them.
- Do not create broad component frameworks before repeated patterns exist.

## Mutations And Refresh

- Make successful mutations visibly update the affected data.
- Keep invalidation or refresh behavior near the owning route/helper.
- Avoid global refreshes when a local state update or scoped reload is enough.
- Preserve user intent such as selected review session, active filter, or open
  detail panel after mutation.
