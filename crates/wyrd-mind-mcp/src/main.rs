//! MCP stdio server for Wyrd Mind.

#![forbid(unsafe_code)]

use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::{
    env,
    io::{self, BufRead, BufReader, Write},
    path::PathBuf,
    process::Command,
};
use wyrd_mind_core::{Database, NewFixImport, NewRepo, NewReviewSession};

fn main() -> Result<()> {
    let db = Database::new(database_path()?);
    db.migrate()?;
    let mut server = McpServer { db };
    server.run()
}

struct McpServer {
    db: Database,
}

impl McpServer {
    fn run(&mut self) -> Result<()> {
        let stdin = io::stdin();
        let mut reader = BufReader::new(stdin.lock());
        let stdout = io::stdout();
        let mut writer = stdout.lock();

        while let Some(message) = read_message(&mut reader)? {
            let request: Value = serde_json::from_str(&message)
                .with_context(|| format!("invalid JSON-RPC message: {message}"))?;
            if request
                .get("method")
                .and_then(Value::as_str)
                .is_some_and(|method| method.starts_with("notifications/"))
            {
                continue;
            }
            let response = self.handle(request);
            write_message(&mut writer, &serde_json::to_string(&response)?)?;
        }
        Ok(())
    }

    fn handle(&self, request: Value) -> Value {
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let method = request.get("method").and_then(Value::as_str).unwrap_or("");
        let params = request.get("params").cloned().unwrap_or_else(|| json!({}));

        let result = match method {
            "initialize" => Ok(json!({
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "wyrd-mind", "version": env!("CARGO_PKG_VERSION") }
            })),
            "tools/list" => Ok(json!({ "tools": tools() })),
            "tools/call" => self.call_tool(params),
            _ => Err(format!("unsupported method: {method}")),
        };

        match result {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err(error) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32603, "message": error }
            }),
        }
    }

    fn call_tool(&self, params: Value) -> std::result::Result<Value, String> {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| "missing tool name".to_string())?;
        let args = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));

        let value = match name {
            "wyrd_mind.register_repo" => self.register_repo(args),
            "wyrd_mind.create_review_session" => self.create_review_session(args),
            "wyrd_mind.list_review_sessions" => self.list_review_sessions(args),
            "wyrd_mind.open_review_session" => self.open_review_session(args),
            "wyrd_mind.get_agent_context" => self.get_agent_context(args),
            "wyrd_mind.get_open_comments" => self.get_open_comments(args),
            "wyrd_mind.import_fix" => self.import_fix(args),
            "wyrd_mind.export_trajectory" => self.export_trajectory(args),
            _ => return Err(format!("unknown tool: {name}")),
        }
        .map_err(|error| error.to_string())?;

        Ok(json!({
            "content": [{ "type": "text", "text": serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string()) }],
            "isError": false
        }))
    }

    fn register_repo(&self, args: Value) -> Result<Value> {
        let path = required_str(&args, "path")?;
        let name = args
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| repo_name(path));
        Ok(json!({ "repo": self.db.upsert_repo(NewRepo { name, path: path.to_string() })? }))
    }

    fn create_review_session(&self, args: Value) -> Result<Value> {
        let repo_id = if let Some(repo_id) = args.get("repo_id").and_then(Value::as_str) {
            repo_id.to_string()
        } else {
            let path = required_str(&args, "repo_path")?;
            let name = args
                .get("repo_name")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| repo_name(path));
            self.db
                .upsert_repo(NewRepo {
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
        let session = self.db.create_review_session(NewReviewSession {
            repo_id,
            title,
            base_ref: base_ref.to_string(),
            head_ref: head_ref.to_string(),
        })?;
        Ok(json!({
            "review_session": session,
            "review_url": review_url(&session.id)
        }))
    }

    fn list_review_sessions(&self, args: Value) -> Result<Value> {
        let mut sessions = self.db.review_sessions()?;
        if let Some(repo_id) = args.get("repo_id").and_then(Value::as_str) {
            sessions.retain(|session| session.repo_id == repo_id);
        }
        Ok(json!({ "review_sessions": sessions }))
    }

    fn open_review_session(&self, args: Value) -> Result<Value> {
        let session_id = required_str(&args, "session_id")?;
        let url = review_url(session_id);
        if args.get("open").and_then(Value::as_bool).unwrap_or(true) {
            let _ = Command::new("open").arg(&url).status();
        }
        Ok(json!({ "session_id": session_id, "review_url": url }))
    }

    fn get_agent_context(&self, args: Value) -> Result<Value> {
        let session_id = required_str(&args, "session_id")?;
        Ok(serde_json::to_value(self.db.agent_context(session_id)?)?)
    }

    fn get_open_comments(&self, args: Value) -> Result<Value> {
        let session_id = required_str(&args, "session_id")?;
        let context = self.db.agent_context(session_id)?;
        Ok(json!({ "open_comments": context.open_comments }))
    }

    fn import_fix(&self, args: Value) -> Result<Value> {
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
        let accepted = args
            .get("accepted")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let id = self.db.import_fix(NewFixImport {
            session_id: session_id.to_string(),
            commit_sha: commit_sha.to_string(),
            agent_name,
            response_text,
            tests_json,
            accepted,
        })?;
        Ok(json!({ "fix_import_id": id }))
    }

    fn export_trajectory(&self, args: Value) -> Result<Value> {
        let repo_id = args.get("repo_id").and_then(Value::as_str);
        Ok(json!({ "records": self.db.trajectory_records(repo_id)? }))
    }
}

fn tools() -> Value {
    json!([
        tool(
            "wyrd_mind.register_repo",
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
            "wyrd_mind.create_review_session",
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
            "wyrd_mind.list_review_sessions",
            "List review sessions, optionally filtered by repo id.",
            json!({
                "type": "object",
                "properties": { "repo_id": { "type": "string" } }
            })
        ),
        tool(
            "wyrd_mind.open_review_session",
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
            "wyrd_mind.get_agent_context",
            "Return agent-ready comments, notes, decisions, repo, refs, and SHAs for a session.",
            json!({
                "type": "object",
                "required": ["session_id"],
                "properties": { "session_id": { "type": "string" } }
            })
        ),
        tool(
            "wyrd_mind.get_open_comments",
            "Return open agent-visible comments for a session.",
            json!({
                "type": "object",
                "required": ["session_id"],
                "properties": { "session_id": { "type": "string" } }
            })
        ),
        tool(
            "wyrd_mind.import_fix",
            "Import a fix commit and optional test result payload for a session.",
            json!({
                "type": "object",
                "required": ["session_id", "commit_sha"],
                "properties": {
                    "session_id": { "type": "string" },
                    "commit_sha": { "type": "string" },
                    "agent_name": { "type": "string" },
                    "response_text": { "type": "string" },
                    "tests_json": { "type": "object" },
                    "accepted": { "type": "boolean" }
                }
            })
        ),
        tool(
            "wyrd_mind.export_trajectory",
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

fn read_message(reader: &mut impl BufRead) -> Result<Option<String>> {
    let mut first = String::new();
    if reader.read_line(&mut first)? == 0 {
        return Ok(None);
    }
    if first.trim().is_empty() {
        return read_message(reader);
    }
    if !first.to_ascii_lowercase().starts_with("content-length:") {
        return Ok(Some(first));
    }

    let length = first
        .split_once(':')
        .context("invalid Content-Length header")?
        .1
        .trim()
        .parse::<usize>()?;

    loop {
        let mut header = String::new();
        reader.read_line(&mut header)?;
        if header == "\r\n" || header == "\n" || header.is_empty() {
            break;
        }
    }

    let mut body = vec![0_u8; length];
    reader.read_exact(&mut body)?;
    Ok(Some(String::from_utf8(body)?))
}

fn write_message(writer: &mut impl Write, body: &str) -> Result<()> {
    write!(writer, "Content-Length: {}\r\n\r\n{}", body.len(), body)?;
    writer.flush()?;
    Ok(())
}

fn required_str<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .with_context(|| format!("missing required argument: {key}"))
}

fn database_path() -> Result<PathBuf> {
    if let Ok(url) = env::var("DATABASE_URL")
        && let Some(path) = url.strip_prefix("sqlite://")
    {
        return Ok(PathBuf::from(path));
    }
    Ok(PathBuf::from(".data/wyrd-mind.db"))
}

fn repo_name(path: &str) -> String {
    PathBuf::from(path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("repo")
        .to_string()
}

fn review_url(session_id: &str) -> String {
    let port = env::var("WYRD_MIND_UI_PORT").unwrap_or_else(|_| "5173".to_string());
    format!("http://127.0.0.1:{port}/review/{session_id}")
}
