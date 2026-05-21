# Svelte 5 And TypeScript

Use Svelte 5 runes deliberately and keep TypeScript strict.

## Props

```svelte
<script lang="ts">
  let { session, onSelect }: {
    session: ReviewSession;
    onSelect?: (id: string) => void;
  } = $props();
</script>
```

- Type props explicitly.
- Prefer action-oriented callback names such as `onSelect`, `onRetry`, and
  `onDismiss`.
- Do not mutate props directly.
- Keep component events and callback payloads typed.

## Runes

- Use `$state` for mutable local component state.
- Use `$derived(...)` or `$derived.by(...)` for pure derived values.
- Keep `$derived` side-effect free.
- Use `$effect` only for external effects such as browser APIs, observers,
  timers, subscriptions, editors, or network-triggering reactions.
- Clean up effects that install subscriptions, observers, or timers.

## Types

- Avoid `any`; use `unknown` at external boundaries and narrow it.
- Type route data, endpoint responses, local UI state, and component props.
- Keep feature-local types near the feature until they are shared.
- Use discriminated unions for async view state when it prevents invalid UI
  combinations.

## Anti-Patterns

- Fetching data inside visual leaf components.
- Using `$effect` for pure calculations.
- Adding global stores for route-local state.
- Hiding important state changes in broad reactive blocks.
- Letting optional fields leak through the UI without an explicit empty state.
