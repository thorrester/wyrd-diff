//! Auto-configure agent harnesses (Claude Code, Codex, OpenCode, Gemini CLI,
//! and any user-defined harness) to use the running Wyrd Diff MCP HTTP
//! transport.
//!
//! Each harness is described by a [`Harness`] entry with a resolved config
//! file path and a [`HarnessFormat`]. The format drives read, write, and
//! snippet generation. Built-in harnesses live in [`built_in_harnesses`].
//! Users can add their own at
//! `$XDG_CONFIG_HOME/wyrd-diff/wyrd-diff.toml` (or
//! `~/.config/wyrd-diff/wyrd-diff.toml`). Legacy `harnesses.toml` files at the
//! same location are still read when `wyrd-diff.toml` is absent.
//!
//! ```toml
//! [[harness]]
//! id = "pi"
//! display = "Pi"
//! binary = "pi"
//! path = "~/.pi/mcp.json"
//! format = "claude-json"
//! ```

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    env, fs,
    path::{Path, PathBuf},
};

/// Default MCP endpoint exposed by the Wyrd Diff API bridge.
pub const DEFAULT_MCP_URL: &str = "http://127.0.0.1:8765/mcp";

/// Server name used inside every harness's config.
pub const SERVER_NAME: &str = "wyrd-diff";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum HarnessFormat {
    /// `mcpServers.<name> = { type: "http", url }` (Claude Code).
    ClaudeJson,
    /// `[mcp_servers.<name>]` with `url` (Codex TOML).
    CodexToml,
    /// `mcp.<name> = { type: "remote", url, enabled }` (OpenCode).
    OpencodeJson,
    /// `mcpServers.<name> = { httpUrl }` (Gemini CLI).
    GeminiJson,
}

#[derive(Debug, Clone)]
pub struct Harness {
    pub id: String,
    pub display: String,
    /// Optional binary name used for `PATH` detection.
    pub binary: Option<String>,
    /// Fully resolved path to the harness config file.
    pub config_path: PathBuf,
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

#[derive(Debug, Default, Deserialize, Serialize)]
struct ConfigFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    home_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    harness: Vec<CustomHarnessEntry>,
}

#[derive(Debug, Deserialize, Serialize)]
struct CustomHarnessEntry {
    id: String,
    display: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    binary: Option<String>,
    path: String,
    format: HarnessFormat,
}

/// Built-in harnesses shipped with Wyrd Diff. Paths are resolved against the
/// supplied home directory.
pub fn built_in_harnesses(home: &Path) -> Vec<Harness> {
    vec![
        Harness {
            id: "claude".to_string(),
            display: "Claude Code".to_string(),
            binary: Some("claude".to_string()),
            config_path: home.join(".claude.json"),
            format: HarnessFormat::ClaudeJson,
        },
        Harness {
            id: "codex".to_string(),
            display: "Codex".to_string(),
            binary: Some("codex".to_string()),
            config_path: home.join(".codex").join("config.toml"),
            format: HarnessFormat::CodexToml,
        },
        Harness {
            id: "opencode".to_string(),
            display: "OpenCode".to_string(),
            binary: Some("opencode".to_string()),
            config_path: home.join(".config").join("opencode").join("opencode.json"),
            format: HarnessFormat::OpencodeJson,
        },
        Harness {
            id: "gemini".to_string(),
            display: "Gemini CLI".to_string(),
            binary: Some("gemini".to_string()),
            config_path: home.join(".gemini").join("settings.json"),
            format: HarnessFormat::GeminiJson,
        },
    ]
}

const CONFIG_FILENAME: &str = "wyrd-diff.toml";
const LEGACY_HARNESS_FILENAME: &str = "harnesses.toml";

/// Path to the canonical user config file (`wyrd-diff.toml`).
pub fn user_config_path(home: &Path) -> PathBuf {
    user_config_path_with(home, env::var_os("XDG_CONFIG_HOME").as_deref())
}

fn user_config_path_with(home: &Path, xdg: Option<&std::ffi::OsStr>) -> PathBuf {
    if let Some(xdg) = xdg.filter(|s| !s.is_empty()) {
        return PathBuf::from(xdg).join("wyrd-diff").join(CONFIG_FILENAME);
    }
    home.join(".config").join("wyrd-diff").join(CONFIG_FILENAME)
}

/// Path to the legacy harness-only registry (`harnesses.toml`). Still read for
/// backward compatibility when `wyrd-diff.toml` is absent.
pub fn legacy_harness_path(home: &Path) -> PathBuf {
    legacy_harness_path_with(home, env::var_os("XDG_CONFIG_HOME").as_deref())
}

fn legacy_harness_path_with(home: &Path, xdg: Option<&std::ffi::OsStr>) -> PathBuf {
    if let Some(xdg) = xdg.filter(|s| !s.is_empty()) {
        return PathBuf::from(xdg)
            .join("wyrd-diff")
            .join(LEGACY_HARNESS_FILENAME);
    }
    home.join(".config")
        .join("wyrd-diff")
        .join(LEGACY_HARNESS_FILENAME)
}

/// Backwards-compatible alias for [`user_config_path`].
#[deprecated = "use user_config_path (writes wyrd-diff.toml)"]
pub fn user_registry_path(home: &Path) -> PathBuf {
    user_config_path(home)
}

fn read_config_file(path: &Path) -> Result<ConfigFile> {
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("failed to parse {}", path.display()))
}

fn load_config_with(home: &Path, xdg: Option<&std::ffi::OsStr>) -> Result<ConfigFile> {
    let canonical = user_config_path_with(home, xdg);
    if canonical.exists() {
        return read_config_file(&canonical);
    }
    let legacy = legacy_harness_path_with(home, xdg);
    if legacy.exists() {
        return read_config_file(&legacy);
    }
    Ok(ConfigFile::default())
}

fn write_config_with(
    home: &Path,
    xdg: Option<&std::ffi::OsStr>,
    config: &ConfigFile,
) -> Result<()> {
    let path = user_config_path_with(home, xdg);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let body = toml::to_string_pretty(config).context("failed to serialize wyrd-diff.toml")?;
    fs::write(&path, body).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

/// Load user-defined harnesses. Missing file is not an error.
///
/// # Errors
/// Returns an error when the file exists but cannot be parsed.
pub fn user_harnesses(home: &Path) -> Result<Vec<Harness>> {
    user_harnesses_with(home, env::var_os("XDG_CONFIG_HOME").as_deref())
}

fn user_harnesses_with(home: &Path, xdg: Option<&std::ffi::OsStr>) -> Result<Vec<Harness>> {
    let config = load_config_with(home, xdg)?;
    config
        .harness
        .into_iter()
        .map(|entry| {
            Ok(Harness {
                id: entry.id,
                display: entry.display,
                binary: entry.binary,
                config_path: expand_user_path(&entry.path, home),
                format: entry.format,
            })
        })
        .collect()
}

/// Resolved view of the `home_dir` setting, including whether the value came
/// from disk or auto-detection.
#[derive(Debug, Clone)]
pub struct HomeDirView {
    /// Saved or detected path. `None` when neither is available.
    pub home_dir: Option<String>,
    /// True when `home_dir` was inferred via [`detect_home_dir`].
    pub inferred: bool,
}

/// Return the saved `home_dir` from `wyrd-diff.toml`, if present.
///
/// # Errors
/// Returns an error when the config file exists but cannot be parsed.
pub fn home_dir(home: &Path) -> Result<Option<String>> {
    home_dir_with(home, env::var_os("XDG_CONFIG_HOME").as_deref())
}

fn home_dir_with(home: &Path, xdg: Option<&std::ffi::OsStr>) -> Result<Option<String>> {
    Ok(load_config_with(home, xdg)?.home_dir)
}

/// Saved value, or first existing candidate from [`detect_home_dir`].
///
/// # Errors
/// Returns an error when the config file exists but cannot be parsed.
pub fn home_dir_view(home: &Path) -> Result<HomeDirView> {
    if let Some(saved) = home_dir(home)? {
        return Ok(HomeDirView {
            home_dir: Some(saved),
            inferred: false,
        });
    }
    Ok(HomeDirView {
        home_dir: detect_home_dir(home),
        inferred: true,
    })
}

/// Update `home_dir` in `wyrd-diff.toml`, preserving any existing harnesses.
///
/// # Errors
/// Returns an error when reading the existing config or writing the new file
/// fails.
pub fn set_home_dir(home: &Path, value: &str) -> Result<()> {
    set_home_dir_with(home, env::var_os("XDG_CONFIG_HOME").as_deref(), value)
}

fn set_home_dir_with(home: &Path, xdg: Option<&std::ffi::OsStr>, value: &str) -> Result<()> {
    let mut config = load_config_with(home, xdg).unwrap_or_default();
    config.home_dir = Some(value.to_string());
    write_config_with(home, xdg, &config)
}

/// Probe common conventions for a top-level repositories directory.
///
/// Returns the first existing directory under `home`, in priority order. None
/// when no candidate matches.
#[must_use]
pub fn detect_home_dir(home: &Path) -> Option<String> {
    const CANDIDATES: &[&str] = &[
        "Documents/GitHub",
        "Documents/github",
        "github",
        "GitHub",
        "code",
        "Code",
        "src",
        "dev",
        "projects",
        "Projects",
        "workspace",
    ];
    for relative in CANDIDATES {
        let candidate = home.join(relative);
        if candidate.is_dir() {
            return Some(candidate.to_string_lossy().to_string());
        }
    }
    None
}

/// Expand a leading `~/`, `~`, or `$HOME/` against `home` and return a
/// platform-string version of the path.
#[must_use]
pub fn expand_user_path_string(raw: &str, home: &Path) -> String {
    expand_user_path(raw.trim(), home)
        .to_string_lossy()
        .to_string()
}

/// Resolved view of `home_dir` for the active `$HOME`.
///
/// # Errors
/// Returns an error when `$HOME` is unset or the config file cannot be parsed.
pub fn home_dir_view_env() -> Result<HomeDirView> {
    let home = resolve_env_home().context("failed to resolve home directory")?;
    home_dir_view(&home)
}

/// Update `home_dir` in `wyrd-diff.toml` using the active `$HOME`.
///
/// # Errors
/// Returns an error when `$HOME` is unset or the file write fails.
pub fn set_home_dir_env(value: &str) -> Result<()> {
    let home = resolve_env_home().context("failed to resolve home directory")?;
    set_home_dir(&home, value)
}

/// Canonical config file path for the active `$HOME`.
///
/// # Errors
/// Returns an error when `$HOME` is unset.
pub fn user_config_path_env() -> Result<PathBuf> {
    let home = resolve_env_home().context("failed to resolve home directory")?;
    Ok(user_config_path(&home))
}

/// Expand `raw` against the active `$HOME`. Returns `raw` unchanged when
/// `$HOME` is unset.
#[must_use]
pub fn expand_user_path_env(raw: &str) -> String {
    if let Some(home) = resolve_env_home() {
        expand_user_path_string(raw, &home)
    } else {
        raw.trim().to_string()
    }
}

/// All known harnesses. User entries override built-ins on `id` collision.
///
/// # Errors
/// Returns an error when the user registry exists but cannot be parsed.
pub fn known_harnesses(home: &Path) -> Result<Vec<Harness>> {
    known_harnesses_with(home, env::var_os("XDG_CONFIG_HOME").as_deref())
}

fn known_harnesses_with(home: &Path, xdg: Option<&std::ffi::OsStr>) -> Result<Vec<Harness>> {
    let mut out = built_in_harnesses(home);
    for entry in user_harnesses_with(home, xdg)? {
        if let Some(slot) = out.iter_mut().find(|h| h.id == entry.id) {
            *slot = entry;
        } else {
            out.push(entry);
        }
    }
    Ok(out)
}

/// Inspect every known harness against the running bridge URL.
///
/// # Errors
/// Returns an error when the home directory cannot be resolved or a config
/// file cannot be read or parsed.
pub fn agent_status(expected_url: &str) -> Result<Vec<HarnessReport>> {
    let home = resolve_env_home().context("failed to resolve home directory")?;
    let path_dirs = path_dirs();
    let harnesses = known_harnesses(&home)?;
    harnesses
        .iter()
        .map(|h| inspect(h, &path_dirs, expected_url))
        .collect()
}

/// Write entries to every **detected** harness. A harness counts as detected
/// when its config file exists or its binary is on `PATH`.
///
/// # Errors
/// Returns an error when the home directory cannot be resolved, the user
/// registry cannot be parsed, or any write fails.
pub fn configure_agents(url: &str) -> Result<Vec<HarnessWrite>> {
    let home = resolve_env_home().context("failed to resolve home directory")?;
    let path_dirs = path_dirs();
    let harnesses = known_harnesses(&home)?;
    let mut out = Vec::new();
    for harness in &harnesses {
        let detected = harness.config_path.exists() || binary_detected(harness, &path_dirs);
        if !detected {
            continue;
        }
        out.push(write_one(harness, url)?);
    }
    Ok(out)
}

/// Force-write to a specific harness by id even when it is not detected.
///
/// # Errors
/// Returns an error when the home directory cannot be resolved, the harness
/// id is unknown, or the write fails.
pub fn configure_agent(id: &str, url: &str) -> Result<HarnessWrite> {
    let home = resolve_env_home().context("failed to resolve home directory")?;
    let harness = known_harnesses(&home)?
        .into_iter()
        .find(|h| h.id == id)
        .with_context(|| format!("unknown harness: {id}"))?;
    write_one(&harness, url)
}

fn inspect(harness: &Harness, path_dirs: &[PathBuf], expected: &str) -> Result<HarnessReport> {
    let detected = harness.config_path.exists() || binary_detected(harness, path_dirs);
    let (state, configured_url) = if !harness.config_path.exists() {
        (AgentConfigState::NoFile, None)
    } else {
        let url = read_url(harness)?;
        let state = match &url {
            Some(u) if u == expected => AgentConfigState::Ok,
            Some(_) => AgentConfigState::Mismatch,
            None => AgentConfigState::Missing,
        };
        (state, url)
    };
    Ok(HarnessReport {
        id: harness.id.clone(),
        display: harness.display.clone(),
        path: harness.config_path.clone(),
        detected,
        state,
        configured_url,
        snippet: snippet_for(harness.format, expected),
    })
}

fn read_url(harness: &Harness) -> Result<Option<String>> {
    let text = fs::read_to_string(&harness.config_path)
        .with_context(|| format!("failed to read {}", harness.config_path.display()))?;
    Ok(match harness.format {
        HarnessFormat::ClaudeJson => json_url(&text, &["mcpServers", SERVER_NAME], &["url"])?,
        HarnessFormat::GeminiJson => {
            json_url(&text, &["mcpServers", SERVER_NAME], &["httpUrl", "url"])?
        }
        HarnessFormat::OpencodeJson => json_url(&text, &["mcp", SERVER_NAME], &["url"])?,
        HarnessFormat::CodexToml => toml_url(&text, &format!("[mcp_servers.{SERVER_NAME}]")),
    })
}

fn write_one(harness: &Harness, url: &str) -> Result<HarnessWrite> {
    let path = harness.config_path.as_path();
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
        id: harness.id.clone(),
        display: harness.display.clone(),
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

fn snippet_for(format: HarnessFormat, url: &str) -> String {
    match format {
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

fn resolve_env_home() -> Option<PathBuf> {
    env::var_os("HOME").map(PathBuf::from)
}

fn path_dirs() -> Vec<PathBuf> {
    env::var_os("PATH")
        .map(|val| env::split_paths(&val).collect())
        .unwrap_or_default()
}

fn binary_detected(harness: &Harness, dirs: &[PathBuf]) -> bool {
    let Some(name) = harness.binary.as_deref() else {
        return false;
    };
    dirs.iter().any(|dir| dir.join(name).is_file())
}

fn expand_user_path(raw: &str, home: &Path) -> PathBuf {
    if let Some(rest) = raw.strip_prefix("~/") {
        return home.join(rest);
    }
    if raw == "~" {
        return home.to_path_buf();
    }
    if let Some(rest) = raw.strip_prefix("$HOME/") {
        return home.join(rest);
    }
    PathBuf::from(raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

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
    fn built_in_harnesses_have_unique_ids() {
        let dir = tempdir().unwrap();
        let list = built_in_harnesses(dir.path());
        let mut ids: Vec<String> = list.iter().map(|h| h.id.clone()).collect();
        let count = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), count, "duplicate built-in harness id");
    }

    #[test]
    fn expand_user_path_resolves_tilde() {
        let home = Path::new("/home/u");
        assert_eq!(expand_user_path("~/.foo/bar", home), home.join(".foo/bar"));
        assert_eq!(expand_user_path("~", home), home.to_path_buf());
        assert_eq!(expand_user_path("$HOME/x.toml", home), home.join("x.toml"));
        assert_eq!(
            expand_user_path("/abs/path", home),
            PathBuf::from("/abs/path")
        );
    }

    #[test]
    fn user_harnesses_loads_custom_entries() {
        let dir = tempdir().unwrap();
        let home = dir.path();
        let registry = home.join(".config").join("wyrd-diff");
        fs::create_dir_all(&registry).unwrap();
        let content = r#"
[[harness]]
id = "pi"
display = "Pi"
binary = "pi"
path = "~/.pi/mcp.json"
format = "claude-json"

[[harness]]
id = "custom"
display = "Custom"
path = "/abs/custom.toml"
format = "codex-toml"
"#;
        fs::write(registry.join("harnesses.toml"), content).unwrap();
        let list = user_harnesses_with(home, None).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, "pi");
        assert_eq!(list[0].config_path, home.join(".pi/mcp.json"));
        assert_eq!(list[0].format, HarnessFormat::ClaudeJson);
        assert_eq!(list[1].config_path, PathBuf::from("/abs/custom.toml"));
        assert_eq!(list[1].binary, None);
    }

    #[test]
    fn user_config_path_respects_xdg() {
        let home = Path::new("/home/u");
        let xdg = std::ffi::OsString::from("/tmp/xdg");
        let path = user_config_path_with(home, Some(xdg.as_os_str()));
        assert_eq!(path, PathBuf::from("/tmp/xdg/wyrd-diff/wyrd-diff.toml"));
        let fallback = user_config_path_with(home, None);
        assert_eq!(
            fallback,
            PathBuf::from("/home/u/.config/wyrd-diff/wyrd-diff.toml")
        );
    }

    #[test]
    fn set_home_dir_preserves_existing_harnesses() {
        let dir = tempdir().unwrap();
        let home = dir.path();
        let registry = home.join(".config").join("wyrd-diff");
        fs::create_dir_all(&registry).unwrap();
        let initial = r#"
[[harness]]
id = "pi"
display = "Pi"
binary = "pi"
path = "~/.pi/mcp.json"
format = "claude-json"
"#;
        fs::write(registry.join("wyrd-diff.toml"), initial).unwrap();

        set_home_dir_with(home, None, "/tmp/repos").unwrap();

        let written = fs::read_to_string(registry.join("wyrd-diff.toml")).unwrap();
        assert!(written.contains("home_dir = \"/tmp/repos\""));
        let list = user_harnesses_with(home, None).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "pi");
    }

    #[test]
    fn load_falls_back_to_legacy_harnesses_file() {
        let dir = tempdir().unwrap();
        let home = dir.path();
        let registry = home.join(".config").join("wyrd-diff");
        fs::create_dir_all(&registry).unwrap();
        let content = r#"
[[harness]]
id = "legacy"
display = "Legacy"
path = "/abs/legacy.toml"
format = "codex-toml"
"#;
        fs::write(registry.join("harnesses.toml"), content).unwrap();
        let list = user_harnesses_with(home, None).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "legacy");
    }

    #[test]
    fn legacy_harness_path_resolves_old_filename() {
        let home = Path::new("/home/u");
        let xdg = std::ffi::OsString::from("/tmp/xdg");
        assert_eq!(
            legacy_harness_path_with(home, Some(xdg.as_os_str())),
            PathBuf::from("/tmp/xdg/wyrd-diff/harnesses.toml")
        );
        assert_eq!(
            legacy_harness_path_with(home, None),
            PathBuf::from("/home/u/.config/wyrd-diff/harnesses.toml")
        );
    }

    #[test]
    fn known_harnesses_overrides_built_in_on_id_collision() {
        let dir = tempdir().unwrap();
        let home = dir.path();
        let registry = home.join(".config").join("wyrd-diff");
        fs::create_dir_all(&registry).unwrap();
        let content = r#"
[[harness]]
id = "claude"
display = "Claude (overridden)"
path = "/custom/claude.json"
format = "claude-json"
"#;
        fs::write(registry.join("harnesses.toml"), content).unwrap();
        let list = known_harnesses_with(home, None).unwrap();
        let claude = list.iter().find(|h| h.id == "claude").unwrap();
        assert_eq!(claude.display, "Claude (overridden)");
        assert_eq!(claude.config_path, PathBuf::from("/custom/claude.json"));
        assert_eq!(list.iter().filter(|h| h.id == "claude").count(), 1);
    }
}
