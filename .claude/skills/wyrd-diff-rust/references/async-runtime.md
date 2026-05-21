# Async Runtime And Concurrency

Use async at IO boundaries and keep core computation synchronous unless async is
part of the caller contract.

## Tokio Rules

- Do not create ad hoc Tokio runtimes in libraries, handlers, or Tauri commands.
- Use the runtime supplied by the binary, API server, or Tauri host.
- Add timeouts around external or long-running operations when the caller needs
  bounded behavior.
- Use cancellation-aware patterns for background work.

## Blocking Work

Potentially blocking work includes:

- Git commands and repository scans
- Filesystem reads over large directories
- SQLite operations under contention
- Export generation
- Process execution from hooks or CLI flows

If blocking work can be expensive from an async handler, isolate it with a clear
blocking strategy and keep the result typed.

## Concurrency

- Bound concurrent work that touches repositories, filesystem, ports, or SQLite.
- Prefer message passing or short-lived locks over broad shared mutable state.
- Keep lock scopes small and never hold a lock across `.await` unless the lock
  type and design explicitly support it.
- Avoid request-triggered background tasks with no owner, cancellation, or error
  reporting path.
