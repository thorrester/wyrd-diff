# Data Workflow UX

The app exists to help engineers review branch changes, capture durable memory,
and hand useful context to agents.

## Core Screens

Good workflow screens answer:

- What repository or review session am I looking at?
- What changed?
- What comments, notes, decisions, or accepted fixes exist?
- What is pending, risky, stale, blocked, or accepted?
- What can I inspect or do next?

## Review And Diff UX

- Keep file paths, line references, commit SHAs, and review session identity
  visible where they matter.
- Make exact diff-line comments easy to locate.
- Preserve scroll and selection context during updates.
- Use clear empty states when a session has no comments, notes, or fixes.
- Distinguish unresolved, accepted, rejected, and informational states without
  relying only on color.

## Notes, Decisions, And Trajectory

- Treat notes and decisions as durable engineering memory, not temporary chat.
- Show timestamps and authors/agents when available.
- Make accepted fix trajectory easy to distinguish from raw agent attempts.
- Keep export/import flows explicit about what will be recorded or written.

## Filters And Search

- Put high-signal filters first.
- Keep active filters visible.
- Preserve filters across refresh when they represent user intent.
- Debounce search if it triggers expensive work.
- Avoid request bursts from every keystroke when local filtering is enough.
