//! Git command integration and unified diff parsing.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, process::Command};

/// Local git repository.
#[derive(Debug, Clone)]
pub struct GitRepo {
    path: PathBuf,
}

/// File diff with parsed hunks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffFile {
    /// File path in the head ref.
    pub path: String,
    /// Old path for renames.
    pub old_path: Option<String>,
    /// File status.
    pub status: String,
    /// Added line count.
    pub additions: i64,
    /// Deleted line count.
    pub deletions: i64,
    /// Parsed hunks.
    pub hunks: Vec<DiffHunk>,
}

/// Unified diff hunk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffHunk {
    /// Old-side start.
    pub old_start: i64,
    /// Old-side line count.
    pub old_lines: i64,
    /// New-side start.
    pub new_start: i64,
    /// New-side line count.
    pub new_lines: i64,
    /// Raw hunk patch.
    pub patch: String,
    /// Parsed diff lines.
    pub lines: Vec<DiffLine>,
}

/// Parsed diff line.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffLine {
    /// Old-side line number.
    pub old_line: Option<i64>,
    /// New-side line number.
    pub new_line: Option<i64>,
    /// Line kind: add, del, ctx, meta, or hunk.
    pub line_kind: String,
    /// Raw line content.
    pub content: String,
}

impl GitRepo {
    /// Create a git repository wrapper.
    ///
    /// # Errors
    /// Returns an error when the path is not a git worktree.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self> {
        let repo = Self { path: path.into() };
        repo.git(["rev-parse", "--show-toplevel"])?;
        Ok(repo)
    }

    /// Repository path.
    #[must_use]
    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    /// Resolve a ref to a full sha.
    ///
    /// # Errors
    /// Returns an error when git cannot resolve the ref.
    pub fn resolve_ref(&self, reference: &str) -> Result<String> {
        Ok(self.git(["rev-parse", reference])?.trim().to_string())
    }

    /// Return commits in base..head order.
    ///
    /// # Errors
    /// Returns an error when git log fails.
    pub fn commits(&self, base: &str, head: &str) -> Result<Vec<CommitInfo>> {
        let format = "%H%x1f%h%x1f%s%x1f%b%x1f%an%x1f%ae%x1f%aI%x1e";
        let output = self.git([
            "log",
            "--reverse",
            &format!("--format={format}"),
            &format!("{base}..{head}"),
        ])?;
        Ok(output
            .split('\x1e')
            .filter_map(|row| {
                let row = row.trim_matches('\n');
                if row.is_empty() {
                    return None;
                }
                let parts: Vec<_> = row.split('\x1f').collect();
                Some(CommitInfo {
                    sha: parts.first().unwrap_or(&"").to_string(),
                    short_sha: parts.get(1).unwrap_or(&"").to_string(),
                    subject: parts.get(2).unwrap_or(&"").to_string(),
                    body: empty_to_none(parts.get(3).copied().unwrap_or("")),
                    author_name: empty_to_none(parts.get(4).copied().unwrap_or("")),
                    author_email: empty_to_none(parts.get(5).copied().unwrap_or("")),
                    authored_at: empty_to_none(parts.get(6).copied().unwrap_or("")),
                })
            })
            .collect())
    }

    /// Return parsed diff files for base..head.
    ///
    /// # Errors
    /// Returns an error when git diff fails.
    pub fn diff_files(&self, base: &str, head: &str) -> Result<Vec<DiffFile>> {
        let diff = self.git([
            "diff",
            "--find-renames",
            "--unified=80",
            &format!("{base}..{head}"),
        ])?;
        Ok(parse_unified_diff(&diff))
    }

    /// Return raw diff for a commit.
    ///
    /// # Errors
    /// Returns an error when git show fails.
    pub fn commit_diff(&self, commit_sha: &str) -> Result<String> {
        self.git(["show", "--format=", "--patch", "--find-renames", commit_sha])
    }

    fn git<const N: usize>(&self, args: [&str; N]) -> Result<String> {
        let output = Command::new("git")
            .args(args)
            .current_dir(&self.path)
            .output()
            .context("failed to run git")?;
        if !output.status.success() {
            bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

/// Commit metadata captured for review sessions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitInfo {
    /// Full sha.
    pub sha: String,
    /// Short sha.
    pub short_sha: String,
    /// Subject.
    pub subject: String,
    /// Body.
    pub body: Option<String>,
    /// Author name.
    pub author_name: Option<String>,
    /// Author email.
    pub author_email: Option<String>,
    /// Author date.
    pub authored_at: Option<String>,
}

fn empty_to_none(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn parse_hunk_header(line: &str) -> Option<(i64, i64, i64, i64)> {
    let after_minus = line.strip_prefix("@@ -")?;
    let (old_part, rest) = after_minus.split_once(" +")?;
    let (new_part, _) = rest.split_once(" @@")?;
    let (old_start, old_lines) = parse_range(old_part)?;
    let (new_start, new_lines) = parse_range(new_part)?;
    Some((old_start, old_lines, new_start, new_lines))
}

fn parse_range(value: &str) -> Option<(i64, i64)> {
    if let Some((start, lines)) = value.split_once(',') {
        Some((start.parse().ok()?, lines.parse().ok()?))
    } else {
        Some((value.parse().ok()?, 1))
    }
}

/// Parse a unified diff into files, hunks, and line records.
#[must_use]
pub fn parse_unified_diff(diff: &str) -> Vec<DiffFile> {
    let mut files = Vec::new();
    let mut current_file: Option<DiffFile> = None;
    let mut current_hunk: Option<DiffHunk> = None;
    let mut old_line = 0_i64;
    let mut new_line = 0_i64;

    for line in diff.lines() {
        if line.starts_with("diff --git ") {
            if let Some(hunk) = current_hunk.take()
                && let Some(file) = current_file.as_mut()
            {
                file.hunks.push(hunk);
            }
            if let Some(file) = current_file.take() {
                files.push(file);
            }
            current_file = Some(DiffFile {
                path: line.to_string(),
                old_path: None,
                status: "modified".to_string(),
                additions: 0,
                deletions: 0,
                hunks: Vec::new(),
            });
            continue;
        }

        let Some(file) = current_file.as_mut() else {
            continue;
        };

        if let Some(path) = line.strip_prefix("+++ b/") {
            file.path = path.to_string();
            continue;
        }
        if let Some(path) = line.strip_prefix("--- a/") {
            file.old_path = Some(path.to_string());
            continue;
        }
        if let Some(status) = line.strip_prefix("new file mode ") {
            let _ = status;
            file.status = "added".to_string();
            continue;
        }
        if let Some(status) = line.strip_prefix("deleted file mode ") {
            let _ = status;
            file.status = "deleted".to_string();
            continue;
        }
        if let Some(path) = line.strip_prefix("rename from ") {
            file.old_path = Some(path.to_string());
            file.status = "renamed".to_string();
            continue;
        }
        if let Some(path) = line.strip_prefix("rename to ") {
            file.path = path.to_string();
            file.status = "renamed".to_string();
            continue;
        }

        if let Some((old_start, old_lines, new_start, new_lines)) = parse_hunk_header(line) {
            if let Some(hunk) = current_hunk.take() {
                file.hunks.push(hunk);
            }
            old_line = old_start;
            new_line = new_start;
            current_hunk = Some(DiffHunk {
                old_start,
                old_lines,
                new_start,
                new_lines,
                patch: format!("{line}\n"),
                lines: Vec::new(),
            });
            continue;
        }

        let Some(hunk) = current_hunk.as_mut() else {
            continue;
        };
        hunk.patch.push_str(line);
        hunk.patch.push('\n');

        let diff_line = if line.starts_with('+') {
            file.additions += 1;
            let record = DiffLine {
                old_line: None,
                new_line: Some(new_line),
                line_kind: "add".to_string(),
                content: line.to_string(),
            };
            new_line += 1;
            record
        } else if line.starts_with('-') {
            file.deletions += 1;
            let record = DiffLine {
                old_line: Some(old_line),
                new_line: None,
                line_kind: "del".to_string(),
                content: line.to_string(),
            };
            old_line += 1;
            record
        } else if line.starts_with(' ') {
            let record = DiffLine {
                old_line: Some(old_line),
                new_line: Some(new_line),
                line_kind: "ctx".to_string(),
                content: line.to_string(),
            };
            old_line += 1;
            new_line += 1;
            record
        } else {
            DiffLine {
                old_line: None,
                new_line: None,
                line_kind: "meta".to_string(),
                content: line.to_string(),
            }
        };
        hunk.lines.push(diff_line);
    }

    if let Some(hunk) = current_hunk.take()
        && let Some(file) = current_file.as_mut()
    {
        file.hunks.push(hunk);
    }
    if let Some(file) = current_file.take() {
        files.push(file);
    }

    files
}

#[cfg(test)]
mod tests {
    use super::parse_unified_diff;

    #[test]
    fn parses_added_deleted_and_context_lines() {
        let diff = "\
diff --git a/a.txt b/a.txt
--- a/a.txt
+++ b/a.txt
@@ -1,2 +1,2 @@
 one
-two
+three
";
        let files = parse_unified_diff(diff);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "a.txt");
        assert_eq!(files[0].additions, 1);
        assert_eq!(files[0].deletions, 1);
        assert_eq!(files[0].hunks[0].lines[0].line_kind, "ctx");
        assert_eq!(files[0].hunks[0].lines[1].old_line, Some(2));
        assert_eq!(files[0].hunks[0].lines[2].new_line, Some(2));
    }
}
