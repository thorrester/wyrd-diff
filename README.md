# wyrd-diff

Local review, decision, and code trajectory memory for branch-based engineering work.

`wyrd-diff` stores review comments, private thinking notes, durable decisions, agent handoff packets, and accepted fix trajectory in a local SQLite database. Target repositories stay clean; engineering memory lives here.

## Development

All tasks are run through `mise`.

```bash
mise install
mise run db:migrate
mise run dev
mise run check
```

`mise run dev` opens the Tauri desktop app. The app starts the local
`127.0.0.1:8765` bridge itself, so the normal desktop workflow does not require
running a separate API server. Browser-only and API-only tasks remain available
for debugging:

```bash
mise run dev:ui
mise run dev:api
mise run dev:mcp
```

## Agent Setup (one-time)

The Tauri app auto-starts the local bridge on `http://127.0.0.1:8765` which
exposes the MCP HTTP transport at `/mcp`. After `mise run dev` is running:

- Click **Configure Claude + Codex** on the home screen, **or**
- Run `mise run configure:agents`

Both write `~/.claude.json` (`mcpServers.wyrd-diff`) and `~/.codex/config.toml`
(`[mcp_servers.wyrd-diff]`). Other MCP entries are left untouched. Restart your
agent client to pick up the new server.

## First Workflow

1. Register a local git repository.
2. Create a review session from a base ref and head ref.
3. Comment on exact diff lines.
4. Record notes and decisions without committing them to the target repo.
5. Expose agent-ready context through the local API.
6. Import resulting fix commits and test outcomes.
7. Export accepted trajectory as JSONL.

## Agent Recording Hook

Wyrd Diff can record agent fix trajectory automatically from Codex or Claude
Stop hooks. The hook is intentionally inert unless a review session is active:

```bash
export WYRD_DIFF_SESSION_ID="<review_session_id>"
export WYRD_DIFF_START_SHA="$(git rev-parse HEAD)"
export WYRD_DIFF_AGENT="codex"
```

When the agent finishes a turn, the hook records the current `HEAD` commit if it
changed from `WYRD_DIFF_START_SHA` and has not already been recorded for the
session. It captures the fix commit diff, hook payload or response text, optional
test JSON, and acceptance state.

Manual equivalent:

```bash
cargo run --manifest-path /path/to/wyrd-diff/Cargo.toml --locked -p wyrd-diff-cli -- \
  review record-fix <session_id> <commit_sha> agent-response.md tests.json
```

Hook command:

```bash
cargo run --manifest-path /path/to/wyrd-diff/Cargo.toml --locked -p wyrd-diff-cli -- hook agent-stop
```

Example configs:

- `examples/codex-hooks.json`
- `examples/claude-settings.local.json`

For Claude Code, put the hook in `.claude/settings.local.json` in the target
repo. For Codex, put it in `.codex/hooks.json` in the target repo. Do not commit
local session ids or machine-specific paths.
