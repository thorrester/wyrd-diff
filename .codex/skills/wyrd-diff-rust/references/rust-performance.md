# Rust Performance And Ownership

Design Rust APIs around the core domain first, then expose them through Axum,
CLI, MCP, and Tauri boundaries.

## API Shape

- Prefer structs with named fields for meaningful records.
- Use enums for closed states such as review status, decision kind, export
  mode, or hook agent.
- Use newtypes for durable IDs when it prevents accidental mixups.
- Keep validation close to constructors or parsing boundaries.
- Keep public functions explicit about inputs, outputs, and failure modes.

Avoid shaping core APIs around JSON, command-line strings, or frontend
convenience. Convert at the edge, then call Rust-native functions.

## Ownership

Borrow when the callee does not need ownership:

- Accept `&str` instead of `String` for read-only text.
- Accept `&Path` instead of `PathBuf` for read-only paths.
- Accept `&[T]` instead of `Vec<T>` when the callee only inspects items.
- Use `Arc<T>` only for real shared ownership across tasks or app state.

Acceptable clones include small IDs and response values crossing an ownership
boundary. Suspicious clones include large diffs, JSON payloads, file contents,
query results, and clones inside loops.

## Allocation

- Use `Vec::with_capacity` or `String::with_capacity` when size is known.
- Prefer `write!` into an existing `String` for repeated formatting.
- Keep JSON serialization at API, storage, export, or MCP boundaries.
- Do not serialize then deserialize just to move values between Rust modules.
- Avoid rebuilding clients, pools, schemas, regexes, or route state per request.

Optimize measured bottlenecks, but remove obvious repeated allocation in hot
paths during normal implementation.

## Traits And Dispatch

- Use concrete types when there is one implementation.
- Use enums when the set is closed and exhaustiveness matters.
- Use generic trait bounds for hot paths that need static dispatch.
- Use `Box<dyn Trait>` only for intentional runtime extensibility.

Do not add broad traits for a single caller. A small helper function is usually
better than a premature platform abstraction.
