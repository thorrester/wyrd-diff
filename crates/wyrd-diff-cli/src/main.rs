//! Wyrd Diff local development and automation CLI.

#![forbid(unsafe_code)]

use anyhow::{Context, Result, bail};
use std::{
    env, fs,
    io::{self, Read},
    net::SocketAddr,
    path::PathBuf,
    process::Command,
};
use wyrd_diff_core::{
    Database, NewAgentSession, NewFixImport, NewRepo, NewReviewSession,
    agent_config::{ConfigAction, DEFAULT_MCP_URL, configure_agents},
};

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let command = args.next().unwrap_or_else(|| "help".to_string());
    let db = Database::new(database_path()?);

    match command.as_str() {
        "help" | "-h" | "--help" => {
            print_help();
            Ok(())
        }
        "db" => match args.next().as_deref() {
            Some("migrate") => {
                db.migrate()?;
                println!("migrated {}", db.path().display());
                Ok(())
            }
            Some("reset") => {
                db.reset()?;
                println!("reset {}", db.path().display());
                Ok(())
            }
            _ => bail!("usage: wyrd-diff-cli db <migrate|reset>"),
        },
        "repo" => match args.next().as_deref() {
            Some("add") => {
                let path = args.next().context("missing repo path")?;
                let name = args.next().unwrap_or_else(|| repo_name(&path));
                db.migrate()?;
                let repo = db.upsert_repo(NewRepo { name, path })?;
                println!("{}", serde_json::to_string_pretty(&repo)?);
                Ok(())
            }
            Some("list") => {
                db.migrate()?;
                println!("{}", serde_json::to_string_pretty(&db.repos()?)?);
                Ok(())
            }
            _ => bail!("usage: wyrd-diff-cli repo <add|list>"),
        },
        "review" => match args.next().as_deref() {
            Some("create") => {
                let repo_id = args.next().context("missing repo id")?;
                let base_ref = args.next().context("missing base ref")?;
                let head_ref = args.next().context("missing head ref")?;
                let title = args
                    .next()
                    .unwrap_or_else(|| format!("{base_ref}..{head_ref}"));
                db.migrate()?;
                let branch = args.next();
                let session = db.create_review_session(NewReviewSession {
                    repo_id,
                    title,
                    base_ref,
                    head_ref,
                    branch,
                })?;
                println!("{}", serde_json::to_string_pretty(&session)?);
                Ok(())
            }
            Some("context") => {
                let session_id = args.next().context("missing session id")?;
                db.migrate()?;
                println!(
                    "{}",
                    serde_json::to_string_pretty(&db.agent_context(&session_id)?)?
                );
                Ok(())
            }
            Some("record-fix") => {
                let session_id = args.next().context("missing session id")?;
                let commit_sha = args.next().context("missing commit sha")?;
                let response_arg = args.next();
                let tests_arg = args.next();
                db.migrate()?;
                let id = db.import_fix(NewFixImport {
                    session_id,
                    commit_sha,
                    agent_name: Some(
                        env::var("WYRD_DIFF_AGENT").unwrap_or_else(|_| "codex".to_string()),
                    ),
                    response_text: read_optional_text(response_arg)?,
                    tests_json: read_optional_text(tests_arg)?,
                    accepted: true,
                    thread_id: env::var("WYRD_DIFF_THREAD_ID").ok(),
                })?;
                println!("{id}");
                Ok(())
            }
            _ => bail!("usage: wyrd-diff-cli review <create|context|record-fix>"),
        },
        "hook" => match args.next().as_deref() {
            Some("agent-start") => {
                db.migrate()?;
                record_agent_start(&db)
            }
            Some("agent-stop") => {
                db.migrate()?;
                record_agent_stop(&db)
            }
            _ => bail!("usage: wyrd-diff-cli hook <agent-start|agent-stop>"),
        },
        "export" => match args.next().as_deref() {
            Some("trajectory") => {
                let repo_id = args.next();
                db.migrate()?;
                for record in db.trajectory_records(repo_id.as_deref())? {
                    println!("{}", serde_json::to_string(&record)?);
                }
                Ok(())
            }
            _ => bail!("usage: wyrd-diff-cli export trajectory [repo_id]"),
        },
        "configure-agents" => {
            let url = args.next().unwrap_or_else(|| DEFAULT_MCP_URL.to_string());
            let writes = configure_agents(&url)?;
            if writes.is_empty() {
                println!("no detected agent harnesses (claude, codex, opencode, gemini)");
            }
            for write in &writes {
                let label = match write.action {
                    ConfigAction::Created => "created",
                    ConfigAction::Updated => "updated",
                    ConfigAction::Unchanged => "unchanged",
                };
                println!(
                    "{} ({}): {} [{label}]",
                    write.display,
                    write.id,
                    write.path.display()
                );
            }
            println!("MCP url: {url}");
            Ok(())
        }
        "serve" => {
            db.migrate()?;
            let host = env::var("WYRD_DIFF_API_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
            let port = env::var("WYRD_DIFF_API_PORT")
                .unwrap_or_else(|_| "8765".to_string())
                .parse::<u16>()?;
            let token =
                env::var("WYRD_DIFF_TOKEN").unwrap_or_else(|_| "dev-local-token".to_string());
            let addr: SocketAddr = format!("{host}:{port}").parse()?;
            let (listener, bound) = wyrd_diff_api::bind_with_fallback(addr, 10).await?;
            let mcp_url = wyrd_diff_api::mcp_url_for(bound);
            println!("wyrd-diff API listening on http://{bound}");
            println!("wyrd-diff MCP at {mcp_url}");
            println!("authorization: Bearer {token}");
            wyrd_diff_api::serve_on(db, token, listener, mcp_url).await
        }
        other => bail!("unknown command: {other}"),
    }
}

fn database_path() -> Result<PathBuf> {
    if let Ok(url) = env::var("DATABASE_URL")
        && let Some(path) = url.strip_prefix("sqlite://")
    {
        return Ok(PathBuf::from(path));
    }
    Ok(PathBuf::from(".data/wyrd-diff.db"))
}

fn repo_name(path: &str) -> String {
    PathBuf::from(path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("repo")
        .to_string()
}

fn read_optional_text(input: Option<String>) -> Result<Option<String>> {
    match input.as_deref() {
        None => Ok(None),
        Some("-") => {
            let mut value = String::new();
            io::stdin().read_to_string(&mut value)?;
            Ok(Some(value))
        }
        Some(path) => fs::read_to_string(path)
            .with_context(|| format!("failed to read {path}"))
            .map(Some),
    }
}

fn record_agent_start(db: &Database) -> Result<()> {
    let Ok(agent_session_id) = env::var("WYRD_DIFF_AGENT_SESSION_ID") else {
        drain_stdin()?;
        println!("wyrd-diff hook skipped: WYRD_DIFF_AGENT_SESSION_ID is not set");
        return Ok(());
    };
    if agent_session_id.trim().is_empty() {
        drain_stdin()?;
        println!("wyrd-diff hook skipped: WYRD_DIFF_AGENT_SESSION_ID is empty");
        return Ok(());
    }
    let agent_name = env::var("WYRD_DIFF_AGENT").unwrap_or_else(|_| "agent".to_string());
    let repo_path = env::var("WYRD_DIFF_REPO_PATH")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(current_repo_root);
    let branch = env::var("WYRD_DIFF_BRANCH")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(current_branch);
    drain_stdin()?;
    let record = db.register_agent_session(NewAgentSession {
        agent_session_id,
        agent_name,
        repo_path,
        branch,
    })?;
    let linked = record
        .review_session_id
        .as_deref()
        .unwrap_or("(no active review for branch)");
    println!(
        "wyrd-diff hook registered agent session {} ({}); linked review: {linked}",
        record.id, record.agent_name
    );
    Ok(())
}

fn record_agent_stop(db: &Database) -> Result<()> {
    let agent_name = env::var("WYRD_DIFF_AGENT").unwrap_or_else(|_| "agent".to_string());
    let agent_session_id = env::var("WYRD_DIFF_AGENT_SESSION_ID")
        .ok()
        .filter(|value| !value.trim().is_empty());
    if let Some(id) = agent_session_id.as_deref() {
        db.bump_agent_session_activity(id, &agent_name)?;
    }

    let session_id = match agent_session_id.as_deref() {
        Some(id) => db.linked_review_session_for_agent(id, &agent_name)?,
        None => env::var("WYRD_DIFF_SESSION_ID")
            .ok()
            .filter(|value| !value.trim().is_empty()),
    };
    let Some(session_id) = session_id else {
        drain_stdin()?;
        println!("wyrd-diff hook skipped: no linked review session for agent");
        return Ok(());
    };

    let commit_sha = env::var("WYRD_DIFF_COMMIT_SHA").unwrap_or_else(|_| current_head());
    if commit_sha.is_empty() {
        drain_stdin()?;
        println!("wyrd-diff hook skipped: git HEAD is unavailable");
        return Ok(());
    }
    if env::var("WYRD_DIFF_START_SHA").is_ok_and(|start| start == commit_sha) {
        drain_stdin()?;
        println!("wyrd-diff hook skipped: HEAD has not changed");
        return Ok(());
    }
    if db.fix_import_exists(&session_id, &commit_sha)? {
        drain_stdin()?;
        println!("wyrd-diff hook skipped: {commit_sha} already recorded for {session_id}");
        return Ok(());
    }

    let response_text = if let Ok(path) = env::var("WYRD_DIFF_RESPONSE_FILE") {
        read_optional_text(Some(path))?
    } else {
        let value = drain_stdin()?;
        (!value.trim().is_empty()).then_some(value)
    };
    let tests_json = env::var("WYRD_DIFF_TESTS_FILE")
        .ok()
        .map(fs::read_to_string)
        .transpose()
        .context("failed to read WYRD_DIFF_TESTS_FILE")?;
    let accepted = env::var("WYRD_DIFF_ACCEPTED").map_or(true, |value| value != "false");

    let id = db.import_fix(NewFixImport {
        session_id,
        commit_sha,
        agent_name: Some(agent_name),
        response_text,
        tests_json,
        accepted,
        thread_id: env::var("WYRD_DIFF_THREAD_ID").ok(),
    })?;
    println!("wyrd-diff hook recorded fix import {id}");
    Ok(())
}

fn drain_stdin() -> Result<String> {
    let mut value = String::new();
    io::stdin().read_to_string(&mut value)?;
    Ok(value)
}

fn current_head() -> String {
    git_capture(&["rev-parse", "HEAD"]).unwrap_or_default()
}

fn current_repo_root() -> Option<String> {
    git_capture(&["rev-parse", "--show-toplevel"])
}

fn current_branch() -> Option<String> {
    git_capture(&["rev-parse", "--abbrev-ref", "HEAD"]).filter(|value| value != "HEAD")
}

fn git_capture(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!value.is_empty()).then_some(value)
}

fn print_help() {
    println!(
        "\
wyrd-diff-cli

Commands:
  db migrate
  db reset
  repo add <path> [name]
  repo list
  review create <repo_id> <base_ref> <head_ref> [title]
  review context <session_id>
  review record-fix <session_id> <commit_sha> [response_file|-] [tests_json_file]
  hook agent-start
  hook agent-stop
  export trajectory [repo_id]
  configure-agents [mcp_url]
  serve
"
    );
}
