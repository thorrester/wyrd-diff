//! Auto-configure agent harnesses (Claude Code, Codex, OpenCode, Gemini CLI,
//! and others) to use the running Wyrd Diff MCP HTTP transport.
//!
//! Each known harness is described by a `Harness` entry with a config file
//! path and a `HarnessFormat`. The format drives read, write, and snippet
//! generation. Adding a new harness is a single registry entry.

use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::{
    env, fs,
    path::{Path, PathBuf},
};

/// Default MCP endpoint exposed by the Wyrd Diff API bridge.
pub const DEFAULT_MCP_URL: &str = "http://127.0.0.1:8765/mcp";

/// Server name used inside every harness's config.
pub const SERVER_NAME: &str = "wyrd-diff";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HarnessFormat {
    /// `~/.claude.json` — `mcpServers.<name> = { type: "http", url }`.
    ClaudeJson,
    /// `~/.codex/config.toml` — `[mcp_servers.<name>]` with `url`.
    CodexToml,
    /// `~/.config/opencode/opencode.json` — `mcp.<name> = { type: "remote", url, enabled }`.
    OpencodeJson,
    /// `~/.gemini/settings.json` — `mcpServers.<name> = { httpUrl }`.
    GeminiJson,
}

#[derive(Debug, Clone)]
pub struct Harness {
    pub id: &'static str,
    pub display: &'static str,
    /// Binary name used for PATH detection.
    pub binary: &'static str,
    /// Segments relative to `$HOME` for the canonical config file.
    pub path_segments: &'static [&'static str],
    pub format: HarnessFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigAction {
    Created,
    Updated,
    Unchanged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentConfigState {
    /// Entry exists and URL matches the running bridge.
    Ok,
    /// Entry exists but URL differs from the running bridge.
    Mismatch,
    /// Config file exists but has no wyrd-diff entry.
    Missing,
    /// Config file does not exist at all.
    NoFile,
}

#[derive(Debug, Clone)]
pub struct HarnessReport {
    pub id: String,
    pub display: String,
    pub path: PathBuf,
    pub detected: bool,
    pub state: AgentConfigState,
    pub configured_url: Option<String>,
    pub snippet: String,
}

#[derive(Debug, Clone)]
pub struct HarnessWrite {
    pub id: String,
    pub display: String,
    pub path: PathBuf,
    pub action: ConfigAction,
}

/// Registry of known agent harnesses.
pub fn known_harnesses() -> Vec<Harness> {
    vec![
        Harness {
            id: "claude",
            display: "Claude Code",
            binary: "claude",
            path_segments: &[".claude.json"],
            format: HarnessFormat::ClaudeJson,
        },
        Harness {
            id: "codex",
            display: "Codex",
            binary: "codex",
            path_segments: &[".codex", "config.toml"],
            format: HarnessFormat::CodexToml,
        },
        Harness {
            id: "opencode",
            display: "OpenCode",
            binary: "opencode",
            path_segments: &[".config", "opencode", "opencode.json"],
            format: HarnessFormat::OpencodeJson,
        },
        Harness {
            id: "gemini",
            display: "Gemini CLI",
            binary: "gemini",
            path_segments: &[".gemini", "settings.json"],
            format: HarnessFormat::GeminiJson,
        },
    ]
}

/// Inspect every known harness against the running bridge URL.
///
/// # Errors
/// Returns an error when the home directory cannot be resolved or a config
/// file cannot be read or parsed.
pub fn agent_status(expected_url: &str) -> Result<Vec<HarnessReport>> {
    let home = home_dir().context("failed to resolve home directory")?;
    let path_dirs = path_dirs();
    known_harnesses()
        .into_iter()
        .map(|h| inspect(&h, &home, &path_dirs, expected_url))
        .collect()
}

/// Write entries to every **detected** harness. A harness counts as detected
/// when its config file exists or its binary is on `PATH`.
///
/// # Errors
/// Returns an error when the home directory cannot be resolved, or any write
/// fails.
pub fn configure_agents(url: &str) -> Result<Vec<HarnessWrite>> {
    let home = home_dir().context("failed to resolve home directory")?;
    let path_dirs = path_dirs();
    let mut out = Vec::new();
    for harness in known_harnesses() {
        let path = config_path(&harness, &home);
        let detected = path.exists() || binary_in_path(harness.binary, &path_dirs);
        if !detected {
            continue;
        }
        out.push(write_one(&harness, &path, url)?);
    }
    Ok(out)
}

/// Force-write to a specific harness by id even when it is not detected.
///
/// # Errors
/// Returns an error when the home directory cannot be resolved, the harness
/// id is unknown, or the write fails.
pub fn configure_agent(id: &str, url: &str) -> Result<HarnessWrite> {
    let home = home_dir().context("failed to resolve home directory")?;
    let harness = known_harnesses()
        .into_iter()
        .find(|h| h.id == id)
        .with_context(|| format!("unknown harness: {id}"))?;
    let path = config_path(&harness, &home);
    write_one(&harness, &path, url)
}

fn inspect(
    harness: &Harness,
    home: &Path,
    path_dirs: &[PathBuf],
    expected: &str,
) -> Result<HarnessReport> {
    let path = config_path(harness, home);
    let detected = path.exists() || binary_in_path(harness.binary, path_dirs);
    let (state, configured_url) = if !path.exists() {
        (AgentConfigState::NoFile, None)
    } else {
        let url = read_url(harness, &path)?;
        let state = match &url {
            Some(u) if u == expected => AgentConfigState::Ok,
            Some(_) => AgentConfigState::Mismatch,
            None => AgentConfigState::Missing,
        };
        (state, url)
    };
    Ok(HarnessReport {
        id: harness.id.to_string(),
        display: harness.display.to_string(),
        path,
        detected,
        state,
        configured_url,
        snippet: snippet_for(harness, expected),
    })
}

fn config_path(harness: &Harness, home: &Path) -> PathBuf {
    let mut path = home.to_path_buf();
    for segment in harness.path_segments {
        path.push(segment);
    }
    path
}

fn read_url(harness: &Harness, path: &Path) -> Result<Option<String>> {
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    Ok(match harness.format {
        HarnessFormat::ClaudeJson => json_url(&text, &["mcpServers", SERVER_NAME], &["url"])?,
        HarnessFormat::GeminiJson => {
            json_url(&text, &["mcpServers", SERVER_NAME], &["httpUrl", "url"])?
        }
        HarnessFormat::OpencodeJson => json_url(&text, &["mcp", SERVER_NAME], &["url"])?,
        HarnessFormat::CodexToml => toml_url(&text, &format!("[mcp_servers.{SERVER_NAME}]")),
    })
}

fn write_one(harness: &Harness, path: &Path, url: &str) -> Result<HarnessWrite> {
    let existed = path.exists();
    let existing = if existed {
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?
    } else {
        String::new()
    };
    let updated = match harness.format {
        HarnessFormat::ClaudeJson => write_claude_json(&existing, url, path)?,
        HarnessFormat::GeminiJson => write_gemini_json(&existing, url, path)?,
        HarnessFormat::OpencodeJson => write_opencode_json(&existing, url, path)?,
        HarnessFormat::CodexToml => write_codex_toml(&existing, url),
    };
    let action = if !existed {
        ConfigAction::Created
    } else if updated == existing {
        ConfigAction::Unchanged
    } else {
        ConfigAction::Updated
    };
    if action != ConfigAction::Unchanged {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        fs::write(path, &updated).with_context(|| format!("failed to write {}", path.display()))?;
    }
    Ok(HarnessWrite {
        id: harness.id.to_string(),
        display: harness.display.to_string(),
        path: path.to_path_buf(),
        action,
    })
}

fn write_claude_json(existing: &str, url: &str, path: &Path) -> Result<String> {
    upsert_json_entry(
        existing,
        path,
        &["mcpServers", SERVER_NAME],
        json!({ "type": "http", "url": url }),
    )
}

fn write_gemini_json(existing: &str, url: &str, path: &Path) -> Result<String> {
    upsert_json_entry(
        existing,
        path,
        &["mcpServers", SERVER_NAME],
        json!({ "httpUrl": url }),
    )
}

fn write_opencode_json(existing: &str, url: &str, path: &Path) -> Result<String> {
    upsert_json_entry(
        existing,
        path,
        &["mcp", SERVER_NAME],
        json!({ "type": "remote", "url": url, "enabled": true }),
    )
}

fn upsert_json_entry(
    existing: &str,
    path: &Path,
    key_path: &[&str],
    value: Value,
) -> Result<String> {
    let mut root: Value = if existing.trim().is_empty() {
        Value::Object(serde_json::Map::new())
    } else {
        serde_json::from_str(existing)
            .with_context(|| format!("failed to parse {}", path.display()))?
    };
    let inserted_at = traverse_or_create(&mut root, key_path, path)?;
    *inserted_at = value;
    let text = serde_json::to_string_pretty(&root)?;
    Ok(format!("{text}\n"))
}

fn traverse_or_create<'a>(
    root: &'a mut Value,
    key_path: &[&str],
    file_path: &Path,
) -> Result<&'a mut Value> {
    let mut current = root;
    for (index, key) in key_path.iter().enumerate() {
        let Value::Object(map) = current else {
            return Err(anyhow::anyhow!(
                "{}: expected object at {}",
                file_path.display(),
                key_path[..index].join(".")
            ));
        };
        current = map
            .entry((*key).to_string())
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
    }
    Ok(current)
}

fn write_codex_toml(existing: &str, url: &str) -> String {
    let block = format!("[mcp_servers.{SERVER_NAME}]\nurl = \"{url}\"\n");
    upsert_toml_section(existing, &format!("[mcp_servers.{SERVER_NAME}]"), &block)
}

fn upsert_toml_section(existing: &str, header: &str, block: &str) -> String {
    let lines: Vec<&str> = existing.lines().collect();
    let start = lines.iter().position(|line| line.trim() == header);

    match start {
        None => {
            let mut out = existing.trim_end().to_string();
            if !out.is_empty() {
                out.push_str("\n\n");
            }
            out.push_str(block.trim_end());
            out.push('\n');
            out
        }
        Some(start_idx) => {
            let mut end_idx = lines.len();
            for (offset, line) in lines.iter().enumerate().skip(start_idx + 1) {
                let trimmed = line.trim();
                if trimmed.starts_with('[') && trimmed.ends_with(']') {
                    end_idx = offset;
                    break;
                }
            }
            let before = lines[..start_idx].join("\n");
            let after = if end_idx < lines.len() {
                lines[end_idx..].join("\n")
            } else {
                String::new()
            };
            let mut out = String::new();
            if !before.is_empty() {
                out.push_str(&before);
                out.push_str("\n\n");
            }
            out.push_str(block.trim_end());
            out.push('\n');
            if !after.is_empty() {
                out.push('\n');
                out.push_str(&after);
                if !after.ends_with('\n') {
                    out.push('\n');
                }
            }
            out
        }
    }
}

fn json_url(text: &str, key_path: &[&str], url_fields: &[&str]) -> Result<Option<String>> {
    let parsed: Value = if text.trim().is_empty() {
        return Ok(None);
    } else {
        serde_json::from_str(text).context("failed to parse JSON")?
    };
    let mut current = &parsed;
    for key in key_path {
        match current.get(*key) {
            Some(next) => current = next,
            None => return Ok(None),
        }
    }
    for field in url_fields {
        if let Some(v) = current.get(*field).and_then(Value::as_str) {
            return Ok(Some(v.to_string()));
        }
    }
    Ok(None)
}

fn toml_url(text: &str, header: &str) -> Option<String> {
    let mut in_block = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_block = trimmed == header;
            continue;
        }
        if !in_block {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("url") {
            let rest = rest.trim_start().strip_prefix('=')?.trim();
            return Some(
                rest.trim_matches(|c: char| c == '"' || c == '\'')
                    .to_string(),
            );
        }
    }
    None
}

fn snippet_for(harness: &Harness, url: &str) -> String {
    match harness.format {
        HarnessFormat::ClaudeJson => format!(
            "{{\n  \"mcpServers\": {{\n    \"{SERVER_NAME}\": {{ \"type\": \"http\", \"url\": \"{url}\" }}\n  }}\n}}"
        ),
        HarnessFormat::GeminiJson => format!(
            "{{\n  \"mcpServers\": {{\n    \"{SERVER_NAME}\": {{ \"httpUrl\": \"{url}\" }}\n  }}\n}}"
        ),
        HarnessFormat::OpencodeJson => format!(
            "{{\n  \"mcp\": {{\n    \"{SERVER_NAME}\": {{ \"type\": \"remote\", \"url\": \"{url}\", \"enabled\": true }}\n  }}\n}}"
        ),
        HarnessFormat::CodexToml => {
            format!("[mcp_servers.{SERVER_NAME}]\nurl = \"{url}\"")
        }
    }
}

fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME").map(PathBuf::from)
}

fn path_dirs() -> Vec<PathBuf> {
    env::var_os("PATH")
        .map(|val| env::split_paths(&val).collect())
        .unwrap_or_default()
}

fn binary_in_path(name: &str, dirs: &[PathBuf]) -> bool {
    dirs.iter().any(|dir| dir.join(name).is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_toml_inserts_block_when_absent() {
        let result = upsert_toml_section(
            "",
            "[mcp_servers.wyrd-diff]",
            "[mcp_servers.wyrd-diff]\nurl = \"http://x\"\n",
        );
        assert!(result.contains("[mcp_servers.wyrd-diff]"));
        assert!(result.contains("url = \"http://x\""));
    }

    #[test]
    fn upsert_toml_replaces_block_when_present() {
        let existing = "[mcp_servers.other]\ncommand = \"x\"\n\n[mcp_servers.wyrd-diff]\nurl = \"http://old\"\n\n[other]\nfoo = 1\n";
        let block = "[mcp_servers.wyrd-diff]\nurl = \"http://new\"\n";
        let out = upsert_toml_section(existing, "[mcp_servers.wyrd-diff]", block);
        assert!(out.contains("[mcp_servers.other]"));
        assert!(out.contains("url = \"http://new\""));
        assert!(!out.contains("http://old"));
        assert!(out.contains("[other]"));
        assert!(out.contains("foo = 1"));
    }

    #[test]
    fn write_claude_json_preserves_other_servers() {
        let existing = r#"{"mcpServers":{"github":{"type":"http","url":"http://gh"}}}"#;
        let out = write_claude_json(existing, "http://wd", Path::new("/tmp/x.json")).unwrap();
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["mcpServers"]["github"]["url"], "http://gh");
        assert_eq!(parsed["mcpServers"]["wyrd-diff"]["url"], "http://wd");
        assert_eq!(parsed["mcpServers"]["wyrd-diff"]["type"], "http");
    }

    #[test]
    fn write_opencode_json_uses_remote_type() {
        let out = write_opencode_json("", "http://wd", Path::new("/tmp/oc.json")).unwrap();
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["mcp"]["wyrd-diff"]["type"], "remote");
        assert_eq!(parsed["mcp"]["wyrd-diff"]["enabled"], true);
        assert_eq!(parsed["mcp"]["wyrd-diff"]["url"], "http://wd");
    }

    #[test]
    fn write_gemini_json_uses_http_url_field() {
        let out = write_gemini_json("", "http://wd", Path::new("/tmp/g.json")).unwrap();
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["mcpServers"]["wyrd-diff"]["httpUrl"], "http://wd");
    }

    #[test]
    fn known_harnesses_have_unique_ids() {
        let harnesses = known_harnesses();
        let mut ids: Vec<&str> = harnesses.iter().map(|h| h.id).collect();
        ids.sort();
        let len = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), len, "duplicate harness id");
    }
}
