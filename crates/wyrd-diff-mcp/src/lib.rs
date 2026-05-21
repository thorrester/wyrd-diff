//! Shared MCP tool surface for Wyrd Diff.
//!
//! Used by both the stdio binary and the HTTP transport in `wyrd-diff-api`.

#![forbid(unsafe_code)]

use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::{env, path::PathBuf, process::Command};
use wyrd_diff_core::{Database, NewFixImport, NewRepo, NewReviewSession};

/// Dispatch a single JSON-RPC request. Returns `None` for notifications.
pub fn dispatch(db: &Database, request: Value) -> Option<Value> {
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    if method.starts_with("notifications/") {
        return None;
    }

    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let params = request.get("params").cloned().unwrap_or_else(|| json!({}));

    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "wyrd-diff", "version": env!("CARGO_PKG_VERSION") }
        })),
        "tools/list" => Ok(json!({ "tools": tools() })),
        "tools/call" => call_tool(db, params),
        _ => Err(format!("unsupported method: {method}")),
    };

    Some(match result {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err(error) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": -32603, "message": error }
        }),
    })
}

fn call_tool(db: &Database, params: Value) -> std::result::Result<Value, String> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing tool name".to_string())?;
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    let value = match name {
        "wyrd_diff.register_repo" => register_repo(db, args),
        "wyrd_diff.create_review_session" => create_review_session(db, args),
        "wyrd_diff.list_review_sessions" => list_review_sessions(db, args),
        "wyrd_diff.open_review_session" => open_review_session(args),
        "wyrd_diff.get_agent_context" => get_agent_context(db, args),
        "wyrd_diff.get_open_comments" => get_open_comments(db, args),
        "wyrd_diff.pending_feedback" => pending_feedback(db, args),
        "wyrd_diff.import_fix" => import_fix(db, args),
        "wyrd_diff.export_trajectory" => export_trajectory(db, args),
        _ => return Err(format!("unknown tool: {name}")),
    }
    .map_err(|error| error.to_string())?;

    Ok(json!({
        "content": [{
            "type": "text",
            "text": serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string())
        }],
        "isError": false
    }))
}

fn register_repo(db: &Database, args: Value) -> Result<Value> {
    let path = required_str(&args, "path")?;
    let name = args
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| repo_name(path));
    Ok(json!({ "repo": db.upsert_repo(NewRepo { name, path: path.to_string() })? }))
}

fn create_review_session(db: &Database, args: Value) -> Result<Value> {
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
        })?
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
    let session = db.create_review_session(NewReviewSession {
        repo_id,
        title,
        base_ref: base_ref.to_string(),
        head_ref: head_ref.to_string(),
        branch,
    })?;
    Ok(json!({
        "review_session": session,
        "review_url": review_url(&session.id)
    }))
}

fn list_review_sessions(db: &Database, args: Value) -> Result<Value> {
    let mut sessions = db.review_sessions()?;
    if let Some(repo_id) = args.get("repo_id").and_then(Value::as_str) {
        sessions.retain(|session| session.repo_id == repo_id);
    }
    Ok(json!({ "review_sessions": sessions }))
}

fn open_review_session(args: Value) -> Result<Value> {
    let session_id = required_str(&args, "session_id")?;
    let url = review_url(session_id);
    if args.get("open").and_then(Value::as_bool).unwrap_or(true) {
        let _ = Command::new("open").arg(&url).status();
    }
    Ok(json!({ "session_id": session_id, "review_url": url }))
}

fn get_agent_context(db: &Database, args: Value) -> Result<Value> {
    let session_id = required_str(&args, "session_id")?;
    Ok(serde_json::to_value(db.agent_context(session_id)?)?)
}

fn get_open_comments(db: &Database, args: Value) -> Result<Value> {
    let session_id = required_str(&args, "session_id")?;
    let context = db.agent_context(session_id)?;
    Ok(json!({ "open_comments": context.open_comments }))
}

fn pending_feedback(db: &Database, args: Value) -> Result<Value> {
    let session_id = resolve_session_id(db, &args)?;
    let agent_session_id = args.get("agent_session_id").and_then(Value::as_str);
    let batch = db.claim_pending_feedback(&session_id, agent_session_id)?;
    match batch {
        Some(batch) => Ok(json!({
            "session_id": session_id,
            "pending": true,
            "batch_id": batch.id,
            "thread_count": batch.thread_count,
            "delivered_at": batch.delivered_at,
            "markdown": batch.payload,
            "threads": batch.threads,
        })),
        None => Ok(json!({
            "session_id": session_id,
            "pending": false,
            "markdown": "No pending feedback for this session.",
        })),
    }
}

fn import_fix(db: &Database, args: Value) -> Result<Value> {
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
    let id = db.import_fix(NewFixImport {
        session_id: session_id.to_string(),
        commit_sha: commit_sha.to_string(),
        agent_name,
        response_text,
        tests_json,
        accepted,
        thread_id,
    })?;
    let mut resolved = Vec::new();
    if let Some(ids) = args.get("resolves_thread_ids").and_then(Value::as_array) {
        for value in ids {
            if let Some(thread_id) = value.as_str()
                && db.resolve_thread(thread_id)?
            {
                resolved.push(thread_id.to_string());
            }
        }
    }
    Ok(json!({ "fix_import_id": id, "resolved_thread_ids": resolved }))
}

fn resolve_session_id(db: &Database, args: &Value) -> Result<String> {
    if let Some(session_id) = args.get("session_id").and_then(Value::as_str) {
        return Ok(session_id.to_string());
    }
    if let Some(agent_session_id) = args.get("agent_session_id").and_then(Value::as_str) {
        let agent_name = args
            .get("agent_name")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        if let Some(linked) = db.linked_review_session_for_agent(agent_session_id, agent_name)? {
            return Ok(linked);
        }
    }
    if let Some(path) = args.get("repo_path").and_then(Value::as_str) {
        let branch = args.get("branch").and_then(Value::as_str);
        let active = db
            .active_review_session_for_repo_path(path, branch)?
            .with_context(|| format!("no active review session for repo: {path}"))?;
        return Ok(active.session.id);
    }
    Err(anyhow::anyhow!(
        "missing required argument: session_id (or agent_session_id + agent_name, or repo_path)"
    ))
}

fn export_trajectory(db: &Database, args: Value) -> Result<Value> {
    let repo_id = args.get("repo_id").and_then(Value::as_str);
    Ok(json!({ "records": db.trajectory_records(repo_id)? }))
}

fn tools() -> Value {
    json!([
        tool(
            "wyrd_diff.register_repo",
            "Register or update a local git repository.",
            json!({
                "type": "object",
                "required": ["path"],
                "properties": {
                    "path": { "type": "string" },
                    "name": { "type": "string" }
                }
            })
        ),
        tool(
            "wyrd_diff.create_review_session",
            "Create a review session from a base ref and head ref. Pass repo_id or repo_path.",
            json!({
                "type": "object",
                "required": ["base_ref", "head_ref"],
                "properties": {
                    "repo_id": { "type": "string" },
                    "repo_path": { "type": "string" },
                    "repo_name": { "type": "string" },
                    "base_ref": { "type": "string" },
                    "head_ref": { "type": "string" },
                    "title": { "type": "string" }
                }
            })
        ),
        tool(
            "wyrd_diff.list_review_sessions",
            "List review sessions, optionally filtered by repo id.",
            json!({
                "type": "object",
                "properties": { "repo_id": { "type": "string" } }
            })
        ),
        tool(
            "wyrd_diff.open_review_session",
            "Open or return the local review URL for a session.",
            json!({
                "type": "object",
                "required": ["session_id"],
                "properties": {
                    "session_id": { "type": "string" },
                    "open": { "type": "boolean" }
                }
            })
        ),
        tool(
            "wyrd_diff.get_agent_context",
            "Return agent-ready comments, notes, decisions, repo, refs, and SHAs for a session.",
            json!({
                "type": "object",
                "required": ["session_id"],
                "properties": { "session_id": { "type": "string" } }
            })
        ),
        tool(
            "wyrd_diff.get_open_comments",
            "Return open agent-visible comments for a session.",
            json!({
                "type": "object",
                "required": ["session_id"],
                "properties": { "session_id": { "type": "string" } }
            })
        ),
        tool(
            "wyrd_diff.pending_feedback",
            "Claim the newest pending feedback batch for a session and mark it delivered. Returns markdown the reviewer queued; subsequent calls only return new replies past the watermark. Pass session_id, or repo_path to use that repo's active session.",
            json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "repo_path": { "type": "string" },
                    "agent_session_id": { "type": "string" }
                }
            })
        ),
        tool(
            "wyrd_diff.import_fix",
            "Import a fix commit and optional test result payload for a session. Pass resolves_thread_ids to auto-resolve threads addressed by this fix.",
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
        ),
        tool(
            "wyrd_diff.export_trajectory",
            "Export accepted trajectory records, optionally filtered by repo id.",
            json!({
                "type": "object",
                "properties": { "repo_id": { "type": "string" } }
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

fn required_str<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .with_context(|| format!("missing required argument: {key}"))
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
