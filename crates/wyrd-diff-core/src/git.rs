//! Git command integration and unified diff parsing.

use anyhow::{Context, Result, bail};
use chrono::{DateTime, FixedOffset, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf, process::Command};

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
    /// Raw unified-diff text for this file, including the `diff --git` header
    /// and every hunk. Suitable for reparse via [`parse_unified_diff`].
    #[serde(default)]
    pub patch_blob: String,
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

    fn open_repo(&self) -> Result<git2::Repository> {
        git2::Repository::open(&self.path)
            .with_context(|| format!("libgit2 open {}", self.path.display()))
    }

    /// Resolve a ref to a full sha.
    ///
    /// The pseudo-ref `WORKTREE` resolves to the current `HEAD` sha so that callers
    /// have a real anchor while diffing uncommitted tracked changes.
    ///
    /// # Errors
    /// Returns an error when libgit2 cannot resolve the ref.
    pub fn resolve_ref(&self, reference: &str) -> Result<String> {
        let target = if reference == "WORKTREE" {
            "HEAD"
        } else {
            reference
        };
        let repo = self.open_repo()?;
        let obj = repo
            .revparse_single(target)
            .with_context(|| format!("resolve ref {target}"))?;
        let oid = obj
            .peel_to_commit()
            .map(|c| c.id())
            .unwrap_or_else(|_| obj.id());
        Ok(oid.to_string())
    }

    /// Return commits in base..head order (oldest first).
    ///
    /// The pseudo-ref `WORKTREE` is rewritten to `HEAD` so that committed history up
    /// to the working tree is still captured alongside the uncommitted diff.
    ///
    /// # Errors
    /// Returns an error when libgit2 cannot walk the revision range.
    pub fn commits(&self, base: &str, head: &str) -> Result<Vec<CommitInfo>> {
        let head = if head == "WORKTREE" { "HEAD" } else { head };
        let repo = self.open_repo()?;
        let base_oid = repo
            .revparse_single(base)
            .with_context(|| format!("resolve base ref {base}"))?
            .peel_to_commit()?
            .id();
        let head_oid = repo
            .revparse_single(head)
            .with_context(|| format!("resolve head ref {head}"))?
            .peel_to_commit()?
            .id();
        let mut walk = repo.revwalk()?;
        walk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::REVERSE)?;
        walk.push(head_oid)?;
        walk.hide(base_oid)?;

        let mut out = Vec::new();
        for oid in walk {
            let oid = oid?;
            let commit = repo.find_commit(oid)?;
            let sha = oid.to_string();
            let short_sha = sha.chars().take(7).collect::<String>();
            let summary = commit.summary().unwrap_or("").to_string();
            let body = commit
                .message()
                .and_then(|m| m.split_once("\n\n").map(|(_, b)| b.trim_end().to_string()))
                .filter(|s| !s.is_empty());
            let author = commit.author();
            let author_name =
                Some(author.name().unwrap_or("").to_string()).filter(|s| !s.is_empty());
            let author_email =
                Some(author.email().unwrap_or("").to_string()).filter(|s| !s.is_empty());
            let when = author.when();
            let authored_at = format_git_time(when.seconds(), when.offset_minutes());
            out.push(CommitInfo {
                sha,
                short_sha,
                subject: summary,
                body,
                author_name,
                author_email,
                authored_at,
            });
        }
        Ok(out)
    }

    /// Return parsed diff files for base..head.
    ///
    /// When `head` is the pseudo-ref `WORKTREE`, the diff includes staged and unstaged
    /// changes to tracked files (`git diff <base>`) plus synthesized "new file"
    /// diffs for every untracked path reported by
    /// `git ls-files --others --exclude-standard`. Files matching `.gitignore` are
    /// skipped. Binary untracked files are emitted as a stub diff.
    ///
    /// # Errors
    /// Returns an error when libgit2 diff fails or untracked files cannot be read.
    pub fn diff_files(&self, base: &str, head: &str) -> Result<Vec<DiffFile>> {
        let diff_text = if head == "WORKTREE" {
            let mut combined = self.git(["diff", "--find-renames", "--unified=3", base])?;
            combined.push_str(&self.synthesize_untracked_diff()?);
            combined
        } else {
            self.libgit2_tree_diff(base, head)?
        };
        Ok(parse_unified_diff(&diff_text))
    }

    fn libgit2_tree_diff(&self, base: &str, head: &str) -> Result<String> {
        let repo = self.open_repo()?;
        let base_tree = repo
            .revparse_single(base)
            .with_context(|| format!("resolve base ref {base}"))?
            .peel_to_commit()?
            .tree()?;
        let head_tree = repo
            .revparse_single(head)
            .with_context(|| format!("resolve head ref {head}"))?
            .peel_to_commit()?
            .tree()?;
        let mut opts = git2::DiffOptions::new();
        opts.context_lines(3);
        let mut diff =
            repo.diff_tree_to_tree(Some(&base_tree), Some(&head_tree), Some(&mut opts))?;
        let mut find_opts = git2::DiffFindOptions::new();
        find_opts.renames(true);
        diff.find_similar(Some(&mut find_opts))?;

        let mut out = String::new();
        diff.print(git2::DiffFormat::Patch, |_delta, _hunk, line| {
            let origin = line.origin();
            if matches!(origin, '+' | '-' | ' ') {
                out.push(origin);
            }
            out.push_str(std::str::from_utf8(line.content()).unwrap_or(""));
            true
        })?;
        Ok(out)
    }

    fn synthesize_untracked_diff(&self) -> Result<String> {
        let listing = self.git(["ls-files", "--others", "--exclude-standard", "-z"])?;
        let mut out = String::new();
        for raw in listing.split('\0') {
            let rel_path = raw.trim_end_matches('\n');
            if rel_path.is_empty() {
                continue;
            }
            out.push_str(&self.render_untracked_file(rel_path)?);
        }
        Ok(out)
    }

    fn render_untracked_file(&self, rel_path: &str) -> Result<String> {
        let absolute = self.path.join(rel_path);
        let bytes = match fs::read(&absolute) {
            Ok(bytes) => bytes,
            Err(err) => {
                return Ok(format!(
                    "diff --git a/{rel_path} b/{rel_path}\nnew file mode 100644\n--- /dev/null\n+++ b/{rel_path}\n@@ -0,0 +1,1 @@\n+[wyrd-diff: failed to read untracked file: {err}]\n"
                ));
            }
        };
        let mut header = format!(
            "diff --git a/{rel_path} b/{rel_path}\nnew file mode 100644\n--- /dev/null\n+++ b/{rel_path}\n"
        );
        if bytes.contains(&0) {
            header.push_str("@@ -0,0 +1,1 @@\n+[wyrd-diff: binary untracked file omitted]\n");
            return Ok(header);
        }
        let text = String::from_utf8_lossy(&bytes);
        let trailing_newline = text.ends_with('\n');
        let lines: Vec<&str> = text.lines().collect();
        if lines.is_empty() {
            header.push_str("@@ -0,0 +0,0 @@\n");
            return Ok(header);
        }
        header.push_str(&format!("@@ -0,0 +1,{} @@\n", lines.len()));
        for line in &lines {
            header.push('+');
            header.push_str(line);
            header.push('\n');
        }
        if !trailing_newline {
            header.push_str("\\ No newline at end of file\n");
        }
        Ok(header)
    }

    /// List branches in the repository.
    ///
    /// Returns local branches first (refs/heads), then remote branches
    /// (refs/remotes) with the remote prefix stripped (e.g. `origin/main` →
    /// `origin/main`). `HEAD` symbolic refs are skipped.
    ///
    /// # Errors
    /// Returns an error when git fails to enumerate refs.
    pub fn branches(&self) -> Result<Vec<String>> {
        let raw = self.git([
            "for-each-ref",
            "--format=%(refname:short)",
            "refs/heads",
            "refs/remotes",
        ])?;
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for line in raw.lines() {
            let name = line.trim();
            if name.is_empty() || name.ends_with("/HEAD") {
                continue;
            }
            if seen.insert(name.to_string()) {
                out.push(name.to_string());
            }
        }
        Ok(out)
    }

    /// Return the repository's default branch, if discoverable.
    ///
    /// Tries `refs/remotes/origin/HEAD` symbolic ref first, then falls back to
    /// the current `HEAD` branch name.
    ///
    /// # Errors
    /// Returns an error when both lookups fail.
    pub fn default_branch(&self) -> Result<Option<String>> {
        if let Ok(value) = self.git(["symbolic-ref", "--short", "refs/remotes/origin/HEAD"]) {
            let name = value.trim();
            if let Some(stripped) = name.strip_prefix("origin/") {
                return Ok(Some(stripped.to_string()));
            }
            if !name.is_empty() {
                return Ok(Some(name.to_string()));
            }
        }
        if let Ok(value) = self.git(["rev-parse", "--abbrev-ref", "HEAD"]) {
            let name = value.trim();
            if !name.is_empty() && name != "HEAD" {
                return Ok(Some(name.to_string()));
            }
        }
        Ok(None)
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

/// Scanned repository candidate from a home directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoCandidate {
    /// Directory name.
    pub name: String,
    /// Absolute path.
    pub path: String,
}

/// Scan a directory for top-level git repositories.
///
/// Returns directories under `home` that contain a `.git` entry (file or
/// directory). Hidden entries (starting with `.`) are skipped. Results sorted
/// by name, case-insensitive.
///
/// # Errors
/// Returns an error when `home` is not readable.
pub fn scan_repos(home: &std::path::Path) -> Result<Vec<RepoCandidate>> {
    let entries = fs::read_dir(home)
        .with_context(|| format!("failed to read directory: {}", home.display()))?;
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let file_type = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        if !file_type.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        if !path.join(".git").exists() {
            continue;
        }
        out.push(RepoCandidate {
            name,
            path: path.to_string_lossy().to_string(),
        });
    }
    out.sort_by_key(|r| r.name.to_lowercase());
    Ok(out)
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

fn format_git_time(seconds: i64, offset_minutes: i32) -> Option<String> {
    let utc: DateTime<Utc> = DateTime::from_timestamp(seconds, 0)?;
    let tz = FixedOffset::east_opt(offset_minutes * 60)?;
    Some(
        utc.with_timezone(&tz)
            .to_rfc3339_opts(SecondsFormat::Secs, false),
    )
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
            let mut blob = String::with_capacity(line.len() + 1);
            blob.push_str(line);
            blob.push('\n');
            current_file = Some(DiffFile {
                path: line.to_string(),
                old_path: None,
                status: "modified".to_string(),
                additions: 0,
                deletions: 0,
                patch_blob: blob,
                hunks: Vec::new(),
            });
            continue;
        }

        let Some(file) = current_file.as_mut() else {
            continue;
        };
        file.patch_blob.push_str(line);
        file.patch_blob.push('\n');

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
    use super::{GitRepo, parse_unified_diff};
    use std::fs;
    use std::process::Command;
    use tempfile::TempDir;

    fn run(cwd: &std::path::Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    }

    #[test]
    fn worktree_diff_includes_untracked_and_staged_changes() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path();
        run(repo, &["init", "--initial-branch=main"]);
        run(repo, &["config", "user.email", "t@e"]);
        run(repo, &["config", "user.name", "t"]);
        fs::write(repo.join("kept.txt"), "one\n").unwrap();
        run(repo, &["add", "kept.txt"]);
        run(repo, &["commit", "-m", "base"]);

        fs::write(repo.join("kept.txt"), "one\ntwo\n").unwrap();
        run(repo, &["add", "kept.txt"]);
        fs::write(repo.join("kept.txt"), "one\ntwo\nthree\n").unwrap();
        fs::write(repo.join("new_file.txt"), "alpha\nbeta\n").unwrap();

        let git = GitRepo::open(repo).unwrap();
        let files = git.diff_files("main", "WORKTREE").unwrap();
        let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        assert!(paths.contains(&"kept.txt"), "got {paths:?}");
        assert!(paths.contains(&"new_file.txt"), "got {paths:?}");
        let new_file = files.iter().find(|f| f.path == "new_file.txt").unwrap();
        assert_eq!(new_file.status, "added");
        assert_eq!(new_file.additions, 2);
        let kept = files.iter().find(|f| f.path == "kept.txt").unwrap();
        assert!(kept.additions >= 2);
    }

    #[test]
    fn worktree_diff_skips_gitignored_files() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path();
        run(repo, &["init", "--initial-branch=main"]);
        run(repo, &["config", "user.email", "t@e"]);
        run(repo, &["config", "user.name", "t"]);
        fs::write(repo.join(".gitignore"), "secret.txt\n").unwrap();
        fs::write(repo.join("kept.txt"), "x\n").unwrap();
        run(repo, &["add", "."]);
        run(repo, &["commit", "-m", "base"]);

        fs::write(repo.join("secret.txt"), "top secret\n").unwrap();
        fs::write(repo.join("show.txt"), "visible\n").unwrap();

        let git = GitRepo::open(repo).unwrap();
        let files = git.diff_files("main", "WORKTREE").unwrap();
        let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        assert!(paths.contains(&"show.txt"), "got {paths:?}");
        assert!(!paths.contains(&"secret.txt"), "secret leaked: {paths:?}");
    }

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
