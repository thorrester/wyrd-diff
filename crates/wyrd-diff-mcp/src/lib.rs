//! Shared MCP tool surface for Wyrd Diff.
//!
//! Used by both the stdio binary and the HTTP transport in `wyrd-diff-api`.
//!
//! Design: every tool is single-purpose. Descriptions explicitly call out
//! which sibling tool to use for adjacent goals so an LLM picking by
//! name+description cannot land on the wrong one.

#![forbid(unsafe_code)]

use anyhow::Context;
use serde_json::{Value, json};
use std::{env, path::PathBuf};
use wyrd_diff_core::{
    Database, NewAgentSession, NewFixImport, NewRepo, NewReviewSession, NewThreadMessage,
};

/// JSON-RPC error codes.
const ERR_METHOD_NOT_FOUND: i64 = -32601;
const ERR_INVALID_PARAMS: i64 = -32602;
const ERR_INTERNAL: i64 = -32603;

/// Structured tool error: carries the JSON-RPC error code so callers see
/// recoverable signals (`-32602` for bad args, `-32601` for unknown tool)
/// instead of a single opaque `-32603`.
struct ToolError {
    code: i64,
    message: String,
    data: Option<Value>,
}

impl ToolError {
    fn invalid_params(message: impl Into<String>, data: Option<Value>) -> Self {
        Self {
            code: ERR_INVALID_PARAMS,
            message: message.into(),
            data,
        }
    }

    fn internal(error: anyhow::Error) -> Self {
        Self {
            code: ERR_INTERNAL,
            message: error.to_string(),
            data: None,
        }
    }

    fn not_found(name: &str) -> Self {
        Self {
            code: ERR_METHOD_NOT_FOUND,
            message: format!("unknown tool: {name}"),
            data: None,
        }
    }
}

/// Dispatch a single JSON-RPC request. Returns `None` for notifications.
pub fn dispatch(db: &Database, request: Value) -> Option<Value> {
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    if method.starts_with("notifications/") {
        return None;
    }

    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let params = request.get("params").cloned().unwrap_or_else(|| json!({}));

    match method {
        "initialize" => Some(success(
            id,
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "wyrd-diff", "version": env!("CARGO_PKG_VERSION") }
            }),
        )),
        "tools/list" => Some(success(id, json!({ "tools": tools() }))),
        "tools/call" => Some(match call_tool(db, params) {
            Ok(result) => success(id, result),
            Err(error) => error_response(id, error),
        }),
        _ => Some(error_response(
            id,
            ToolError {
                code: ERR_METHOD_NOT_FOUND,
                message: format!("unsupported method: {method}"),
                data: None,
            },
        )),
    }
}

fn success(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error_response(id: Value, error: ToolError) -> Value {
    let mut body = json!({ "code": error.code, "message": error.message });
    if let Some(data) = error.data {
        body["data"] = data;
    }
    json!({ "jsonrpc": "2.0", "id": id, "error": body })
}

fn call_tool(db: &Database, params: Value) -> std::result::Result<Value, ToolError> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::invalid_params("missing tool name", None))?;
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    let value = match name {
        // Setup
        "wyrd_diff.register_repo" => register_repo(db, args),
        "wyrd_diff.create_session" => create_session(db, args),
        "wyrd_diff.list_sessions" => list_sessions(db, args),
        "wyrd_diff.register_agent_session" => register_agent_session(db, args),
        "wyrd_diff.export_trajectory" => export_trajectory(db, args),
        // Read
        "wyrd_diff.get_session" => get_session(db, args),
        "wyrd_diff.list_open_threads" => list_open_threads(db, args),
        "wyrd_diff.get_thread" => get_thread(db, args),
        "wyrd_diff.get_review_url" => get_review_url(args),
        // Write
        "wyrd_diff.claim_next_feedback_batch" => claim_next_feedback_batch(db, args),
        "wyrd_diff.post_thread_message" => post_thread_message(db, args),
        "wyrd_diff.set_thread_status" => set_thread_status(db, args),
        "wyrd_diff.submit_fix" => submit_fix(db, args),
        _ => return Err(ToolError::not_found(name)),
    }?;

    Ok(json!({
        "content": [{
            "type": "text",
            "text": serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string())
        }],
        "isError": false
    }))
}

// ─── Setup ──────────────────────────────────────────────────────────────────

fn register_repo(db: &Database, args: Value) -> std::result::Result<Value, ToolError> {
    let path = required_str(&args, "path")?;
    let name = args
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| repo_name(path));
    let repo = db
        .upsert_repo(NewRepo {
            name,
            path: path.to_string(),
        })
        .map_err(ToolError::internal)?;
    Ok(json!({ "repo": repo }))
}

fn create_session(db: &Database, args: Value) -> std::result::Result<Value, ToolError> {
    let repo_id = if let Some(repo_id) = args.get("repo_id").and_then(Value::as_str) {
        repo_id.to_string()
    } else {
        let path = required_str(&args, "repo_path")?;
        let name = args
            .get("repo_name")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| repo_name(path));
        db.upsert_repo(NewRepo {
            name,
            path: path.to_string(),
        })
        .map_err(ToolError::internal)?
        .id
    };
    let base_ref = required_str(&args, "base_ref")?;
    let head_ref = required_str(&args, "head_ref")?;
    let title = args
        .get("title")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("{base_ref}..{head_ref}"));
    let branch = args
        .get("branch")
        .and_then(Value::as_str)
        .map(str::to_string);
    let session = db
        .create_review_session(NewReviewSession {
            repo_id,
            title,
            base_ref: base_ref.to_string(),
            head_ref: head_ref.to_string(),
            branch,
        })
        .map_err(ToolError::internal)?;
    Ok(json!({
        "review_session": session,
        "review_url": review_url(&session.id)
    }))
}

fn list_sessions(db: &Database, args: Value) -> std::result::Result<Value, ToolError> {
    let mut sessions = db.review_sessions().map_err(ToolError::internal)?;
    if let Some(repo_id) = args.get("repo_id").and_then(Value::as_str) {
        sessions.retain(|session| session.repo_id == repo_id);
    }
    Ok(json!({ "review_sessions": sessions }))
}

fn register_agent_session(db: &Database, args: Value) -> std::result::Result<Value, ToolError> {
    let agent_session_id = required_str(&args, "agent_session_id")?.to_string();
    let agent_name = args
        .get("agent_name")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let repo_path = args
        .get("repo_path")
        .and_then(Value::as_str)
        .map(str::to_string);
    let branch = args
        .get("branch")
        .and_then(Value::as_str)
        .map(str::to_string);
    let record = db
        .register_agent_session(NewAgentSession {
            agent_session_id,
            agent_name,
            repo_path,
            branch,
        })
        .map_err(ToolError::internal)?;
    Ok(json!({ "agent_session": record }))
}

fn export_trajectory(db: &Database, args: Value) -> std::result::Result<Value, ToolError> {
    let repo_id = args.get("repo_id").and_then(Value::as_str);
    let records = db
        .trajectory_records(repo_id)
        .map_err(ToolError::internal)?;
    Ok(json!({ "records": records }))
}

// ─── Read ───────────────────────────────────────────────────────────────────

fn get_session(db: &Database, args: Value) -> std::result::Result<Value, ToolError> {
    let session_id = required_str(&args, "session_id")?;
    let session = db
        .review_session(session_id)
        .map_err(ToolError::internal)?
        .ok_or_else(|| {
            ToolError::invalid_params(
                format!("session not found: {session_id}"),
                Some(json!({ "session_id": session_id })),
            )
        })?;
    let repo = db
        .repo(&session.repo_id)
        .map_err(ToolError::internal)?
        .ok_or_else(|| {
            ToolError::internal(anyhow::anyhow!("repo missing for session: {session_id}"))
        })?;
    Ok(json!({ "session": session, "repo": repo }))
}

fn list_open_threads(db: &Database, args: Value) -> std::result::Result<Value, ToolError> {
    let session_id = required_str(&args, "session_id")?;
    let threads: Vec<_> = db
        .review_threads(session_id)
        .map_err(ToolError::internal)?
        .into_iter()
        .filter(|thread| thread.status == "open" && thread.visibility == "agent")
        .collect();
    Ok(json!({ "threads": threads }))
}

fn get_thread(db: &Database, args: Value) -> std::result::Result<Value, ToolError> {
    let thread_id = required_str(&args, "thread_id")?;
    let thread = db
        .review_thread(thread_id)
        .map_err(ToolError::internal)?
        .ok_or_else(|| {
            ToolError::invalid_params(
                format!("thread not found: {thread_id}"),
                Some(json!({ "thread_id": thread_id })),
            )
        })?;
    Ok(json!({ "thread": thread }))
}

fn get_review_url(args: Value) -> std::result::Result<Value, ToolError> {
    let session_id = required_str(&args, "session_id")?;
    Ok(json!({
        "session_id": session_id,
        "review_url": review_url(session_id),
    }))
}

// ─── Write ──────────────────────────────────────────────────────────────────

fn claim_next_feedback_batch(db: &Database, args: Value) -> std::result::Result<Value, ToolError> {
    let session_id = resolve_session_id(db, &args)?;
    let agent_session_id = args.get("agent_session_id").and_then(Value::as_str);
    let agent_name = args
        .get("agent_name")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    if let Some(id) = agent_session_id {
        db.bump_agent_session_activity(id, agent_name)
            .map_err(ToolError::internal)?;
    }
    let batch = db
        .claim_pending_feedback(&session_id, agent_session_id)
        .map_err(ToolError::internal)?;
    Ok(match batch {
        Some(batch) => json!({
            "session_id": session_id,
            "pending": true,
            "batch_id": batch.id,
            "thread_count": batch.thread_count,
            "delivered_at": batch.delivered_at,
            "markdown": batch.payload,
            "threads": batch.threads,
        }),
        None => json!({
            "session_id": session_id,
            "pending": false,
            "markdown": "No pending feedback for this session.",
        }),
    })
}

fn post_thread_message(db: &Database, args: Value) -> std::result::Result<Value, ToolError> {
    let thread_id = required_str(&args, "thread_id")?.to_string();
    let body = required_str(&args, "body")?.to_string();
    let author_kind = args
        .get("author_kind")
        .and_then(Value::as_str)
        .unwrap_or("agent")
        .to_string();
    let author_name = args
        .get("author_name")
        .and_then(Value::as_str)
        .map(str::to_string);
    let message_type = args
        .get("message_type")
        .and_then(Value::as_str)
        .unwrap_or("agent_response")
        .to_string();
    let status = args
        .get("status")
        .and_then(Value::as_str)
        .map(str::to_string);
    let visibility = args
        .get("visibility")
        .and_then(Value::as_str)
        .map(str::to_string);
    let message = db
        .add_thread_message(NewThreadMessage {
            thread_id,
            author_kind: Some(author_kind),
            author_name,
            message_type,
            body,
            status,
            visibility,
            fix_import_id: None,
        })
        .map_err(ToolError::internal)?;
    Ok(json!({ "message": message }))
}

fn set_thread_status(db: &Database, args: Value) -> std::result::Result<Value, ToolError> {
    let thread_id = required_str(&args, "thread_id")?;
    let status = required_str(&args, "status")?;
    let updated = match status {
        "resolved" => db.resolve_thread(thread_id).map_err(ToolError::internal)?,
        "open" => db.reopen_thread(thread_id).map_err(ToolError::internal)?,
        other => {
            return Err(ToolError::invalid_params(
                "status must be 'open' or 'resolved'",
                Some(json!({ "field": "status", "got": other })),
            ));
        }
    };
    Ok(json!({ "updated": updated, "status": status }))
}

fn submit_fix(db: &Database, args: Value) -> std::result::Result<Value, ToolError> {
    let session_id = required_str(&args, "session_id")?;
    let commit_sha = required_str(&args, "commit_sha")?;
    let tests_json = args.get("tests_json").map(Value::to_string);
    let agent_name = args
        .get("agent_name")
        .and_then(Value::as_str)
        .map(str::to_string);
    let response_text = args
        .get("response_text")
        .and_then(Value::as_str)
        .map(str::to_string);
    let thread_id = args
        .get("thread_id")
        .and_then(Value::as_str)
        .map(str::to_string);
    let accepted = args
        .get("accepted")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let id = db
        .import_fix(NewFixImport {
            session_id: session_id.to_string(),
            commit_sha: commit_sha.to_string(),
            agent_name,
            response_text,
            tests_json,
            accepted,
            thread_id,
        })
        .map_err(ToolError::internal)?;
    let mut resolved = Vec::new();
    if let Some(ids) = args.get("resolves_thread_ids").and_then(Value::as_array) {
        for value in ids {
            if let Some(thread_id) = value.as_str()
                && db.resolve_thread(thread_id).map_err(ToolError::internal)?
            {
                resolved.push(thread_id.to_string());
            }
        }
    }
    Ok(json!({ "fix_import_id": id, "resolved_thread_ids": resolved }))
}

// ─── Helpers ────────────────────────────────────────────────────────────────

fn resolve_session_id(db: &Database, args: &Value) -> std::result::Result<String, ToolError> {
    if let Some(session_id) = args.get("session_id").and_then(Value::as_str) {
        return Ok(session_id.to_string());
    }
    if let Some(agent_session_id) = args.get("agent_session_id").and_then(Value::as_str) {
        let agent_name = args
            .get("agent_name")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        if let Some(linked) = db
            .linked_review_session_for_agent(agent_session_id, agent_name)
            .map_err(ToolError::internal)?
        {
            return Ok(linked);
        }
    }
    if let Some(path) = args.get("repo_path").and_then(Value::as_str) {
        let branch = args.get("branch").and_then(Value::as_str);
        let active = db
            .active_review_session_for_repo_path(path, branch)
            .map_err(ToolError::internal)?
            .with_context(|| format!("no active review session for repo: {path}"))
            .map_err(ToolError::internal)?;
        return Ok(active.session.id);
    }
    Err(ToolError::invalid_params(
        "must supply session_id, or (agent_session_id + agent_name), or (repo_path + optional branch)",
        Some(json!({
            "accepted_keys": ["session_id", "agent_session_id+agent_name", "repo_path+branch"]
        })),
    ))
}

fn required_str<'a>(args: &'a Value, key: &str) -> std::result::Result<&'a str, ToolError> {
    args.get(key).and_then(Value::as_str).ok_or_else(|| {
        ToolError::invalid_params(
            format!("missing required argument: {key}"),
            Some(json!({ "field": key })),
        )
    })
}

fn repo_name(path: &str) -> String {
    PathBuf::from(path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("repo")
        .to_string()
}

fn review_url(session_id: &str) -> String {
    let port = env::var("WYRD_DIFF_UI_PORT").unwrap_or_else(|_| "5173".to_string());
    format!("http://127.0.0.1:{port}/review/{session_id}")
}

// ─── Tool registry ──────────────────────────────────────────────────────────

fn tools() -> Value {
    json!([
        tool(
            "wyrd_diff.register_repo",
            "Register or update a local git repository.\n\
             \n\
             Use when: you need a `repo_id` before calling \
             `wyrd_diff.create_session` and only have a filesystem path.\n\
             Do NOT use to: create a review session — use `wyrd_diff.create_session` \
             after this returns.\n\
             \n\
             Returns: { repo }.\n\
             Mutates: upserts a row in `repos`.",
            json!({
                "type": "object",
                "required": ["path"],
                "properties": {
                    "path": { "type": "string", "description": "Absolute path to the git repo." },
                    "name": { "type": "string", "description": "Optional display name. Defaults to the directory name." }
                }
            })
        ),
        tool(
            "wyrd_diff.create_session",
            "Create a review session from a base ref and head ref.\n\
             \n\
             Use when: starting a new review for an agent to fix. Pass either \
             `repo_id` (after register_repo) or `repo_path` (auto-registers).\n\
             Do NOT use to: list existing sessions — use `wyrd_diff.list_sessions`.\n\
             \n\
             Returns: { review_session, review_url }.\n\
             Mutates: inserts a row in `review_sessions`.",
            json!({
                "type": "object",
                "required": ["base_ref", "head_ref"],
                "properties": {
                    "repo_id": { "type": "string" },
                    "repo_path": { "type": "string" },
                    "repo_name": { "type": "string" },
                    "base_ref": { "type": "string" },
                    "head_ref": { "type": "string" },
                    "branch": { "type": "string" },
                    "title": { "type": "string" }
                }
            })
        ),
        tool(
            "wyrd_diff.list_sessions",
            "List review sessions, optionally filtered by repo id. Read-only.\n\
             \n\
             Use when: an agent needs to discover existing sessions in a repo.\n\
             Do NOT use to: fetch a single session's metadata — use \
             `wyrd_diff.get_session`. Do NOT use to: read open threads — use \
             `wyrd_diff.list_open_threads`.\n\
             \n\
             Returns: { review_sessions: [ReviewSession...] }.\n\
             Mutates: nothing.",
            json!({
                "type": "object",
                "properties": { "repo_id": { "type": "string" } }
            })
        ),
        tool(
            "wyrd_diff.register_agent_session",
            "Register or refresh an agent harness session, optionally binding \
             it to a repo path and branch.\n\
             \n\
             Use when: an agent loop boots and wants subsequent \
             `claim_next_feedback_batch` calls to resolve the active review \
             from (agent_session_id, agent_name) or (repo_path, branch).\n\
             Do NOT use to: claim feedback — use \
             `wyrd_diff.claim_next_feedback_batch` after this.\n\
             \n\
             Returns: { agent_session }.\n\
             Mutates: upserts a row in `agent_sessions`.",
            json!({
                "type": "object",
                "required": ["agent_session_id"],
                "properties": {
                    "agent_session_id": { "type": "string" },
                    "agent_name": { "type": "string" },
                    "repo_path": { "type": "string" },
                    "branch": { "type": "string" }
                }
            })
        ),
        tool(
            "wyrd_diff.export_trajectory",
            "Export accepted trajectory records (session + decisions + fix) for \
             training/audit. Read-only.\n\
             \n\
             Use when: collecting accepted fixes across sessions.\n\
             Do NOT use to: read a single session's state — use \
             `wyrd_diff.get_session` and `wyrd_diff.list_open_threads`.\n\
             \n\
             Returns: { records: [TrajectoryRecord...] }.\n\
             Mutates: nothing.",
            json!({
                "type": "object",
                "properties": { "repo_id": { "type": "string" } }
            })
        ),
        tool(
            "wyrd_diff.get_session",
            "Fetch a single review session's repo and metadata. Read-only.\n\
             \n\
             Use when: an agent needs the base/head SHAs, refs, repo path, \
             title, or status for a known session id.\n\
             Do NOT use to: read review threads — use \
             `wyrd_diff.list_open_threads` or `wyrd_diff.get_thread`. Do NOT \
             use to: claim newly-dispatched feedback for an agent loop — use \
             `wyrd_diff.claim_next_feedback_batch`.\n\
             \n\
             Returns: { session, repo }.\n\
             Mutates: nothing.",
            json!({
                "type": "object",
                "required": ["session_id"],
                "properties": { "session_id": { "type": "string" } }
            })
        ),
        tool(
            "wyrd_diff.list_open_threads",
            "List all open inline review threads for a session, each with its \
             full message history. Read-only.\n\
             \n\
             Use when: you need to see what reviewer feedback is still open \
             and unaddressed across the whole session.\n\
             Do NOT use to: claim newly-dispatched feedback for an agent \
             loop — use `wyrd_diff.claim_next_feedback_batch` (state-mutating, \
             marks the batch delivered). Do NOT use to: fetch session \
             metadata (refs, SHAs, repo path) — use `wyrd_diff.get_session`. \
             Do NOT use to: fetch one specific thread by id — use \
             `wyrd_diff.get_thread`.\n\
             \n\
             Returns: { threads: [ReviewThread...] }. Each thread includes \
             anchor, status, visibility, and ordered messages.\n\
             Mutates: nothing.",
            json!({
                "type": "object",
                "required": ["session_id"],
                "properties": { "session_id": { "type": "string" } }
            })
        ),
        tool(
            "wyrd_diff.get_thread",
            "Fetch one review thread by id, including its full ordered \
             message history. Read-only.\n\
             \n\
             Use when: an agent already has a `thread_id` (e.g. from a \
             claimed batch) and needs the latest message state.\n\
             Do NOT use to: enumerate all open threads — use \
             `wyrd_diff.list_open_threads`. Do NOT use to: post a new \
             message — use `wyrd_diff.post_thread_message`.\n\
             \n\
             Returns: { thread }.\n\
             Mutates: nothing.",
            json!({
                "type": "object",
                "required": ["thread_id"],
                "properties": { "thread_id": { "type": "string" } }
            })
        ),
        tool(
            "wyrd_diff.get_review_url",
            "Return the local web URL for a review session. Read-only.\n\
             \n\
             Use when: surfacing a click-through link to the human reviewer \
             in chat output.\n\
             Do NOT use to: open the URL — the agent host should hand the \
             URL to the user; this tool will not invoke a browser. Do NOT use \
             to: fetch session metadata — use `wyrd_diff.get_session`.\n\
             \n\
             Returns: { session_id, review_url }.\n\
             Mutates: nothing.",
            json!({
                "type": "object",
                "required": ["session_id"],
                "properties": { "session_id": { "type": "string" } }
            })
        ),
        tool(
            "wyrd_diff.claim_next_feedback_batch",
            "Claim the next pending feedback batch for an agent session and \
             mark it delivered. State-mutating.\n\
             \n\
             Use when: an agent loop polls for new reviewer feedback to act \
             on. Subsequent calls return only feedback past the watermark.\n\
             Do NOT use to: read all open threads (regardless of delivery \
             state) — use `wyrd_diff.list_open_threads`. Do NOT use to: \
             reply or resolve — use `wyrd_diff.post_thread_message` or \
             `wyrd_diff.set_thread_status`.\n\
             \n\
             Session resolution order: explicit `session_id`, then linked \
             review for `(agent_session_id, agent_name)`, then active review \
             for `(repo_path, branch)`. Supply at least one of these.\n\
             \n\
             Returns: when pending — { session_id, pending: true, batch_id, \
             thread_count, delivered_at, markdown, threads }. When empty — \
             { session_id, pending: false, markdown }.\n\
             Mutates: marks the batch as delivered and bumps agent session \
             activity.",
            json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "repo_path": { "type": "string" },
                    "branch": { "type": "string" },
                    "agent_session_id": { "type": "string" },
                    "agent_name": { "type": "string" }
                }
            })
        ),
        tool(
            "wyrd_diff.post_thread_message",
            "Post a textual reply on a review thread without requiring a \
             commit. State-mutating.\n\
             \n\
             Use when: an agent needs to ask a clarifying question, explain \
             a plan, or acknowledge feedback before (or instead of) landing \
             a commit.\n\
             Do NOT use to: record a fix commit — use `wyrd_diff.submit_fix` \
             once code lands. Do NOT use to: resolve or reopen a thread — \
             use `wyrd_diff.set_thread_status`.\n\
             \n\
             Returns: { message }.\n\
             Mutates: inserts a row in `thread_messages`.",
            json!({
                "type": "object",
                "required": ["thread_id", "body"],
                "properties": {
                    "thread_id": { "type": "string" },
                    "body": { "type": "string" },
                    "author_kind": { "type": "string", "description": "Defaults to 'agent'." },
                    "author_name": { "type": "string" },
                    "message_type": { "type": "string", "description": "Defaults to 'agent_response'. Use 'question' or 'comment' for non-fix replies." },
                    "status": { "type": "string" },
                    "visibility": { "type": "string" }
                }
            })
        ),
        tool(
            "wyrd_diff.set_thread_status",
            "Set a review thread's status to `open` or `resolved`. \
             State-mutating.\n\
             \n\
             Use when: an agent has finished addressing feedback (set to \
             `resolved`) or needs to reopen a previously closed thread \
             (set to `open`).\n\
             Do NOT use to: record a fix commit — use `wyrd_diff.submit_fix`, \
             which can auto-resolve threads via `resolves_thread_ids`. Do \
             NOT use to: post a textual reply — use \
             `wyrd_diff.post_thread_message`.\n\
             \n\
             `status` must be exactly `\"open\"` or `\"resolved\"`. Any other \
             value returns JSON-RPC -32602 (invalid params).\n\
             \n\
             Returns: { updated, status }.\n\
             Mutates: updates `review_threads.status`.",
            json!({
                "type": "object",
                "required": ["thread_id", "status"],
                "properties": {
                    "thread_id": { "type": "string" },
                    "status": { "type": "string", "enum": ["open", "resolved"] }
                }
            })
        ),
        tool(
            "wyrd_diff.submit_fix",
            "Record a fix commit (sha + agent response + optional tests) \
             against a session, and optionally bulk-resolve threads it \
             addresses. State-mutating.\n\
             \n\
             Use when: an agent has landed code that addresses one or more \
             open threads. Pass `resolves_thread_ids` to auto-mark those \
             threads `resolved` in the same call.\n\
             Do NOT use to: post a non-commit reply — use \
             `wyrd_diff.post_thread_message`. Do NOT use to: change a thread \
             status without a commit — use `wyrd_diff.set_thread_status`.\n\
             \n\
             Returns: { fix_import_id, resolved_thread_ids }.\n\
             Mutates: inserts a row in `fix_imports`; optionally updates \
             `review_threads.status` for each id in `resolves_thread_ids`.",
            json!({
                "type": "object",
                "required": ["session_id", "commit_sha"],
                "properties": {
                    "session_id": { "type": "string" },
                    "commit_sha": { "type": "string" },
                    "agent_name": { "type": "string" },
                    "response_text": { "type": "string" },
                    "thread_id": { "type": "string" },
                    "tests_json": { "type": "object" },
                    "accepted": { "type": "boolean" },
                    "resolves_thread_ids": {
                        "type": "array",
                        "items": { "type": "string" }
                    }
                }
            })
        )
    ])
}

fn tool(name: &str, description: &str, input_schema: Value) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": input_schema
    })
}
