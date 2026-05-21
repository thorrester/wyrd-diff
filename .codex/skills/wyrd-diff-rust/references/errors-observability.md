# Errors And Observability

Errors should help the user or agent understand what failed and what to do next.

## Error Types

- Use `thiserror` for library errors.
- Use `anyhow` in binaries and top-level command orchestration.
- Preserve source errors when useful and safe.
- Include operation, path, session ID, repo ID, ref, or field context where it
  helps diagnosis.
- Avoid exposing secrets, local tokens, or irrelevant absolute paths in UI/API
  errors unless the path is the object being acted on.

## Boundary Conversion

- Convert core errors into API, CLI, MCP, or Tauri-facing errors at the edge.
- Keep HTTP status choices stable and meaningful.
- Keep CLI errors concise but actionable.
- Keep MCP errors structured enough for agents to recover.
- Keep Tauri command errors useful for frontend display.

## Observability

- Use structured `tracing` fields for repo IDs, review session IDs, commit SHAs,
  command names, and hook agent names.
- Log state transitions and durable writes at useful levels.
- Avoid logging full diffs, large payloads, tokens, or private notes by default.
- Instrument long-running workflows enough to diagnose where time was spent.
