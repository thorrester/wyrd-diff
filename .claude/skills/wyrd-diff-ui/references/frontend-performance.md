# Frontend Performance

Developer workflow screens can become data-heavy. Keep rendering predictable.

## Lists And Tables

- Bound list sizes when displaying sessions, files, comments, or trajectory
  records.
- Use pagination, progressive disclosure, or virtualization when data can grow.
- Keep row keys stable.
- Avoid expensive formatting work inside repeated markup.
- Precompute derived collections with `$derived` when inputs are clear.

## Diffs And Code

- Avoid rendering large hidden diff trees.
- Keep syntax highlighting scoped to visible content where practical.
- Preserve scroll position when refreshing surrounding metadata.
- Avoid re-highlighting unchanged content on every local state update.

## Effects

- Use `$effect` for external subscriptions and browser APIs only.
- Clean up observers, timers, and event listeners.
- Debounce search and resize work when it touches layout or API calls.
- Avoid global stores that cause unrelated routes/components to re-render.

## Payloads

- Request only the data the view needs.
- Avoid loading full exports, raw diffs, or large notes collections for summary
  screens.
- Keep error payloads and diagnostics useful but bounded.
