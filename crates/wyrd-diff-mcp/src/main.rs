//! MCP stdio server for Wyrd Diff.
//!
//! Most logic lives in `wyrd_diff_mcp::dispatch`; this binary is a thin
//! stdio loop wrapping that shared dispatcher.

#![forbid(unsafe_code)]

use anyhow::{Context, Result};
use serde_json::Value;
use std::{
    env,
    io::{self, BufRead, BufReader, Write},
    path::PathBuf,
};
use wyrd_diff_core::Database;
use wyrd_diff_mcp::dispatch;

fn main() -> Result<()> {
    let db = Database::new(database_path()?);
    db.migrate()?;

    let stdin = io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    let stdout = io::stdout();
    let mut writer = stdout.lock();

    while let Some(message) = read_message(&mut reader)? {
        let request: Value = serde_json::from_str(&message)
            .with_context(|| format!("invalid JSON-RPC message: {message}"))?;
        if let Some(response) = dispatch(&db, request) {
            write_message(&mut writer, &serde_json::to_string(&response)?)?;
        }
    }
    Ok(())
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

fn database_path() -> Result<PathBuf> {
    if let Ok(url) = env::var("DATABASE_URL")
        && let Some(path) = url.strip_prefix("sqlite://")
    {
        return Ok(PathBuf::from(path));
    }
    Ok(PathBuf::from(".data/wyrd-diff.db"))
}
