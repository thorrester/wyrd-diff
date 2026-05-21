//! SQLite persistence.

use crate::{
    ActiveReviewSessionRecord, FeedbackBatchRecord, FeedbackBatchThreadRecord, FileRecord,
    FixImportRecord, GitRepo, RepoRecord, ReviewDiffLine, ReviewFileDiff, ReviewHunk,
    ReviewSessionRecord, ReviewThreadRecord, SourceContext, ThreadMessageRecord,
    export::{AgentContext, TrajectoryRecord},
};
use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};
use uuid::Uuid;

const INIT_SQL: &str = include_str!("../../../migrations/001_init.sql");

/// SQLite database handle.
#[derive(Debug, Clone)]
pub struct Database {
    path: PathBuf,
}

/// Repository creation payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewRepo {
    /// Display name.
    pub name: String,
    /// Absolute repository path.
    pub path: String,
}

/// Review-session creation payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewReviewSession {
    /// Repository id.
    pub repo_id: String,
    /// Display title.
    pub title: String,
    /// Base ref.
    pub base_ref: String,
    /// Head ref.
    pub head_ref: String,
}

/// Line-comment creation payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewComment {
    /// Review session id.
    #[serde(default)]
    pub session_id: String,
    /// File path.
    pub file_path: String,
    /// Diff line id.
    pub diff_line_id: Option<String>,
    /// Old-side line number.
    pub old_line: Option<i64>,
    /// New-side line number.
    pub new_line: Option<i64>,
    /// Old-side start line for a selected range.
    #[serde(default)]
    pub range_start_old_line: Option<i64>,
    /// New-side start line for a selected range.
    #[serde(default)]
    pub range_start_new_line: Option<i64>,
    /// Old-side end line for a selected range.
    #[serde(default)]
    pub range_end_old_line: Option<i64>,
    /// New-side end line for a selected range.
    #[serde(default)]
    pub range_end_new_line: Option<i64>,
    /// Selected diff text for a range comment.
    #[serde(default)]
    pub selected_text: Option<String>,
    /// Body.
    pub body: String,
    /// Status.
    pub status: Option<String>,
    /// Visibility.
    pub visibility: Option<String>,
}

/// Inline thread creation payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewReviewThread {
    /// Review session id.
    #[serde(default)]
    pub session_id: String,
    /// File path.
    pub file_path: String,
    /// Anchor diff line id.
    pub anchor_diff_line_id: Option<String>,
    /// Old-side anchor line number.
    pub old_line: Option<i64>,
    /// New-side anchor line number.
    pub new_line: Option<i64>,
    /// Old-side start line for a selected range.
    #[serde(default)]
    pub range_start_old_line: Option<i64>,
    /// New-side start line for a selected range.
    #[serde(default)]
    pub range_start_new_line: Option<i64>,
    /// Old-side end line for a selected range.
    #[serde(default)]
    pub range_end_old_line: Option<i64>,
    /// New-side end line for a selected range.
    #[serde(default)]
    pub range_end_new_line: Option<i64>,
    /// Selected diff text for the thread.
    #[serde(default)]
    pub selected_text: Option<String>,
    /// Thread status.
    pub status: Option<String>,
    /// Visibility.
    pub visibility: Option<String>,
    /// First message type.
    pub message_type: String,
    /// First message body.
    pub body: String,
}

/// Thread message creation payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewThreadMessage {
    /// Parent thread id.
    #[serde(default)]
    pub thread_id: String,
    /// Author kind.
    #[serde(default)]
    pub author_kind: Option<String>,
    /// Author display name.
    #[serde(default)]
    pub author_name: Option<String>,
    /// Message type.
    pub message_type: String,
    /// Message body.
    pub body: String,
    /// Message status.
    pub status: Option<String>,
    /// Visibility.
    pub visibility: Option<String>,
    /// Linked fix import id.
    #[serde(default)]
    pub fix_import_id: Option<String>,
}

/// Note creation payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewNote {
    /// Repository id.
    pub repo_id: Option<String>,
    /// Review session id.
    pub session_id: Option<String>,
    /// Title.
    pub title: String,
    /// Body.
    pub body: String,
    /// Note type.
    pub note_type: String,
    /// Status.
    pub status: Option<String>,
    /// Visibility.
    pub visibility: Option<String>,
    /// Source context visible when the note was recorded.
    #[serde(default)]
    pub source_context: Option<SourceContext>,
}

/// Decision creation payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewDecision {
    /// Repository id.
    pub repo_id: Option<String>,
    /// Review session id.
    pub session_id: Option<String>,
    /// Title.
    pub title: String,
    /// Context.
    pub context: String,
    /// Decision.
    pub decision: String,
    /// Rationale.
    pub rationale: String,
    /// Alternatives.
    pub alternatives: Option<String>,
    /// Consequences.
    pub consequences: Option<String>,
    /// Status.
    pub status: Option<String>,
    /// Visibility.
    pub visibility: Option<String>,
    /// Source context visible when the decision was recorded.
    #[serde(default)]
    pub source_context: Option<SourceContext>,
}

/// Fix import payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewFixImport {
    /// Review session id.
    #[serde(default)]
    pub session_id: String,
    /// Commit sha.
    pub commit_sha: String,
    /// Agent name or runtime that produced the fix.
    pub agent_name: Option<String>,
    /// Agent response or completion transcript.
    pub response_text: Option<String>,
    /// Test results JSON.
    pub tests_json: Option<String>,
    /// Whether the fix was accepted.
    pub accepted: bool,
    /// Optional thread id to append the agent response to.
    #[serde(default)]
    pub thread_id: Option<String>,
}

impl Database {
    /// Create a database wrapper.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Database path.
    #[must_use]
    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    /// Open a SQLite connection.
    ///
    /// # Errors
    /// Returns an error when SQLite cannot open the database.
    pub fn connect(&self) -> Result<Connection> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let conn = Connection::open(&self.path)
            .with_context(|| format!("failed to open {}", self.path.display()))?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        Ok(conn)
    }

    /// Apply migrations.
    ///
    /// # Errors
    /// Returns an error when migration SQL fails.
    pub fn migrate(&self) -> Result<()> {
        let mut conn = self.connect()?;
        let tx = conn.transaction()?;
        tx.execute_batch(INIT_SQL)?;
        tx.execute(
            "insert or ignore into schema_migrations(version, applied_at) values(1, ?1)",
            [now()],
        )?;
        tx.commit()?;
        ensure_source_context_columns(&conn)?;
        ensure_feedback_columns(&conn)?;
        Ok(())
    }

    /// Delete and recreate the database.
    ///
    /// # Errors
    /// Returns an error when reset or migration fails.
    pub fn reset(&self) -> Result<()> {
        if self.path.exists() {
            fs::remove_file(&self.path)?;
        }
        self.migrate()
    }

    /// Create or update a repository record.
    ///
    /// # Errors
    /// Returns an error when the path is not a git repository or SQLite fails.
    pub fn upsert_repo(&self, input: NewRepo) -> Result<RepoRecord> {
        let repo = GitRepo::open(&input.path)?;
        let path = repo.path().canonicalize()?.display().to_string();
        let conn = self.connect()?;
        let timestamp = now();
        let existing: Option<String> = conn
            .query_row("select id from repos where path = ?1", [&path], |row| {
                row.get(0)
            })
            .optional()?;
        let id = existing.unwrap_or_else(new_id);
        conn.execute(
            "insert into repos(id, name, path, created_at, updated_at)
             values(?1, ?2, ?3, ?4, ?4)
             on conflict(path) do update set name = excluded.name, updated_at = excluded.updated_at",
            params![id, input.name, path, timestamp],
        )?;
        self.repo(&id)?.context("repo was not persisted")
    }

    /// Fetch all repositories.
    ///
    /// # Errors
    /// Returns an error when SQLite fails.
    pub fn repos(&self) -> Result<Vec<RepoRecord>> {
        let conn = self.connect()?;
        let mut stmt =
            conn.prepare("select id, name, path, created_at, updated_at from repos order by name")?;
        let rows = stmt.query_map([], map_repo)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    /// Fetch one repository.
    ///
    /// # Errors
    /// Returns an error when SQLite fails.
    pub fn repo(&self, id: &str) -> Result<Option<RepoRecord>> {
        let conn = self.connect()?;
        conn.query_row(
            "select id, name, path, created_at, updated_at from repos where id = ?1",
            [id],
            map_repo,
        )
        .optional()
        .map_err(Into::into)
    }

    /// Create a review session and snapshot commits/diff lines.
    ///
    /// # Errors
    /// Returns an error when git or SQLite fails.
    pub fn create_review_session(&self, input: NewReviewSession) -> Result<ReviewSessionRecord> {
        let repo = self
            .repo(&input.repo_id)?
            .with_context(|| format!("repo not found: {}", input.repo_id))?;
        let git = GitRepo::open(&repo.path)?;
        let base_sha = git.resolve_ref(&input.base_ref)?;
        let head_sha = git.resolve_ref(&input.head_ref)?;
        let commits = git.commits(&input.base_ref, &input.head_ref)?;
        let files = git.diff_files(&input.base_ref, &input.head_ref)?;

        let mut conn = self.connect()?;
        let tx = conn.transaction()?;
        let id = new_id();
        let timestamp = now();
        tx.execute(
            "insert into review_sessions(id, repo_id, title, base_ref, head_ref, base_sha, head_sha, status, created_at, updated_at)
             values(?1, ?2, ?3, ?4, ?5, ?6, ?7, 'open', ?8, ?8)",
            params![
                id,
                input.repo_id,
                input.title,
                input.base_ref,
                input.head_ref,
                base_sha,
                head_sha,
                timestamp
            ],
        )?;

        for commit in commits {
            tx.execute(
                "insert into commits(id, session_id, sha, short_sha, subject, body, author_name, author_email, authored_at)
                 values(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    new_id(),
                    id,
                    commit.sha,
                    commit.short_sha,
                    commit.subject,
                    commit.body,
                    commit.author_name,
                    commit.author_email,
                    commit.authored_at
                ],
            )?;
        }

        for file in files {
            let file_id = new_id();
            tx.execute(
                "insert into files(id, session_id, path, old_path, status, additions, deletions, review_state)
                 values(?1, ?2, ?3, ?4, ?5, ?6, ?7, 'unreviewed')",
                params![
                    file_id,
                    id,
                    file.path,
                    file.old_path,
                    file.status,
                    file.additions,
                    file.deletions
                ],
            )?;

            for hunk in file.hunks {
                let hunk_id = new_id();
                tx.execute(
                    "insert into hunks(id, file_id, old_start, old_lines, new_start, new_lines, patch)
                     values(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        hunk_id,
                        file_id,
                        hunk.old_start,
                        hunk.old_lines,
                        hunk.new_start,
                        hunk.new_lines,
                        hunk.patch
                    ],
                )?;
                for line in hunk.lines {
                    tx.execute(
                        "insert into diff_lines(id, hunk_id, file_path, old_line, new_line, line_kind, content)
                         values(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                        params![
                            new_id(),
                            hunk_id,
                            file.path,
                            line.old_line,
                            line.new_line,
                            line.line_kind,
                            line.content
                        ],
                    )?;
                }
            }
        }

        insert_event(
            &tx,
            Some(&repo.id),
            Some(&id),
            "review_session_created",
            &serde_json::json!({
                "base_ref": input.base_ref,
                "head_ref": input.head_ref,
                "base_sha": base_sha,
                "head_sha": head_sha
            }),
        )?;
        tx.execute(
            "insert into active_review_sessions(repo_id, session_id, start_sha, updated_at)
             values(?1, ?2, ?3, ?4)
             on conflict(repo_id) do update set session_id = excluded.session_id, start_sha = excluded.start_sha, updated_at = excluded.updated_at",
            params![repo.id, id, head_sha, timestamp],
        )?;
        tx.commit()?;
        self.review_session(&id)?
            .context("review session was not persisted")
    }

    /// Re-snapshot a review session against the current state of its refs.
    ///
    /// Replaces the session's commits, files, hunks, and diff lines with a fresh diff
    /// against `base_ref`/`head_ref`. Comments, threads, notes, and decisions are
    /// re-anchored to matching new diff lines by `(file_path, old_line, new_line)`; any
    /// anchor that no longer matches is set to `NULL` but the parent row is preserved.
    ///
    /// # Errors
    /// Returns an error when git or SQLite fails, or the session does not exist.
    pub fn refresh_review_session(&self, session_id: &str) -> Result<ReviewSessionRecord> {
        let session = self
            .review_session(session_id)?
            .with_context(|| format!("review session not found: {session_id}"))?;
        let repo = self
            .repo(&session.repo_id)?
            .with_context(|| format!("repo not found: {}", session.repo_id))?;
        let git = GitRepo::open(&repo.path)?;
        let base_sha = git.resolve_ref(&session.base_ref)?;
        let head_sha = git.resolve_ref(&session.head_ref)?;
        let commits = git.commits(&session.base_ref, &session.head_ref)?;
        let files = git.diff_files(&session.base_ref, &session.head_ref)?;

        let mut conn = self.connect()?;
        let tx = conn.transaction()?;
        let timestamp = now();

        tx.execute(
            "update comments set diff_line_id = null where session_id = ?1",
            params![session_id],
        )?;
        tx.execute(
            "update review_threads set anchor_diff_line_id = null where session_id = ?1",
            params![session_id],
        )?;
        tx.execute(
            "update notes set source_diff_line_id = null where session_id = ?1",
            params![session_id],
        )?;
        tx.execute(
            "update decisions set source_diff_line_id = null where session_id = ?1",
            params![session_id],
        )?;

        tx.execute(
            "delete from diff_lines
             where hunk_id in (
               select h.id from hunks h
               join files f on h.file_id = f.id
               where f.session_id = ?1
             )",
            params![session_id],
        )?;
        tx.execute(
            "delete from hunks
             where file_id in (select id from files where session_id = ?1)",
            params![session_id],
        )?;
        tx.execute(
            "delete from files where session_id = ?1",
            params![session_id],
        )?;
        tx.execute(
            "delete from commits where session_id = ?1",
            params![session_id],
        )?;

        for commit in commits {
            tx.execute(
                "insert into commits(id, session_id, sha, short_sha, subject, body, author_name, author_email, authored_at)
                 values(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    new_id(),
                    session_id,
                    commit.sha,
                    commit.short_sha,
                    commit.subject,
                    commit.body,
                    commit.author_name,
                    commit.author_email,
                    commit.authored_at
                ],
            )?;
        }

        for file in files {
            let file_id = new_id();
            tx.execute(
                "insert into files(id, session_id, path, old_path, status, additions, deletions, review_state)
                 values(?1, ?2, ?3, ?4, ?5, ?6, ?7, 'unreviewed')",
                params![
                    file_id,
                    session_id,
                    file.path,
                    file.old_path,
                    file.status,
                    file.additions,
                    file.deletions
                ],
            )?;
            for hunk in file.hunks {
                let hunk_id = new_id();
                tx.execute(
                    "insert into hunks(id, file_id, old_start, old_lines, new_start, new_lines, patch)
                     values(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        hunk_id,
                        file_id,
                        hunk.old_start,
                        hunk.old_lines,
                        hunk.new_start,
                        hunk.new_lines,
                        hunk.patch
                    ],
                )?;
                for line in hunk.lines {
                    tx.execute(
                        "insert into diff_lines(id, hunk_id, file_path, old_line, new_line, line_kind, content)
                         values(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                        params![
                            new_id(),
                            hunk_id,
                            file.path,
                            line.old_line,
                            line.new_line,
                            line.line_kind,
                            line.content
                        ],
                    )?;
                }
            }
        }

        let reanchor_comments = "update comments set diff_line_id = (
              select dl.id from diff_lines dl
              join hunks h on dl.hunk_id = h.id
              join files f on h.file_id = f.id
              where f.session_id = ?1
                and dl.file_path = comments.file_path
                and dl.old_line is comments.old_line
                and dl.new_line is comments.new_line
              limit 1
            )
            where session_id = ?1 and (old_line is not null or new_line is not null)";
        tx.execute(reanchor_comments, params![session_id])?;

        let reanchor_threads = "update review_threads set anchor_diff_line_id = (
              select dl.id from diff_lines dl
              join hunks h on dl.hunk_id = h.id
              join files f on h.file_id = f.id
              where f.session_id = ?1
                and dl.file_path = review_threads.file_path
                and dl.old_line is review_threads.old_line
                and dl.new_line is review_threads.new_line
              limit 1
            )
            where session_id = ?1 and (old_line is not null or new_line is not null)";
        tx.execute(reanchor_threads, params![session_id])?;

        let reanchor_notes = "update notes set source_diff_line_id = (
              select dl.id from diff_lines dl
              join hunks h on dl.hunk_id = h.id
              join files f on h.file_id = f.id
              where f.session_id = ?1
                and dl.file_path = notes.source_file_path
                and dl.old_line is notes.source_old_line
                and dl.new_line is notes.source_new_line
              limit 1
            )
            where session_id = ?1 and (source_old_line is not null or source_new_line is not null)";
        tx.execute(reanchor_notes, params![session_id])?;

        let reanchor_decisions = "update decisions set source_diff_line_id = (
              select dl.id from diff_lines dl
              join hunks h on dl.hunk_id = h.id
              join files f on h.file_id = f.id
              where f.session_id = ?1
                and dl.file_path = decisions.source_file_path
                and dl.old_line is decisions.source_old_line
                and dl.new_line is decisions.source_new_line
              limit 1
            )
            where session_id = ?1 and (source_old_line is not null or source_new_line is not null)";
        tx.execute(reanchor_decisions, params![session_id])?;

        tx.execute(
            "update review_sessions set base_sha = ?1, head_sha = ?2, updated_at = ?3 where id = ?4",
            params![base_sha, head_sha, timestamp, session_id],
        )?;

        insert_event(
            &tx,
            Some(&repo.id),
            Some(session_id),
            "review_session_refreshed",
            &serde_json::json!({
                "base_ref": session.base_ref,
                "head_ref": session.head_ref,
                "base_sha": base_sha,
                "head_sha": head_sha,
                "previous_base_sha": session.base_sha,
                "previous_head_sha": session.head_sha
            }),
        )?;

        tx.commit()?;
        self.review_session(session_id)?
            .context("review session was not persisted")
    }

    /// Fetch one review session.
    ///
    /// # Errors
    /// Returns an error when SQLite fails.
    pub fn review_session(&self, id: &str) -> Result<Option<ReviewSessionRecord>> {
        let conn = self.connect()?;
        conn.query_row(
            "select id, repo_id, title, base_ref, head_ref, base_sha, head_sha, status, created_at, updated_at
             from review_sessions where id = ?1",
            [id],
            map_review_session,
        )
        .optional()
        .map_err(Into::into)
    }

    /// Fetch all review sessions.
    ///
    /// # Errors
    /// Returns an error when SQLite fails.
    pub fn review_sessions(&self) -> Result<Vec<ReviewSessionRecord>> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare(
            "select id, repo_id, title, base_ref, head_ref, base_sha, head_sha, status, created_at, updated_at
             from review_sessions order by created_at desc",
        )?;
        let rows = stmt.query_map([], map_review_session)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    /// Mark a review session as the active session for its repository.
    ///
    /// # Errors
    /// Returns an error when SQLite fails or the session does not exist.
    pub fn activate_review_session(&self, session_id: &str) -> Result<ActiveReviewSessionRecord> {
        let session = self
            .review_session(session_id)?
            .with_context(|| format!("session not found: {session_id}"))?;
        let repo = self
            .repo(&session.repo_id)?
            .with_context(|| format!("repo not found: {}", session.repo_id))?;
        let timestamp = now();
        let conn = self.connect()?;
        conn.execute(
            "insert into active_review_sessions(repo_id, session_id, start_sha, updated_at)
             values(?1, ?2, ?3, ?4)
             on conflict(repo_id) do update set session_id = excluded.session_id, start_sha = excluded.start_sha, updated_at = excluded.updated_at",
            params![session.repo_id, session.id, session.head_sha, timestamp],
        )?;
        Ok(ActiveReviewSessionRecord {
            repo,
            session,
            start_sha: self
                .review_session(session_id)?
                .context("session not found after activation")?
                .head_sha,
            updated_at: timestamp,
        })
    }

    /// Resolve the active review session for a local repository path.
    ///
    /// # Errors
    /// Returns an error when SQLite fails.
    pub fn active_review_session_for_repo_path(
        &self,
        path: impl Into<PathBuf>,
    ) -> Result<Option<ActiveReviewSessionRecord>> {
        let path = path.into().canonicalize()?.display().to_string();
        let conn = self.connect()?;
        conn.query_row(
            "select
                r.id, r.name, r.path, r.created_at, r.updated_at,
                rs.id, rs.repo_id, rs.title, rs.base_ref, rs.head_ref, rs.base_sha, rs.head_sha, rs.status, rs.created_at, rs.updated_at,
                ars.start_sha, ars.updated_at
             from repos r
             join active_review_sessions ars on ars.repo_id = r.id
             join review_sessions rs on rs.id = ars.session_id
             where r.path = ?1",
            [path],
            |row| {
                Ok(ActiveReviewSessionRecord {
                    repo: RepoRecord {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        path: row.get(2)?,
                        created_at: row.get(3)?,
                        updated_at: row.get(4)?,
                    },
                    session: ReviewSessionRecord {
                        id: row.get(5)?,
                        repo_id: row.get(6)?,
                        title: row.get(7)?,
                        base_ref: row.get(8)?,
                        head_ref: row.get(9)?,
                        base_sha: row.get(10)?,
                        head_sha: row.get(11)?,
                        status: row.get(12)?,
                        created_at: row.get(13)?,
                        updated_at: row.get(14)?,
                    },
                    start_sha: row.get(15)?,
                    updated_at: row.get(16)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
    }

    /// Fetch diff files, hunks, and lines for a review session.
    ///
    /// # Errors
    /// Returns an error when SQLite fails.
    pub fn review_diff(&self, session_id: &str) -> Result<Vec<ReviewFileDiff>> {
        let conn = self.connect()?;
        let mut file_stmt = conn.prepare(
            "select id, session_id, path, old_path, status, additions, deletions, review_state
             from files where session_id = ?1 order by path",
        )?;
        let file_rows = file_stmt.query_map([session_id], |row| {
            Ok(FileRecord {
                id: row.get(0)?,
                session_id: row.get(1)?,
                path: row.get(2)?,
                old_path: row.get(3)?,
                status: row.get(4)?,
                additions: row.get(5)?,
                deletions: row.get(6)?,
                review_state: row.get(7)?,
            })
        })?;
        let mut files = Vec::new();
        for file in file_rows {
            let file = file?;
            let mut hunk_stmt = conn.prepare(
                "select id, old_start, old_lines, new_start, new_lines, patch
                 from hunks where file_id = ?1 order by rowid",
            )?;
            let hunk_rows = hunk_stmt.query_map([&file.id], |row| {
                Ok(ReviewHunk {
                    id: row.get(0)?,
                    old_start: row.get(1)?,
                    old_lines: row.get(2)?,
                    new_start: row.get(3)?,
                    new_lines: row.get(4)?,
                    patch: row.get(5)?,
                    lines: Vec::new(),
                })
            })?;
            let mut hunks = Vec::new();
            for hunk in hunk_rows {
                let mut hunk = hunk?;
                let mut line_stmt = conn.prepare(
                    "select id, file_path, old_line, new_line, line_kind, content
                     from diff_lines where hunk_id = ?1 order by rowid",
                )?;
                let line_rows = line_stmt.query_map([&hunk.id], |row| {
                    Ok(ReviewDiffLine {
                        id: row.get(0)?,
                        file_path: row.get(1)?,
                        old_line: row.get(2)?,
                        new_line: row.get(3)?,
                        line_kind: row.get(4)?,
                        content: row.get(5)?,
                    })
                })?;
                hunk.lines = line_rows.collect::<rusqlite::Result<Vec<_>>>()?;
                hunks.push(hunk);
            }
            files.push(ReviewFileDiff { file, hunks });
        }
        Ok(files)
    }

    /// Add a review comment.
    ///
    /// # Errors
    /// Returns an error when SQLite fails.
    pub fn add_comment(&self, input: NewComment) -> Result<String> {
        let conn = self.connect()?;
        let id = new_id();
        let timestamp = now();
        let status = input.status.unwrap_or_else(|| "open".to_string());
        let visibility = input.visibility.unwrap_or_else(|| "agent".to_string());
        conn.execute(
            "insert into comments(id, session_id, file_path, diff_line_id, old_line, new_line, range_start_old_line, range_start_new_line, range_end_old_line, range_end_new_line, selected_text, body, status, visibility, created_at, updated_at)
             values(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?15)",
            params![
                id,
                input.session_id,
                input.file_path,
                input.diff_line_id,
                input.old_line,
                input.new_line,
                input.range_start_old_line,
                input.range_start_new_line,
                input.range_end_old_line,
                input.range_end_new_line,
                input.selected_text,
                input.body,
                status,
                visibility,
                timestamp
            ],
        )?;
        Ok(id)
    }

    /// Add an inline review thread and its first message.
    ///
    /// # Errors
    /// Returns an error when SQLite fails.
    pub fn add_review_thread(&self, input: NewReviewThread) -> Result<ReviewThreadRecord> {
        let mut conn = self.connect()?;
        let tx = conn.transaction()?;
        let id = new_id();
        let timestamp = now();
        let status = input.status.unwrap_or_else(|| "open".to_string());
        let visibility = input.visibility.unwrap_or_else(|| "agent".to_string());
        tx.execute(
            "insert into review_threads(id, session_id, file_path, anchor_diff_line_id, old_line, new_line, range_start_old_line, range_start_new_line, range_end_old_line, range_end_new_line, selected_text, status, visibility, created_at, updated_at)
             values(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?14)",
            params![
                id,
                input.session_id,
                input.file_path,
                input.anchor_diff_line_id,
                input.old_line,
                input.new_line,
                input.range_start_old_line,
                input.range_start_new_line,
                input.range_end_old_line,
                input.range_end_new_line,
                input.selected_text,
                status,
                visibility,
                timestamp
            ],
        )?;
        insert_thread_message(
            &tx,
            NewThreadMessage {
                thread_id: id.clone(),
                author_kind: Some("human".to_string()),
                author_name: None,
                message_type: input.message_type,
                body: input.body,
                status: Some("open".to_string()),
                visibility: Some(visibility),
                fix_import_id: None,
            },
        )?;
        tx.commit()?;
        self.review_thread(&id)?
            .with_context(|| format!("review thread was not persisted: {id}"))
    }

    /// Delete an inline review thread and all of its messages.
    ///
    /// # Errors
    /// Returns an error when SQLite fails.
    pub fn delete_review_thread(&self, id: &str) -> Result<bool> {
        let mut conn = self.connect()?;
        let tx = conn.transaction()?;
        tx.execute("delete from thread_messages where thread_id = ?1", [id])?;
        let removed = tx.execute("delete from review_threads where id = ?1", [id])?;
        tx.commit()?;
        Ok(removed > 0)
    }

    /// Add a message to an inline review thread.
    ///
    /// # Errors
    /// Returns an error when SQLite fails.
    pub fn add_thread_message(&self, input: NewThreadMessage) -> Result<ThreadMessageRecord> {
        let mut conn = self.connect()?;
        let tx = conn.transaction()?;
        let id = insert_thread_message(&tx, input.clone())?;
        tx.execute(
            "update review_threads set updated_at = ?1 where id = ?2",
            params![now(), input.thread_id],
        )?;
        tx.commit()?;
        self.thread_message(&id)?
            .with_context(|| format!("thread message was not persisted: {id}"))
    }

    /// Fetch one inline review thread.
    ///
    /// # Errors
    /// Returns an error when SQLite fails.
    pub fn review_thread(&self, id: &str) -> Result<Option<ReviewThreadRecord>> {
        let conn = self.connect()?;
        let mut thread = conn
            .query_row(
                "select id, session_id, file_path, anchor_diff_line_id, old_line, new_line, range_start_old_line, range_start_new_line, range_end_old_line, range_end_new_line, selected_text, status, visibility, created_at, updated_at, last_delivered_message_id
                 from review_threads where id = ?1",
                [id],
                map_review_thread,
            )
            .optional()?;
        if let Some(thread) = thread.as_mut() {
            thread.messages = thread_messages_for_conn(&conn, &thread.id)?;
        }
        Ok(thread)
    }

    /// Fetch inline review threads for a session.
    ///
    /// # Errors
    /// Returns an error when SQLite fails.
    pub fn review_threads(&self, session_id: &str) -> Result<Vec<ReviewThreadRecord>> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare(
            "select id, session_id, file_path, anchor_diff_line_id, old_line, new_line, range_start_old_line, range_start_new_line, range_end_old_line, range_end_new_line, selected_text, status, visibility, created_at, updated_at, last_delivered_message_id
             from review_threads where session_id = ?1 order by created_at",
        )?;
        let rows = stmt.query_map([session_id], map_review_thread)?;
        let mut threads = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        for thread in &mut threads {
            thread.messages = thread_messages_for_conn(&conn, &thread.id)?;
        }
        Ok(threads)
    }

    /// Fetch one thread message.
    ///
    /// # Errors
    /// Returns an error when SQLite fails.
    pub fn thread_message(&self, id: &str) -> Result<Option<ThreadMessageRecord>> {
        let conn = self.connect()?;
        conn.query_row(
            "select id, thread_id, author_kind, author_name, message_type, body, status, visibility, fix_import_id, created_at, updated_at
             from thread_messages where id = ?1",
            [id],
            map_thread_message,
        )
        .optional()
        .map_err(Into::into)
    }

    /// Add a note.
    ///
    /// # Errors
    /// Returns an error when SQLite fails.
    pub fn add_note(&self, input: NewNote) -> Result<String> {
        let conn = self.connect()?;
        let id = new_id();
        let timestamp = now();
        let source = input.source_context.as_ref();
        conn.execute(
            "insert into notes(id, repo_id, session_id, title, body, note_type, status, visibility, source_file_path, source_diff_line_id, source_old_line, source_new_line, source_line_kind, source_content, created_at, updated_at)
             values(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?15)",
            params![
                id,
                input.repo_id,
                input.session_id,
                input.title,
                input.body,
                input.note_type,
                input.status.unwrap_or_else(|| "draft".to_string()),
                input.visibility.unwrap_or_else(|| "agent".to_string()),
                source.map(|value| value.file_path.as_str()),
                source.and_then(|value| value.diff_line_id.as_deref()),
                source.and_then(|value| value.old_line),
                source.and_then(|value| value.new_line),
                source.and_then(|value| value.line_kind.as_deref()),
                source.and_then(|value| value.content.as_deref()),
                timestamp
            ],
        )?;
        Ok(id)
    }

    /// Add a decision.
    ///
    /// # Errors
    /// Returns an error when SQLite fails.
    pub fn add_decision(&self, input: NewDecision) -> Result<String> {
        let conn = self.connect()?;
        let id = new_id();
        let timestamp = now();
        let source = input.source_context.as_ref();
        conn.execute(
            "insert into decisions(id, repo_id, session_id, title, context, decision, rationale, alternatives, consequences, status, visibility, source_file_path, source_diff_line_id, source_old_line, source_new_line, source_line_kind, source_content, created_at, updated_at)
             values(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?18)",
            params![
                id,
                input.repo_id,
                input.session_id,
                input.title,
                input.context,
                input.decision,
                input.rationale,
                input.alternatives,
                input.consequences,
                input.status.unwrap_or_else(|| "accepted".to_string()),
                input.visibility.unwrap_or_else(|| "agent".to_string()),
                source.map(|value| value.file_path.as_str()),
                source.and_then(|value| value.diff_line_id.as_deref()),
                source.and_then(|value| value.old_line),
                source.and_then(|value| value.new_line),
                source.and_then(|value| value.line_kind.as_deref()),
                source.and_then(|value| value.content.as_deref()),
                timestamp
            ],
        )?;
        Ok(id)
    }

    /// Import a fix commit.
    ///
    /// # Errors
    /// Returns an error when git or SQLite fails.
    pub fn import_fix(&self, input: NewFixImport) -> Result<String> {
        let session = self
            .review_session(&input.session_id)?
            .with_context(|| format!("session not found: {}", input.session_id))?;
        let repo = self
            .repo(&session.repo_id)?
            .with_context(|| format!("repo not found: {}", session.repo_id))?;
        let git = GitRepo::open(repo.path)?;
        let diff = git.commit_diff(&input.commit_sha)?;
        let mut conn = self.connect()?;
        let tx = conn.transaction()?;
        let id = new_id();
        let response_text = input.response_text.clone();
        let agent_name = input.agent_name.clone();
        let thread_id = input.thread_id.clone();
        tx.execute(
            "insert into fix_imports(id, session_id, commit_sha, diff_patch, agent_name, response_text, tests_json, accepted, created_at)
             values(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                id,
                input.session_id,
                input.commit_sha,
                diff,
                input.agent_name,
                input.response_text,
                input.tests_json,
                i64::from(input.accepted),
                now()
            ],
        )?;
        if let Some(response_text) = response_text.filter(|value| !value.trim().is_empty()) {
            if let Some(thread_id) = thread_id {
                insert_thread_message(
                    &tx,
                    NewThreadMessage {
                        thread_id,
                        author_kind: Some("agent".to_string()),
                        author_name: agent_name,
                        message_type: "agent_response".to_string(),
                        body: response_text,
                        status: Some("open".to_string()),
                        visibility: Some("agent".to_string()),
                        fix_import_id: Some(id.clone()),
                    },
                )?;
            } else {
                let mut stmt = tx.prepare(
                    "select id from review_threads
                     where session_id = ?1 and status = 'open' and visibility = 'agent'
                     order by created_at",
                )?;
                let thread_ids = stmt
                    .query_map([&input.session_id], |row| row.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                drop(stmt);
                for thread_id in thread_ids {
                    insert_thread_message(
                        &tx,
                        NewThreadMessage {
                            thread_id,
                            author_kind: Some("agent".to_string()),
                            author_name: agent_name.clone(),
                            message_type: "agent_response".to_string(),
                            body: response_text.clone(),
                            status: Some("open".to_string()),
                            visibility: Some("agent".to_string()),
                            fix_import_id: Some(id.clone()),
                        },
                    )?;
                }
            }
        }
        tx.commit()?;
        Ok(id)
    }

    /// Check whether a fix commit has already been imported for a session.
    ///
    /// # Errors
    /// Returns an error when SQLite fails.
    pub fn fix_import_exists(&self, session_id: &str, commit_sha: &str) -> Result<bool> {
        let conn = self.connect()?;
        let count: i64 = conn.query_row(
            "select count(*) from fix_imports where session_id = ?1 and commit_sha = ?2",
            params![session_id, commit_sha],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    /// List imported fix records for a review session.
    ///
    /// # Errors
    /// Returns an error when SQLite fails.
    pub fn fix_imports(&self, session_id: &str) -> Result<Vec<FixImportRecord>> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare(
            "select id, session_id, commit_sha, diff_patch, agent_name, response_text, tests_json, accepted, created_at
             from fix_imports where session_id = ?1 order by created_at",
        )?;
        let rows = stmt.query_map([session_id], |row| {
            Ok(FixImportRecord {
                id: row.get(0)?,
                session_id: row.get(1)?,
                commit_sha: row.get(2)?,
                diff_patch: row.get(3)?,
                agent_name: row.get(4)?,
                response_text: row.get(5)?,
                tests_json: row.get(6)?,
                accepted: row.get::<_, i64>(7)? == 1,
                created_at: row.get(8)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    /// Build agent context for a review session.
    ///
    /// # Errors
    /// Returns an error when SQLite fails.
    pub fn agent_context(&self, session_id: &str) -> Result<AgentContext> {
        crate::export::agent_context(self, session_id)
    }

    /// Export trajectory records.
    ///
    /// # Errors
    /// Returns an error when SQLite fails.
    pub fn trajectory_records(&self, repo_id: Option<&str>) -> Result<Vec<TrajectoryRecord>> {
        crate::export::trajectory_records(self, repo_id)
    }

    /// Set a thread's status.
    ///
    /// # Errors
    /// Returns an error when SQLite fails.
    pub fn set_thread_status(&self, id: &str, status: &str) -> Result<bool> {
        let conn = self.connect()?;
        let updated = conn.execute(
            "update review_threads set status = ?1, updated_at = ?2 where id = ?3",
            params![status, now(), id],
        )?;
        Ok(updated > 0)
    }

    /// Mark a thread as resolved.
    ///
    /// # Errors
    /// Returns an error when SQLite fails.
    pub fn resolve_thread(&self, id: &str) -> Result<bool> {
        self.set_thread_status(id, "resolved")
    }

    /// Reopen a previously resolved thread.
    ///
    /// # Errors
    /// Returns an error when SQLite fails.
    pub fn reopen_thread(&self, id: &str) -> Result<bool> {
        self.set_thread_status(id, "open")
    }

    /// Build a feedback batch for the session and persist it as pending.
    ///
    /// Selects open threads that have either never been delivered or have new
    /// human replies past their watermark. Renders a markdown payload, stores
    /// it, and supersedes any prior pending batch for the same session.
    ///
    /// # Errors
    /// Returns an error when SQLite fails or the session does not exist.
    pub fn create_feedback_batch(
        &self,
        session_id: &str,
        agent_session_id: Option<String>,
    ) -> Result<FeedbackBatchRecord> {
        let session = self
            .review_session(session_id)?
            .with_context(|| format!("session not found: {session_id}"))?;
        let threads = self.review_threads(session_id)?;
        let watermarks = thread_watermarks(&self.connect()?, session_id)?;
        let mut snapshots = Vec::new();
        for thread in &threads {
            if thread.status != "open" {
                continue;
            }
            let watermark = watermarks.get(&thread.id).cloned().flatten();
            let (delivery_kind, included) =
                select_messages_for_delivery(thread, watermark.as_deref());
            if included.is_empty() {
                continue;
            }
            snapshots.push((thread.clone(), delivery_kind, included));
        }
        let payload = render_feedback_markdown(&session, &snapshots);
        let mut conn = self.connect()?;
        let tx = conn.transaction()?;
        tx.execute(
            "update feedback_batches set status = 'superseded'
             where session_id = ?1 and status = 'pending'",
            [session_id],
        )?;
        let batch_id = new_id();
        let created_at = now();
        tx.execute(
            "insert into feedback_batches(id, session_id, agent_session_id, status, payload, thread_count, created_at, delivered_at)
             values(?1, ?2, ?3, 'pending', ?4, ?5, ?6, null)",
            params![
                batch_id,
                session_id,
                agent_session_id,
                payload,
                i64::try_from(snapshots.len()).unwrap_or(i64::MAX),
                created_at
            ],
        )?;
        let mut thread_rows = Vec::with_capacity(snapshots.len());
        for (thread, delivery_kind, messages) in &snapshots {
            let ids: Vec<String> = messages.iter().map(|message| message.id.clone()).collect();
            let ids_json = serde_json::to_string(&ids)?;
            tx.execute(
                "insert into feedback_batch_threads(batch_id, thread_id, delivery_kind, message_ids)
                 values(?1, ?2, ?3, ?4)",
                params![batch_id, thread.id, delivery_kind, ids_json],
            )?;
            thread_rows.push(FeedbackBatchThreadRecord {
                batch_id: batch_id.clone(),
                thread_id: thread.id.clone(),
                delivery_kind: delivery_kind.clone(),
                message_ids: ids,
            });
        }
        tx.commit()?;
        Ok(FeedbackBatchRecord {
            id: batch_id,
            session_id: session_id.to_string(),
            agent_session_id,
            status: "pending".to_string(),
            payload,
            thread_count: i64::try_from(snapshots.len()).unwrap_or(i64::MAX),
            created_at,
            delivered_at: None,
            threads: thread_rows,
        })
    }

    /// List feedback batches for a session, newest first.
    ///
    /// # Errors
    /// Returns an error when SQLite fails.
    pub fn feedback_batches(&self, session_id: &str) -> Result<Vec<FeedbackBatchRecord>> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare(
            "select id, session_id, agent_session_id, status, payload, thread_count, created_at, delivered_at
             from feedback_batches where session_id = ?1 order by created_at desc",
        )?;
        let rows = stmt.query_map([session_id], map_feedback_batch)?;
        let mut batches = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        for batch in &mut batches {
            batch.threads = feedback_batch_threads_for_conn(&conn, &batch.id)?;
        }
        Ok(batches)
    }

    /// Fetch one feedback batch.
    ///
    /// # Errors
    /// Returns an error when SQLite fails.
    pub fn feedback_batch(&self, id: &str) -> Result<Option<FeedbackBatchRecord>> {
        let conn = self.connect()?;
        let mut batch = conn
            .query_row(
                "select id, session_id, agent_session_id, status, payload, thread_count, created_at, delivered_at
                 from feedback_batches where id = ?1",
                [id],
                map_feedback_batch,
            )
            .optional()?;
        if let Some(batch) = batch.as_mut() {
            batch.threads = feedback_batch_threads_for_conn(&conn, &batch.id)?;
        }
        Ok(batch)
    }

    /// Pull the newest pending feedback batch for a session, marking it
    /// delivered and advancing per-thread watermarks atomically.
    ///
    /// # Errors
    /// Returns an error when SQLite fails.
    pub fn claim_pending_feedback(
        &self,
        session_id: &str,
        agent_session_id: Option<&str>,
    ) -> Result<Option<FeedbackBatchRecord>> {
        let mut conn = self.connect()?;
        let tx = conn.transaction()?;
        let batch_id: Option<String> = tx
            .query_row(
                "select id from feedback_batches
                 where session_id = ?1 and status = 'pending'
                 order by created_at desc limit 1",
                [session_id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(batch_id) = batch_id else {
            return Ok(None);
        };
        let delivered_at = now();
        tx.execute(
            "update feedback_batches
             set status = 'delivered', delivered_at = ?1, agent_session_id = coalesce(?2, agent_session_id)
             where id = ?3",
            params![delivered_at, agent_session_id, batch_id],
        )?;
        let snapshot_threads = feedback_batch_threads_for_tx(&tx, &batch_id)?;
        for thread in &snapshot_threads {
            let Some(max_id) = thread.message_ids.last() else {
                continue;
            };
            tx.execute(
                "update review_threads
                 set last_delivered_message_id = ?1, updated_at = ?2
                 where id = ?3",
                params![max_id, delivered_at, thread.thread_id],
            )?;
        }
        tx.commit()?;
        self.feedback_batch(&batch_id)
    }

    /// Internal connection for export helpers.
    ///
    /// # Errors
    /// Returns an error when SQLite cannot open the database.
    pub fn export_connection(&self) -> Result<Connection> {
        self.connect()
    }
}

fn now() -> String {
    Utc::now().to_rfc3339()
}

fn new_id() -> String {
    Uuid::new_v4().to_string()
}

fn ensure_source_context_columns(conn: &Connection) -> Result<()> {
    for table in ["notes", "decisions"] {
        ensure_column(conn, table, "source_file_path", "text")?;
        ensure_column(conn, table, "source_diff_line_id", "text")?;
        ensure_column(conn, table, "source_old_line", "integer")?;
        ensure_column(conn, table, "source_new_line", "integer")?;
        ensure_column(conn, table, "source_line_kind", "text")?;
        ensure_column(conn, table, "source_content", "text")?;
    }
    ensure_column(conn, "fix_imports", "agent_name", "text")?;
    ensure_column(conn, "fix_imports", "response_text", "text")?;
    ensure_column(conn, "comments", "range_start_old_line", "integer")?;
    ensure_column(conn, "comments", "range_start_new_line", "integer")?;
    ensure_column(conn, "comments", "range_end_old_line", "integer")?;
    ensure_column(conn, "comments", "range_end_new_line", "integer")?;
    ensure_column(conn, "comments", "selected_text", "text")?;
    conn.execute(
        "create table if not exists active_review_sessions (
          repo_id text primary key references repos(id),
          session_id text not null references review_sessions(id),
          start_sha text not null,
          updated_at text not null
        )",
        [],
    )?;
    conn.execute(
        "create table if not exists review_threads (
          id text primary key,
          session_id text not null references review_sessions(id),
          file_path text not null,
          anchor_diff_line_id text references diff_lines(id),
          old_line integer,
          new_line integer,
          range_start_old_line integer,
          range_start_new_line integer,
          range_end_old_line integer,
          range_end_new_line integer,
          selected_text text,
          status text not null,
          visibility text not null,
          created_at text not null,
          updated_at text not null
        )",
        [],
    )?;
    conn.execute(
        "create table if not exists thread_messages (
          id text primary key,
          thread_id text not null references review_threads(id),
          author_kind text not null,
          author_name text,
          message_type text not null,
          body text not null,
          status text not null,
          visibility text not null,
          fix_import_id text references fix_imports(id),
          created_at text not null,
          updated_at text not null
        )",
        [],
    )?;
    conn.execute(
        "insert or ignore into schema_migrations(version, applied_at) values(2, ?1)",
        [now()],
    )?;
    Ok(())
}

fn ensure_feedback_columns(conn: &Connection) -> Result<()> {
    ensure_column(conn, "review_threads", "last_delivered_message_id", "text")?;
    ensure_column(conn, "review_sessions", "agent_session_id", "text")?;
    conn.execute(
        "create table if not exists feedback_batches (
          id text primary key,
          session_id text not null references review_sessions(id),
          agent_session_id text,
          status text not null,
          payload text not null,
          thread_count integer not null,
          created_at text not null,
          delivered_at text
        )",
        [],
    )?;
    conn.execute(
        "create table if not exists feedback_batch_threads (
          batch_id text not null references feedback_batches(id),
          thread_id text not null references review_threads(id),
          delivery_kind text not null,
          message_ids text not null,
          primary key (batch_id, thread_id)
        )",
        [],
    )?;
    conn.execute(
        "insert or ignore into schema_migrations(version, applied_at) values(3, ?1)",
        [now()],
    )?;
    Ok(())
}

fn ensure_column(
    conn: &Connection,
    table: &'static str,
    column: &'static str,
    definition: &'static str,
) -> Result<()> {
    let mut stmt = conn.prepare(&format!("pragma table_info({table})"))?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if !columns.iter().any(|existing| existing == column) {
        conn.execute(
            &format!("alter table {table} add column {column} {definition}"),
            [],
        )?;
    }
    Ok(())
}

fn map_repo(row: &rusqlite::Row<'_>) -> rusqlite::Result<RepoRecord> {
    Ok(RepoRecord {
        id: row.get(0)?,
        name: row.get(1)?,
        path: row.get(2)?,
        created_at: row.get(3)?,
        updated_at: row.get(4)?,
    })
}

fn map_review_session(row: &rusqlite::Row<'_>) -> rusqlite::Result<ReviewSessionRecord> {
    Ok(ReviewSessionRecord {
        id: row.get(0)?,
        repo_id: row.get(1)?,
        title: row.get(2)?,
        base_ref: row.get(3)?,
        head_ref: row.get(4)?,
        base_sha: row.get(5)?,
        head_sha: row.get(6)?,
        status: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
    })
}

fn map_review_thread(row: &rusqlite::Row<'_>) -> rusqlite::Result<ReviewThreadRecord> {
    Ok(ReviewThreadRecord {
        id: row.get(0)?,
        session_id: row.get(1)?,
        file_path: row.get(2)?,
        anchor_diff_line_id: row.get(3)?,
        old_line: row.get(4)?,
        new_line: row.get(5)?,
        range_start_old_line: row.get(6)?,
        range_start_new_line: row.get(7)?,
        range_end_old_line: row.get(8)?,
        range_end_new_line: row.get(9)?,
        selected_text: row.get(10)?,
        status: row.get(11)?,
        visibility: row.get(12)?,
        created_at: row.get(13)?,
        updated_at: row.get(14)?,
        last_delivered_message_id: row.get(15)?,
        messages: Vec::new(),
    })
}

fn map_thread_message(row: &rusqlite::Row<'_>) -> rusqlite::Result<ThreadMessageRecord> {
    Ok(ThreadMessageRecord {
        id: row.get(0)?,
        thread_id: row.get(1)?,
        author_kind: row.get(2)?,
        author_name: row.get(3)?,
        message_type: row.get(4)?,
        body: row.get(5)?,
        status: row.get(6)?,
        visibility: row.get(7)?,
        fix_import_id: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

fn thread_messages_for_conn(
    conn: &Connection,
    thread_id: &str,
) -> Result<Vec<ThreadMessageRecord>> {
    let mut stmt = conn.prepare(
        "select id, thread_id, author_kind, author_name, message_type, body, status, visibility, fix_import_id, created_at, updated_at
         from thread_messages where thread_id = ?1 order by created_at",
    )?;
    let rows = stmt.query_map([thread_id], map_thread_message)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

fn insert_thread_message(
    tx: &rusqlite::Transaction<'_>,
    input: NewThreadMessage,
) -> Result<String> {
    let id = new_id();
    let timestamp = now();
    tx.execute(
        "insert into thread_messages(id, thread_id, author_kind, author_name, message_type, body, status, visibility, fix_import_id, created_at, updated_at)
         values(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
        params![
            id,
            input.thread_id,
            input.author_kind.unwrap_or_else(|| "human".to_string()),
            input.author_name,
            input.message_type,
            input.body,
            input.status.unwrap_or_else(|| "open".to_string()),
            input.visibility.unwrap_or_else(|| "agent".to_string()),
            input.fix_import_id,
            timestamp
        ],
    )?;
    Ok(id)
}

fn insert_event(
    tx: &rusqlite::Transaction<'_>,
    repo_id: Option<&str>,
    session_id: Option<&str>,
    event_kind: &str,
    payload: &serde_json::Value,
) -> Result<()> {
    tx.execute(
        "insert into trajectory_events(id, repo_id, session_id, event_kind, payload_json, created_at)
         values(?1, ?2, ?3, ?4, ?5, ?6)",
        params![new_id(), repo_id, session_id, event_kind, payload.to_string(), now()],
    )?;
    Ok(())
}

fn map_feedback_batch(row: &rusqlite::Row<'_>) -> rusqlite::Result<FeedbackBatchRecord> {
    Ok(FeedbackBatchRecord {
        id: row.get(0)?,
        session_id: row.get(1)?,
        agent_session_id: row.get(2)?,
        status: row.get(3)?,
        payload: row.get(4)?,
        thread_count: row.get(5)?,
        created_at: row.get(6)?,
        delivered_at: row.get(7)?,
        threads: Vec::new(),
    })
}

fn feedback_batch_threads_for_conn(
    conn: &Connection,
    batch_id: &str,
) -> Result<Vec<FeedbackBatchThreadRecord>> {
    let mut stmt = conn.prepare(
        "select batch_id, thread_id, delivery_kind, message_ids
         from feedback_batch_threads where batch_id = ?1",
    )?;
    let rows = stmt
        .query_map([batch_id], map_feedback_batch_thread)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter().map(parse_feedback_batch_thread).collect()
}

fn feedback_batch_threads_for_tx(
    tx: &rusqlite::Transaction<'_>,
    batch_id: &str,
) -> Result<Vec<FeedbackBatchThreadRecord>> {
    let mut stmt = tx.prepare(
        "select batch_id, thread_id, delivery_kind, message_ids
         from feedback_batch_threads where batch_id = ?1",
    )?;
    let rows = stmt
        .query_map([batch_id], map_feedback_batch_thread)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter().map(parse_feedback_batch_thread).collect()
}

fn map_feedback_batch_thread(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<(String, String, String, String)> {
    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
}

fn parse_feedback_batch_thread(
    raw: (String, String, String, String),
) -> Result<FeedbackBatchThreadRecord> {
    let (batch_id, thread_id, delivery_kind, ids_json) = raw;
    let message_ids: Vec<String> = serde_json::from_str(&ids_json)?;
    Ok(FeedbackBatchThreadRecord {
        batch_id,
        thread_id,
        delivery_kind,
        message_ids,
    })
}

fn thread_watermarks(
    conn: &Connection,
    session_id: &str,
) -> Result<std::collections::HashMap<String, Option<String>>> {
    let mut stmt = conn.prepare(
        "select id, last_delivered_message_id from review_threads where session_id = ?1",
    )?;
    let rows = stmt.query_map([session_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
    })?;
    let mut map = std::collections::HashMap::new();
    for row in rows {
        let (id, watermark) = row?;
        map.insert(id, watermark);
    }
    Ok(map)
}

fn select_messages_for_delivery(
    thread: &ReviewThreadRecord,
    watermark: Option<&str>,
) -> (String, Vec<ThreadMessageRecord>) {
    let visible: Vec<&ThreadMessageRecord> = thread
        .messages
        .iter()
        .filter(|message| message.visibility != "private")
        .collect();
    match watermark {
        None => {
            let included: Vec<ThreadMessageRecord> =
                visible.iter().map(|message| (*message).clone()).collect();
            let has_human = included
                .iter()
                .any(|message| message.author_kind == "human");
            if has_human {
                ("full".to_string(), included)
            } else {
                ("full".to_string(), Vec::new())
            }
        }
        Some(mark) => {
            let mut seen = false;
            let mut delta: Vec<ThreadMessageRecord> = Vec::new();
            for message in &visible {
                if seen {
                    delta.push((*message).clone());
                } else if message.id == mark {
                    seen = true;
                }
            }
            let has_new_human = delta.iter().any(|message| message.author_kind == "human");
            if has_new_human {
                ("delta".to_string(), delta)
            } else {
                ("delta".to_string(), Vec::new())
            }
        }
    }
}

fn render_feedback_markdown(
    session: &ReviewSessionRecord,
    snapshots: &[(ReviewThreadRecord, String, Vec<ThreadMessageRecord>)],
) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let _ = writeln!(out, "# Wyrd Diff feedback batch");
    let _ = writeln!(out);
    let _ = writeln!(out, "Session: {}", session.title);
    let _ = writeln!(
        out,
        "Range: {}..{} ({}..{})",
        session.base_ref,
        session.head_ref,
        short_sha(&session.base_sha),
        short_sha(&session.head_sha),
    );
    let _ = writeln!(out, "Threads needing attention: {}", snapshots.len());
    let _ = writeln!(out);
    if snapshots.is_empty() {
        let _ = writeln!(out, "No open threads with new reviewer input.");
        return out;
    }
    let _ = writeln!(
        out,
        "Resolve each thread, then import the fix commit referencing the thread ids below. Imports may pass `resolves_thread_ids` to auto-resolve."
    );
    let _ = writeln!(out);
    for (index, (thread, delivery_kind, messages)) in snapshots.iter().enumerate() {
        let _ = writeln!(out, "---");
        let _ = writeln!(out);
        let header_line = format_thread_anchor(thread);
        let _ = writeln!(
            out,
            "## Thread {} — {}{}",
            index + 1,
            thread.file_path,
            header_line,
        );
        let _ = writeln!(out, "thread_id: `{}`", thread.id);
        let _ = writeln!(
            out,
            "delivery: {} ({} message(s))",
            delivery_kind,
            messages.len()
        );
        if let Some(selected) = thread
            .selected_text
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            let _ = writeln!(out);
            let _ = writeln!(out, "Selected diff:");
            let _ = writeln!(out, "```");
            let _ = writeln!(out, "{}", selected.trim_end());
            let _ = writeln!(out, "```");
        }
        let _ = writeln!(out);
        let section_title = if delivery_kind == "delta" {
            "New replies"
        } else {
            "Messages"
        };
        let _ = writeln!(out, "### {section_title}");
        for message in messages {
            let author = message
                .author_name
                .as_deref()
                .unwrap_or(message.author_kind.as_str());
            let _ = writeln!(
                out,
                "- **[{}] {} — {}**",
                message.author_kind, author, message.created_at
            );
            for line in message.body.lines() {
                let _ = writeln!(out, "  {line}");
            }
        }
        let _ = writeln!(out);
    }
    out
}

fn format_thread_anchor(thread: &ReviewThreadRecord) -> String {
    let start = thread
        .range_start_new_line
        .or(thread.new_line)
        .or(thread.range_start_old_line)
        .or(thread.old_line);
    let end = thread
        .range_end_new_line
        .or(thread.new_line)
        .or(thread.range_end_old_line)
        .or(thread.old_line);
    match (start, end) {
        (Some(start), Some(end)) if start != end => format!(":{start}-{end}"),
        (Some(start), _) => format!(":{start}"),
        _ => String::new(),
    }
}

fn short_sha(value: &str) -> &str {
    let end = value.len().min(7);
    &value[..end]
}

#[cfg(test)]
mod tests {
    use super::{
        Database, NewComment, NewDecision, NewFixImport, NewNote, NewRepo, NewReviewSession,
        NewReviewThread, SourceContext,
    };
    use anyhow::Result;
    use std::process::Command;
    use tempfile::tempdir;

    #[test]
    fn creates_review_session_and_agent_context() -> Result<()> {
        let temp = tempdir()?;
        let repo_dir = temp.path().join("repo");
        std::fs::create_dir(&repo_dir)?;
        run(&repo_dir, ["git", "init"])?;
        run(
            &repo_dir,
            ["git", "config", "user.email", "test@example.com"],
        )?;
        run(&repo_dir, ["git", "config", "user.name", "Test User"])?;
        std::fs::write(repo_dir.join("a.txt"), "one\n")?;
        run(&repo_dir, ["git", "add", "."])?;
        run(&repo_dir, ["git", "commit", "-m", "base"])?;
        run(&repo_dir, ["git", "checkout", "-b", "feature"])?;
        std::fs::write(repo_dir.join("a.txt"), "one\ntwo\n")?;
        run(&repo_dir, ["git", "commit", "-am", "change"])?;

        let db = Database::new(temp.path().join("diff.db"));
        db.migrate()?;
        let repo = db.upsert_repo(NewRepo {
            name: "fixture".to_string(),
            path: repo_dir.display().to_string(),
        })?;
        let session = db.create_review_session(NewReviewSession {
            repo_id: repo.id.clone(),
            title: "feature review".to_string(),
            base_ref: "master".to_string(),
            head_ref: "feature".to_string(),
        })?;
        db.add_comment(NewComment {
            session_id: session.id.clone(),
            file_path: "a.txt".to_string(),
            diff_line_id: None,
            old_line: None,
            new_line: Some(2),
            range_start_old_line: None,
            range_start_new_line: Some(2),
            range_end_old_line: None,
            range_end_new_line: Some(2),
            selected_text: Some("+two".to_string()),
            body: "explain this addition".to_string(),
            status: None,
            visibility: None,
        })?;
        db.add_note(NewNote {
            repo_id: Some(repo.id),
            session_id: Some(session.id.clone()),
            title: "style".to_string(),
            body: "prefer explicit review intent".to_string(),
            note_type: "preference".to_string(),
            status: None,
            visibility: None,
            source_context: Some(SourceContext {
                file_path: "a.txt".to_string(),
                diff_line_id: None,
                old_line: None,
                new_line: Some(2),
                line_kind: Some("add".to_string()),
                content: Some("+two".to_string()),
            }),
        })?;
        db.add_decision(NewDecision {
            repo_id: None,
            session_id: Some(session.id.clone()),
            title: "opaque names".to_string(),
            context: "model drift".to_string(),
            decision: "treat names as opaque".to_string(),
            rationale: "provider-owned strings drift".to_string(),
            alternatives: None,
            consequences: None,
            status: None,
            visibility: None,
            source_context: None,
        })?;
        let thread = db.add_review_thread(NewReviewThread {
            session_id: session.id.clone(),
            file_path: "a.txt".to_string(),
            anchor_diff_line_id: None,
            old_line: None,
            new_line: Some(2),
            range_start_old_line: None,
            range_start_new_line: Some(2),
            range_end_old_line: None,
            range_end_new_line: Some(2),
            selected_text: Some("+two".to_string()),
            status: None,
            visibility: None,
            message_type: "agent_instruction".to_string(),
            body: "please explain and tighten this change".to_string(),
        })?;
        let fix_sha = command_output(&repo_dir, ["git", "rev-parse", "HEAD"])?;
        db.import_fix(NewFixImport {
            session_id: session.id.clone(),
            commit_sha: fix_sha.trim().to_string(),
            agent_name: Some("codex".to_string()),
            response_text: Some("tightened the change and verified tests".to_string()),
            tests_json: None,
            accepted: true,
            thread_id: Some(thread.id.clone()),
        })?;
        let context = db.agent_context(&session.id)?;
        assert_eq!(context.open_comments.len(), 1);
        assert_eq!(context.open_threads.len(), 1);
        assert_eq!(context.open_threads[0].messages.len(), 2);
        assert_eq!(context.open_threads[0].messages[1].author_kind, "agent");
        assert_eq!(context.agent_visible_notes.len(), 1);
        assert_eq!(
            context.agent_visible_notes[0]
                .source_context
                .as_ref()
                .map(|source| source.file_path.as_str()),
            Some("a.txt")
        );
        assert_eq!(context.accepted_decisions.len(), 1);
        Ok(())
    }

    fn run<const N: usize>(dir: &std::path::Path, args: [&str; N]) -> Result<()> {
        let status = Command::new(args[0])
            .args(&args[1..])
            .current_dir(dir)
            .status()?;
        assert!(status.success(), "{args:?}");
        Ok(())
    }

    fn command_output<const N: usize>(dir: &std::path::Path, args: [&str; N]) -> Result<String> {
        let output = Command::new(args[0])
            .args(&args[1..])
            .current_dir(dir)
            .output()?;
        assert!(output.status.success(), "{args:?}");
        Ok(String::from_utf8(output.stdout)?)
    }
}
