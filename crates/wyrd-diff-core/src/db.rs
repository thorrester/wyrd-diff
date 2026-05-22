//! SQLite persistence.

use crate::{
    ActiveReviewSessionRecord, AgentSessionRecord, FeedbackBatchRecord, FeedbackBatchThreadRecord,
    FileRecord, FixImportRecord, GitRepo, OverviewEntry, RepoRecord, ReviewDiffLine,
    ReviewFileDiff, ReviewHunk, ReviewSessionRecord, ReviewThreadRecord, SourceContext,
    ThreadMessageRecord,
    export::{AgentContext, TrajectoryRecord},
};
use anyhow::{Context, Result};
use chrono::Utc;
use r2d2::{Pool, PooledConnection};
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, OnceLock},
};
use tracing::{debug, info_span};
use uuid::Uuid;

const INIT_SQL: &str = include_str!("../../../migrations/001_init.sql");

/// Pooled SQLite connection alias.
pub type PooledConn = PooledConnection<SqliteConnectionManager>;

/// SQLite database handle. Cheap to clone; backing pool is shared.
#[derive(Debug, Clone)]
pub struct Database {
    inner: Arc<DatabaseInner>,
}

#[derive(Debug)]
struct DatabaseInner {
    path: PathBuf,
    pool: OnceLock<Pool<SqliteConnectionManager>>,
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
    /// Branch this review targets. Defaults to `head_ref` when omitted.
    #[serde(default)]
    pub branch: Option<String>,
}

/// Agent-session registration payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewAgentSession {
    /// Harness-provided session id (Claude/Codex/etc.).
    pub agent_session_id: String,
    /// Agent name, e.g. "claude" or "codex".
    pub agent_name: String,
    /// Absolute repo path the agent is running in. Optional for non-git contexts.
    #[serde(default)]
    pub repo_path: Option<String>,
    /// Current git branch at registration time. Optional for detached HEAD.
    #[serde(default)]
    pub branch: Option<String>,
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
    /// Create a database wrapper. Pool is built lazily on first connect.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            inner: Arc::new(DatabaseInner {
                path: path.into(),
                pool: OnceLock::new(),
            }),
        }
    }

    /// Database path.
    #[must_use]
    pub fn path(&self) -> &PathBuf {
        &self.inner.path
    }

    fn pool(&self) -> Result<&Pool<SqliteConnectionManager>> {
        if let Some(pool) = self.inner.pool.get() {
            return Ok(pool);
        }
        let path = &self.inner.path;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let manager = SqliteConnectionManager::file(path).with_init(|conn| {
            conn.busy_timeout(std::time::Duration::from_secs(5))?;
            conn.pragma_update(None, "journal_mode", "WAL")?;
            conn.pragma_update(None, "foreign_keys", "ON")?;
            conn.pragma_update(None, "synchronous", "NORMAL")?;
            Ok(())
        });
        let pool = Pool::builder()
            .max_size(8)
            .build(manager)
            .with_context(|| format!("failed to build connection pool for {}", path.display()))?;
        let _ = self.inner.pool.set(pool);
        Ok(self.inner.pool.get().expect("pool was just initialized"))
    }

    /// Acquire a pooled SQLite connection.
    ///
    /// # Errors
    /// Returns an error when SQLite cannot open the database or the pool is exhausted.
    pub fn connect(&self) -> Result<PooledConn> {
        let pool = self.pool()?;
        pool.get().with_context(|| {
            format!(
                "failed to get connection from pool for {}",
                self.inner.path.display()
            )
        })
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
        ensure_multi_branch_sessions(&mut conn)?;
        ensure_patch_blob_column(&conn)?;
        Ok(())
    }

    /// Delete and recreate the database.
    ///
    /// # Errors
    /// Returns an error when reset or migration fails.
    pub fn reset(&self) -> Result<()> {
        let path = self.inner.path.clone();
        if path.exists() {
            fs::remove_file(&path)?;
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
        let _span = info_span!(
            "create_review_session",
            repo_id = %input.repo_id,
            base = %input.base_ref,
            head = %input.head_ref,
        )
        .entered();
        let repo = self
            .repo(&input.repo_id)?
            .with_context(|| format!("repo not found: {}", input.repo_id))?;
        let git = GitRepo::open(&repo.path)?;
        let base_sha = {
            let _s = info_span!("resolve_refs").entered();
            git.resolve_ref(&input.base_ref)?
        };
        let head_sha = git.resolve_ref(&input.head_ref)?;
        let commits = {
            let _s = info_span!("git_commits").entered();
            git.commits(&input.base_ref, &input.head_ref)?
        };
        let files = {
            let _s = info_span!("git_diff_files").entered();
            git.diff_files(&input.base_ref, &input.head_ref)?
        };
        let file_count = files.len();
        let blob_bytes: usize = files.iter().map(|f| f.patch_blob.len()).sum();
        debug!(file_count, blob_bytes, "diff snapshot loaded");
        let _persist_span = info_span!("persist_diff", file_count, blob_bytes).entered();

        let mut conn = self.connect()?;
        let tx = conn.transaction()?;
        let id = new_id();
        let timestamp = now();
        let branch = input
            .branch
            .clone()
            .or_else(|| Some(input.head_ref.clone()));
        tx.execute(
            "insert into review_sessions(id, repo_id, title, base_ref, head_ref, branch, base_sha, head_sha, status, created_at, updated_at)
             values(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'open', ?9, ?9)",
            params![
                id,
                input.repo_id,
                input.title,
                input.base_ref,
                input.head_ref,
                branch,
                base_sha,
                head_sha,
                timestamp
            ],
        )?;

        {
            let mut commit_stmt = tx.prepare_cached(
                "insert into commits(id, session_id, sha, short_sha, subject, body, author_name, author_email, authored_at)
                 values(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            )?;
            for commit in &commits {
                commit_stmt.execute(params![
                    new_id(),
                    id,
                    commit.sha,
                    commit.short_sha,
                    commit.subject,
                    commit.body,
                    commit.author_name,
                    commit.author_email,
                    commit.authored_at
                ])?;
            }
        }

        {
            let mut file_stmt = tx.prepare_cached(
                "insert into files(id, session_id, path, old_path, status, additions, deletions, review_state, patch_blob)
                 values(?1, ?2, ?3, ?4, ?5, ?6, ?7, 'unreviewed', ?8)",
            )?;
            for file in &files {
                file_stmt.execute(params![
                    new_id(),
                    id,
                    file.path,
                    file.old_path,
                    file.status,
                    file.additions,
                    file.deletions,
                    file.patch_blob,
                ])?;
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
        let active_branch = branch.clone().unwrap_or_else(|| input.head_ref.clone());
        tx.execute(
            "insert into active_review_sessions(repo_id, branch, session_id, start_sha, updated_at)
             values(?1, ?2, ?3, ?4, ?5)
             on conflict(repo_id, branch) do update set
               session_id = excluded.session_id,
               start_sha = excluded.start_sha,
               updated_at = excluded.updated_at",
            params![repo.id, active_branch, id, head_sha, timestamp],
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
        let _span = info_span!("refresh_review_session", session_id = %session_id).entered();
        let session = self
            .review_session(session_id)?
            .with_context(|| format!("review session not found: {session_id}"))?;
        let repo = self
            .repo(&session.repo_id)?
            .with_context(|| format!("repo not found: {}", session.repo_id))?;
        let git = GitRepo::open(&repo.path)?;
        let base_sha = {
            let _s = info_span!("resolve_refs").entered();
            git.resolve_ref(&session.base_ref)?
        };
        let head_sha = git.resolve_ref(&session.head_ref)?;
        let commits = {
            let _s = info_span!("git_commits").entered();
            git.commits(&session.base_ref, &session.head_ref)?
        };
        let files = {
            let _s = info_span!("git_diff_files").entered();
            git.diff_files(&session.base_ref, &session.head_ref)?
        };
        let file_count = files.len();
        let blob_bytes: usize = files.iter().map(|f| f.patch_blob.len()).sum();
        debug!(file_count, blob_bytes, "diff snapshot loaded");
        let _persist_span = info_span!("persist_diff", file_count, blob_bytes).entered();

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

        {
            let mut commit_stmt = tx.prepare_cached(
                "insert into commits(id, session_id, sha, short_sha, subject, body, author_name, author_email, authored_at)
                 values(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            )?;
            for commit in &commits {
                commit_stmt.execute(params![
                    new_id(),
                    session_id,
                    commit.sha,
                    commit.short_sha,
                    commit.subject,
                    commit.body,
                    commit.author_name,
                    commit.author_email,
                    commit.authored_at
                ])?;
            }
        }

        {
            let mut file_stmt = tx.prepare_cached(
                "insert into files(id, session_id, path, old_path, status, additions, deletions, review_state, patch_blob)
                 values(?1, ?2, ?3, ?4, ?5, ?6, ?7, 'unreviewed', ?8)",
            )?;
            for file in &files {
                file_stmt.execute(params![
                    new_id(),
                    session_id,
                    file.path,
                    file.old_path,
                    file.status,
                    file.additions,
                    file.deletions,
                    file.patch_blob,
                ])?;
            }
        }

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
            "select id, repo_id, title, base_ref, head_ref, branch, base_sha, head_sha, status, created_at, updated_at
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
            "select id, repo_id, title, base_ref, head_ref, branch, base_sha, head_sha, status, created_at, updated_at
             from review_sessions order by created_at desc",
        )?;
        let rows = stmt.query_map([], map_review_session)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    /// Mark a review session as the active session for its (repo, branch) tuple.
    ///
    /// Branch is taken from `session.branch`, falling back to `session.head_ref`
    /// when null. This means existing reviews that pre-date the multi-branch
    /// migration continue to activate against their head ref as the key.
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
        let branch = session
            .branch
            .clone()
            .unwrap_or_else(|| session.head_ref.clone());
        let timestamp = now();
        let conn = self.connect()?;
        conn.execute(
            "insert into active_review_sessions(repo_id, branch, session_id, start_sha, updated_at)
             values(?1, ?2, ?3, ?4, ?5)
             on conflict(repo_id, branch) do update set
               session_id = excluded.session_id,
               start_sha = excluded.start_sha,
               updated_at = excluded.updated_at",
            params![
                session.repo_id,
                branch,
                session.id,
                session.head_sha,
                timestamp
            ],
        )?;
        Ok(ActiveReviewSessionRecord {
            repo,
            branch,
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
    /// When `branch` is `Some`, returns the active review for that exact branch.
    /// When `branch` is `None`, returns any active review for the repo (newest
    /// updated_at wins). The `None` case exists to keep one-call callers working
    /// when the branch is unknown; prefer passing `Some(branch)` when possible.
    ///
    /// # Errors
    /// Returns an error when SQLite fails.
    pub fn active_review_session_for_repo_path(
        &self,
        path: impl Into<PathBuf>,
        branch: Option<&str>,
    ) -> Result<Option<ActiveReviewSessionRecord>> {
        let path = path.into().canonicalize()?.display().to_string();
        let conn = self.connect()?;
        let row_to_record =
            |row: &rusqlite::Row<'_>| -> rusqlite::Result<ActiveReviewSessionRecord> {
                Ok(ActiveReviewSessionRecord {
                    repo: RepoRecord {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        path: row.get(2)?,
                        created_at: row.get(3)?,
                        updated_at: row.get(4)?,
                    },
                    branch: row.get(17)?,
                    session: ReviewSessionRecord {
                        id: row.get(5)?,
                        repo_id: row.get(6)?,
                        title: row.get(7)?,
                        base_ref: row.get(8)?,
                        head_ref: row.get(9)?,
                        branch: row.get(10)?,
                        base_sha: row.get(11)?,
                        head_sha: row.get(12)?,
                        status: row.get(13)?,
                        created_at: row.get(14)?,
                        updated_at: row.get(15)?,
                    },
                    start_sha: row.get(16)?,
                    updated_at: row.get(18)?,
                })
            };
        let base_select = "select
                r.id, r.name, r.path, r.created_at, r.updated_at,
                rs.id, rs.repo_id, rs.title, rs.base_ref, rs.head_ref, rs.branch, rs.base_sha, rs.head_sha, rs.status, rs.created_at, rs.updated_at,
                ars.start_sha, ars.branch, ars.updated_at
             from repos r
             join active_review_sessions ars on ars.repo_id = r.id
             join review_sessions rs on rs.id = ars.session_id";
        match branch {
            Some(branch) => conn
                .query_row(
                    &format!("{base_select} where r.path = ?1 and ars.branch = ?2"),
                    params![path, branch],
                    row_to_record,
                )
                .optional()
                .map_err(Into::into),
            None => conn
                .query_row(
                    &format!(
                        "{base_select} where r.path = ?1 order by ars.updated_at desc limit 1"
                    ),
                    params![path],
                    row_to_record,
                )
                .optional()
                .map_err(Into::into),
        }
    }

    /// Aggregated fleet-view overview: every active review across repos +
    /// branches, with open + pending thread counts and live agent sessions.
    ///
    /// `idle_cutoff_seconds` filters agent sessions by `last_activity_at`.
    /// Pass `i64::MAX` to disable filtering.
    ///
    /// # Errors
    /// Returns an error when SQLite reads fail.
    pub fn overview(&self, idle_cutoff_seconds: i64) -> Result<Vec<OverviewEntry>> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare(
            "select
                r.id, r.name, r.path, r.created_at, r.updated_at,
                rs.id, rs.repo_id, rs.title, rs.base_ref, rs.head_ref, rs.branch, rs.base_sha, rs.head_sha, rs.status, rs.created_at, rs.updated_at,
                ars.branch, ars.updated_at
             from active_review_sessions ars
             join repos r on r.id = ars.repo_id
             join review_sessions rs on rs.id = ars.session_id
             order by ars.updated_at desc",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                RepoRecord {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    path: row.get(2)?,
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
                },
                ReviewSessionRecord {
                    id: row.get(5)?,
                    repo_id: row.get(6)?,
                    title: row.get(7)?,
                    base_ref: row.get(8)?,
                    head_ref: row.get(9)?,
                    branch: row.get(10)?,
                    base_sha: row.get(11)?,
                    head_sha: row.get(12)?,
                    status: row.get(13)?,
                    created_at: row.get(14)?,
                    updated_at: row.get(15)?,
                },
                row.get::<_, String>(16)?,
                row.get::<_, String>(17)?,
            ))
        })?;
        let entries: Vec<_> = rows.collect::<rusqlite::Result<Vec<_>>>()?;

        let repo_ids: std::collections::HashSet<String> = entries
            .iter()
            .map(|(_, session, _, _)| session.repo_id.clone())
            .collect();
        let cutoff = if idle_cutoff_seconds == i64::MAX {
            None
        } else {
            Some(
                Utc::now()
                    - chrono::Duration::try_seconds(idle_cutoff_seconds)
                        .unwrap_or_else(chrono::Duration::zero),
            )
        };
        let mut agents_by_key: std::collections::HashMap<
            (String, String),
            Vec<AgentSessionRecord>,
        > = std::collections::HashMap::new();
        if !repo_ids.is_empty() {
            let repo_id_vec: Vec<String> = repo_ids.into_iter().collect();
            let placeholders = (1..=repo_id_vec.len())
                .map(|i| format!("?{i}"))
                .collect::<Vec<_>>()
                .join(",");
            let sql = format!(
                "select id, agent_session_id, agent_name, repo_id, branch,
                        started_at, last_activity_at, ended_at
                 from agent_sessions
                 where repo_id in ({placeholders})
                 order by last_activity_at desc"
            );
            let mut agent_stmt = conn.prepare(&sql)?;
            let params: Vec<&dyn rusqlite::ToSql> = repo_id_vec
                .iter()
                .map(|s| s as &dyn rusqlite::ToSql)
                .collect();
            let agent_rows = agent_stmt.query_map(params.as_slice(), map_agent_session)?;
            for row in agent_rows {
                let record = row?;
                if let Some(cutoff_dt) = cutoff
                    && let Ok(parsed) =
                        chrono::DateTime::parse_from_rfc3339(&record.last_activity_at)
                    && parsed.with_timezone(&Utc) < cutoff_dt
                {
                    continue;
                }
                let Some(repo_id) = record.repo_id.clone() else {
                    continue;
                };
                let Some(branch) = record.branch.clone() else {
                    continue;
                };
                agents_by_key
                    .entry((repo_id, branch))
                    .or_default()
                    .push(record);
            }
        }

        let mut out = Vec::with_capacity(entries.len());
        for (repo, session, branch, updated_at) in entries {
            let open_thread_count: i64 = conn.query_row(
                "select count(*) from review_threads
                 where session_id = ?1 and status = 'open'",
                [&session.id],
                |row| row.get(0),
            )?;
            let pending_thread_count = self.pending_thread_count(&conn, &session.id)?;
            let session_branch = session
                .branch
                .clone()
                .unwrap_or_else(|| session.head_ref.clone());
            let mut agent_sessions = agents_by_key
                .remove(&(session.repo_id.clone(), session_branch))
                .unwrap_or_default();
            for record in &mut agent_sessions {
                record.review_session_id = Some(session.id.clone());
            }
            out.push(OverviewEntry {
                repo,
                branch,
                session,
                open_thread_count,
                pending_thread_count,
                agent_sessions,
                updated_at,
            });
        }
        Ok(out)
    }

    fn pending_thread_count(&self, conn: &Connection, session_id: &str) -> Result<i64> {
        let mut stmt = conn.prepare(
            "select t.id, t.last_delivered_message_id,
                    (select id from thread_messages m
                       where m.thread_id = t.id and m.visibility != 'private'
                       order by m.created_at desc, m.id desc limit 1) as latest_id,
                    (select count(*) from thread_messages m
                       where m.thread_id = t.id and m.visibility != 'private'
                         and m.author_kind = 'human') as human_count
             from review_threads t
             where t.session_id = ?1 and t.status = 'open' and t.visibility = 'agent'",
        )?;
        let rows = stmt.query_map([session_id], |row| {
            Ok((
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })?;
        let mut count = 0;
        for row in rows {
            let (watermark, latest, human_count) = row?;
            let pending = match (watermark.as_deref(), latest.as_deref()) {
                (None, Some(_)) if human_count > 0 => true,
                (Some(w), Some(l)) if w != l => true,
                _ => false,
            };
            if pending {
                count += 1;
            }
        }
        Ok(count)
    }

    /// Register an agent session, resolving its repo by path when available.
    ///
    /// Idempotent on `(agent_session_id, agent_name)`. If a row already exists,
    /// `repo_id` and `branch` are refreshed and `last_activity_at` bumped, but
    /// `started_at` is preserved. The linked review session is computed lazily
    /// at lookup time, not stored on this row.
    ///
    /// # Errors
    /// Returns an error when SQLite fails.
    pub fn register_agent_session(&self, input: NewAgentSession) -> Result<AgentSessionRecord> {
        let timestamp = now();
        let repo_id = match input.repo_path.as_deref() {
            Some(path) => self.repo_id_for_path(path)?,
            None => None,
        };
        let conn = self.connect()?;
        let existing: Option<String> = conn
            .query_row(
                "select id from agent_sessions
                 where agent_session_id = ?1 and agent_name = ?2",
                params![input.agent_session_id, input.agent_name],
                |row| row.get(0),
            )
            .optional()?;
        let id = match existing {
            Some(id) => {
                conn.execute(
                    "update agent_sessions
                     set repo_id = ?1, branch = ?2, last_activity_at = ?3, ended_at = null
                     where id = ?4",
                    params![repo_id, input.branch, timestamp, id],
                )?;
                id
            }
            None => {
                let id = new_id();
                conn.execute(
                    "insert into agent_sessions
                       (id, agent_session_id, agent_name, repo_id, branch, started_at, last_activity_at)
                     values(?1, ?2, ?3, ?4, ?5, ?6, ?6)",
                    params![
                        id,
                        input.agent_session_id,
                        input.agent_name,
                        repo_id,
                        input.branch,
                        timestamp
                    ],
                )?;
                id
            }
        };
        self.agent_session(&id)?
            .context("agent session was not persisted")
    }

    /// Bump an agent session's activity timestamp. No-op if it does not exist.
    ///
    /// # Errors
    /// Returns an error when SQLite fails.
    pub fn bump_agent_session_activity(
        &self,
        agent_session_id: &str,
        agent_name: &str,
    ) -> Result<()> {
        let conn = self.connect()?;
        conn.execute(
            "update agent_sessions set last_activity_at = ?1
             where agent_session_id = ?2 and agent_name = ?3",
            params![now(), agent_session_id, agent_name],
        )?;
        Ok(())
    }

    /// Mark an agent session ended.
    ///
    /// # Errors
    /// Returns an error when SQLite fails.
    pub fn end_agent_session(&self, agent_session_id: &str, agent_name: &str) -> Result<()> {
        let conn = self.connect()?;
        let timestamp = now();
        conn.execute(
            "update agent_sessions
             set ended_at = ?1, last_activity_at = ?1
             where agent_session_id = ?2 and agent_name = ?3",
            params![timestamp, agent_session_id, agent_name],
        )?;
        Ok(())
    }

    /// Fetch one agent session by internal row id.
    ///
    /// # Errors
    /// Returns an error when SQLite fails.
    pub fn agent_session(&self, id: &str) -> Result<Option<AgentSessionRecord>> {
        let conn = self.connect()?;
        let row = conn
            .query_row(
                "select id, agent_session_id, agent_name, repo_id, branch,
                        started_at, last_activity_at, ended_at
                 from agent_sessions where id = ?1",
                [id],
                map_agent_session,
            )
            .optional()?;
        match row {
            Some(record) => Ok(Some(self.attach_review_link(record)?)),
            None => Ok(None),
        }
    }

    /// Fetch an agent session by the harness-provided id + agent name.
    ///
    /// # Errors
    /// Returns an error when SQLite fails.
    pub fn agent_session_by_external_id(
        &self,
        agent_session_id: &str,
        agent_name: &str,
    ) -> Result<Option<AgentSessionRecord>> {
        let conn = self.connect()?;
        let row = conn
            .query_row(
                "select id, agent_session_id, agent_name, repo_id, branch,
                        started_at, last_activity_at, ended_at
                 from agent_sessions
                 where agent_session_id = ?1 and agent_name = ?2",
                params![agent_session_id, agent_name],
                map_agent_session,
            )
            .optional()?;
        match row {
            Some(record) => Ok(Some(self.attach_review_link(record)?)),
            None => Ok(None),
        }
    }

    /// List agent sessions linked to a review session (active in the
    /// last `idle_cutoff_seconds`). Pass `i64::MAX` to include everything.
    ///
    /// # Errors
    /// Returns an error when SQLite fails.
    pub fn agent_sessions_for_review(
        &self,
        session_id: &str,
        idle_cutoff_seconds: i64,
    ) -> Result<Vec<AgentSessionRecord>> {
        let session = self
            .review_session(session_id)?
            .with_context(|| format!("review session not found: {session_id}"))?;
        let branch = session
            .branch
            .clone()
            .unwrap_or_else(|| session.head_ref.clone());
        let conn = self.connect()?;
        let mut stmt = conn.prepare(
            "select id, agent_session_id, agent_name, repo_id, branch,
                    started_at, last_activity_at, ended_at
             from agent_sessions
             where repo_id = ?1 and branch = ?2
             order by last_activity_at desc",
        )?;
        let rows = stmt.query_map(params![session.repo_id, branch], map_agent_session)?;
        let mut out = Vec::new();
        let cutoff = if idle_cutoff_seconds == i64::MAX {
            None
        } else {
            Some(
                Utc::now()
                    - chrono::Duration::try_seconds(idle_cutoff_seconds)
                        .unwrap_or_else(chrono::Duration::zero),
            )
        };
        for row in rows {
            let mut record = row?;
            if let Some(cutoff_dt) = cutoff
                && let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(&record.last_activity_at)
                && parsed.with_timezone(&Utc) < cutoff_dt
            {
                continue;
            }
            record.review_session_id = Some(session_id.to_string());
            out.push(record);
        }
        Ok(out)
    }

    /// Resolve the linked review session id for an agent session, by looking up
    /// the active review for that agent's `(repo, branch)` at call time.
    ///
    /// # Errors
    /// Returns an error when SQLite fails.
    pub fn linked_review_session_for_agent(
        &self,
        agent_session_id: &str,
        agent_name: &str,
    ) -> Result<Option<String>> {
        let record = match self.agent_session_by_external_id(agent_session_id, agent_name)? {
            Some(record) => record,
            None => return Ok(None),
        };
        Ok(record.review_session_id)
    }

    fn attach_review_link(&self, mut record: AgentSessionRecord) -> Result<AgentSessionRecord> {
        if let (Some(repo_id), Some(branch)) = (record.repo_id.as_deref(), record.branch.as_deref())
        {
            let conn = self.connect()?;
            let id: Option<String> = conn
                .query_row(
                    "select session_id from active_review_sessions
                     where repo_id = ?1 and branch = ?2",
                    params![repo_id, branch],
                    |row| row.get(0),
                )
                .optional()?;
            record.review_session_id = id;
        }
        Ok(record)
    }

    fn repo_id_for_path(&self, path: &str) -> Result<Option<String>> {
        let canonical = match PathBuf::from(path).canonicalize() {
            Ok(p) => p.display().to_string(),
            Err(_) => return Ok(None),
        };
        let conn = self.connect()?;
        let id: Option<String> = conn
            .query_row("select id from repos where path = ?1", [canonical], |row| {
                row.get(0)
            })
            .optional()?;
        Ok(id)
    }

    /// Fetch lightweight file summaries for a review session.
    ///
    /// Does not load `patch_blob` — use [`Database::review_diff_file`] per file
    /// when the UI is ready to render that file's hunks.
    ///
    /// # Errors
    /// Returns an error when SQLite fails.
    pub fn review_diff_summary(&self, session_id: &str) -> Result<Vec<FileRecord>> {
        let _span = info_span!("review_diff_summary", session_id = %session_id).entered();
        let conn = self.connect()?;
        let mut stmt = conn.prepare_cached(
            "select id, session_id, path, old_path, status, additions, deletions, review_state
             from files where session_id = ?1 order by path",
        )?;
        let rows = stmt.query_map([session_id], |row| {
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
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    /// Fetch one file's parsed diff for a review session.
    ///
    /// Reads the file row + stored `patch_blob` and reparses on demand. Returns
    /// `None` when the session has no row matching `file_path`.
    ///
    /// # Errors
    /// Returns an error when SQLite fails.
    pub fn review_diff_file(
        &self,
        session_id: &str,
        file_path: &str,
    ) -> Result<Option<ReviewFileDiff>> {
        let _span = info_span!(
            "review_diff_file",
            session_id = %session_id,
            file = %file_path,
        )
        .entered();
        let conn = self.connect()?;
        let mut stmt = conn.prepare_cached(
            "select id, session_id, path, old_path, status, additions, deletions, review_state, patch_blob
             from files where session_id = ?1 and path = ?2 limit 1",
        )?;
        let row = stmt
            .query_row(params![session_id, file_path], |row| {
                let file = FileRecord {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    path: row.get(2)?,
                    old_path: row.get(3)?,
                    status: row.get(4)?,
                    additions: row.get(5)?,
                    deletions: row.get(6)?,
                    review_state: row.get(7)?,
                };
                let blob: Option<String> = row.get(8)?;
                Ok((file, blob))
            })
            .optional()?;
        let Some((file, blob)) = row else {
            return Ok(None);
        };
        let hunks = blob
            .as_deref()
            .map(crate::git::parse_unified_diff)
            .and_then(|parsed| parsed.into_iter().next())
            .map(|df| {
                df.hunks
                    .into_iter()
                    .enumerate()
                    .map(|(hi, h)| ReviewHunk {
                        id: format!("{}#h{hi}", file.path),
                        old_start: Some(h.old_start),
                        old_lines: Some(h.old_lines),
                        new_start: Some(h.new_start),
                        new_lines: Some(h.new_lines),
                        patch: h.patch,
                        lines: h
                            .lines
                            .into_iter()
                            .enumerate()
                            .map(|(li, l)| ReviewDiffLine {
                                id: format!("{}#h{hi}#l{li}", file.path),
                                file_path: file.path.clone(),
                                old_line: l.old_line,
                                new_line: l.new_line,
                                line_kind: l.line_kind,
                                content: l.content,
                            })
                            .collect(),
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        Ok(Some(ReviewFileDiff { file, hunks }))
    }

    /// Fetch all diff files for a review session by reparsing each per-file
    /// `patch_blob`. Prefer the summary + per-file APIs for interactive UI to
    /// avoid loading every file at once.
    ///
    /// # Errors
    /// Returns an error when SQLite fails.
    pub fn review_diff(&self, session_id: &str) -> Result<Vec<ReviewFileDiff>> {
        let _span = info_span!("review_diff_read", session_id = %session_id).entered();
        let summary = self.review_diff_summary(session_id)?;
        let mut out = Vec::with_capacity(summary.len());
        for record in summary {
            if let Some(diff) = self.review_diff_file(session_id, &record.path)? {
                out.push(diff);
            }
        }
        Ok(out)
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
                Option::<String>::None,
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
                Option::<String>::None,
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
                Option::<String>::None,
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
                Option::<String>::None,
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
        let conn = self.connect()?;
        let watermarks = thread_watermarks(&conn, session_id)?;
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
    pub fn export_connection(&self) -> Result<PooledConn> {
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

fn ensure_multi_branch_sessions(conn: &mut Connection) -> Result<()> {
    ensure_column(conn, "review_sessions", "branch", "text")?;
    let needs_rebuild = active_review_sessions_needs_rebuild(conn)?;
    if needs_rebuild {
        rebuild_active_review_sessions(conn)?;
    }
    conn.execute(
        "create table if not exists agent_sessions (
          id text primary key,
          agent_session_id text not null,
          agent_name text not null,
          repo_id text references repos(id),
          branch text,
          started_at text not null,
          last_activity_at text not null,
          ended_at text,
          unique (agent_session_id, agent_name)
        )",
        [],
    )?;
    conn.execute(
        "create index if not exists idx_agent_sessions_repo_branch
         on agent_sessions(repo_id, branch)",
        [],
    )?;
    conn.execute(
        "insert or ignore into schema_migrations(version, applied_at) values(4, ?1)",
        [now()],
    )?;
    Ok(())
}

fn active_review_sessions_needs_rebuild(conn: &Connection) -> Result<bool> {
    let mut stmt = conn.prepare("pragma table_info(active_review_sessions)")?;
    let mut has_branch = false;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let name: String = row.get(1)?;
        if name == "branch" {
            has_branch = true;
            break;
        }
    }
    Ok(!has_branch)
}

fn rebuild_active_review_sessions(conn: &mut Connection) -> Result<()> {
    conn.pragma_update(None, "foreign_keys", "OFF")?;
    let tx = conn.transaction()?;
    tx.execute(
        "create table active_review_sessions_new (
          repo_id text not null references repos(id),
          branch text not null,
          session_id text not null references review_sessions(id),
          start_sha text not null,
          updated_at text not null,
          primary key (repo_id, branch)
        )",
        [],
    )?;
    tx.execute(
        "insert into active_review_sessions_new (repo_id, branch, session_id, start_sha, updated_at)
         select ars.repo_id,
                coalesce(rs.branch, rs.head_ref, '__legacy__'),
                ars.session_id,
                ars.start_sha,
                ars.updated_at
         from active_review_sessions ars
         left join review_sessions rs on rs.id = ars.session_id",
        [],
    )?;
    tx.execute("drop table active_review_sessions", [])?;
    tx.execute(
        "alter table active_review_sessions_new rename to active_review_sessions",
        [],
    )?;
    tx.execute(
        "update review_sessions
         set branch = coalesce(branch, head_ref)
         where branch is null",
        [],
    )?;
    tx.commit()?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    Ok(())
}

fn ensure_patch_blob_column(conn: &Connection) -> Result<()> {
    ensure_column(conn, "files", "patch_blob", "text")?;
    conn.execute(
        "insert or ignore into schema_migrations(version, applied_at) values(4, ?1)",
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

fn map_agent_session(row: &rusqlite::Row<'_>) -> rusqlite::Result<AgentSessionRecord> {
    Ok(AgentSessionRecord {
        id: row.get(0)?,
        agent_session_id: row.get(1)?,
        agent_name: row.get(2)?,
        repo_id: row.get(3)?,
        branch: row.get(4)?,
        review_session_id: None,
        started_at: row.get(5)?,
        last_activity_at: row.get(6)?,
        ended_at: row.get(7)?,
    })
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
        branch: row.get(5)?,
        base_sha: row.get(6)?,
        head_sha: row.get(7)?,
        status: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
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
        Database, NewAgentSession, NewComment, NewDecision, NewFixImport, NewNote, NewRepo,
        NewReviewSession, NewReviewThread, SourceContext,
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
            branch: Some("feature".to_string()),
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

    #[test]
    fn agent_session_registration_links_to_active_review() -> Result<()> {
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
            branch: Some("feature".to_string()),
        })?;

        let agent = db.register_agent_session(NewAgentSession {
            agent_session_id: "claude-abc".to_string(),
            agent_name: "claude".to_string(),
            repo_path: Some(repo_dir.display().to_string()),
            branch: Some("feature".to_string()),
        })?;
        assert_eq!(
            agent.review_session_id.as_deref(),
            Some(session.id.as_str())
        );
        assert_eq!(agent.repo_id.as_deref(), Some(repo.id.as_str()));

        // Idempotent: re-register same id keeps row id stable.
        let again = db.register_agent_session(NewAgentSession {
            agent_session_id: "claude-abc".to_string(),
            agent_name: "claude".to_string(),
            repo_path: Some(repo_dir.display().to_string()),
            branch: Some("feature".to_string()),
        })?;
        assert_eq!(again.id, agent.id);

        // Lookup by external id resolves the linked review session.
        let linked = db.linked_review_session_for_agent("claude-abc", "claude")?;
        assert_eq!(linked.as_deref(), Some(session.id.as_str()));

        // Agent session registered on a branch with no active review returns
        // None for the linked id, but the row still exists.
        let other = db.register_agent_session(NewAgentSession {
            agent_session_id: "codex-xyz".to_string(),
            agent_name: "codex".to_string(),
            repo_path: Some(repo_dir.display().to_string()),
            branch: Some("other-branch".to_string()),
        })?;
        assert!(other.review_session_id.is_none());

        // Multiple agents on the same branch both surface in the list.
        let _other_on_feature = db.register_agent_session(NewAgentSession {
            agent_session_id: "codex-2".to_string(),
            agent_name: "codex".to_string(),
            repo_path: Some(repo_dir.display().to_string()),
            branch: Some("feature".to_string()),
        })?;
        let active = db.agent_sessions_for_review(&session.id, i64::MAX)?;
        assert_eq!(active.len(), 2);

        // Branch-aware active session lookup.
        let resolved = db
            .active_review_session_for_repo_path(repo_dir.display().to_string(), Some("feature"))?
            .expect("active session for feature");
        assert_eq!(resolved.session.id, session.id);
        assert_eq!(resolved.branch, "feature");

        // Wrong branch returns None.
        let missing =
            db.active_review_session_for_repo_path(repo_dir.display().to_string(), Some("nope"))?;
        assert!(missing.is_none());

        Ok(())
    }

    #[test]
    fn add_review_thread_accepts_non_persistent_anchor_id() -> Result<()> {
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
            repo_id: repo.id,
            title: "feature review".to_string(),
            base_ref: "master".to_string(),
            head_ref: "feature".to_string(),
            branch: Some("feature".to_string()),
        })?;

        // Frontend sends synthetic line ids like "a.txt#h0#l0" that do not
        // correspond to any diff_lines row. The insert must succeed and the
        // stored anchor must be NULL.
        let thread = db.add_review_thread(NewReviewThread {
            session_id: session.id.clone(),
            file_path: "a.txt".to_string(),
            anchor_diff_line_id: Some("a.txt#h0#l0".to_string()),
            old_line: None,
            new_line: Some(2),
            range_start_old_line: None,
            range_start_new_line: Some(2),
            range_end_old_line: None,
            range_end_new_line: Some(2),
            selected_text: Some("+two".to_string()),
            status: None,
            visibility: None,
            message_type: "comment".to_string(),
            body: "needs a check".to_string(),
        })?;
        assert!(thread.anchor_diff_line_id.is_none());
        let threads = db.review_threads(&session.id)?;
        assert_eq!(threads.len(), 1);
        assert!(threads[0].anchor_diff_line_id.is_none());
        assert_eq!(threads[0].new_line, Some(2));
        Ok(())
    }

    /// Helper: build a session against a tiny fixture repo.
    fn fixture_session(temp: &std::path::Path) -> Result<(Database, String)> {
        let repo_dir = temp.join("repo");
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
        let db = Database::new(temp.join("diff.db"));
        db.migrate()?;
        let repo = db.upsert_repo(NewRepo {
            name: "fixture".to_string(),
            path: repo_dir.display().to_string(),
        })?;
        let session = db.create_review_session(NewReviewSession {
            repo_id: repo.id,
            title: "feature review".to_string(),
            base_ref: "master".to_string(),
            head_ref: "feature".to_string(),
            branch: Some("feature".to_string()),
        })?;
        Ok((db, session.id))
    }

    #[test]
    fn add_comment_with_synthetic_diff_line_id_persists_with_null_anchor() -> Result<()> {
        let temp = tempdir()?;
        let (db, session_id) = fixture_session(temp.path())?;
        let id = db.add_comment(NewComment {
            session_id: session_id.clone(),
            file_path: "a.txt".to_string(),
            diff_line_id: Some("a.txt#h0#l0".to_string()),
            old_line: None,
            new_line: Some(2),
            range_start_old_line: None,
            range_start_new_line: Some(2),
            range_end_old_line: None,
            range_end_new_line: Some(2),
            selected_text: Some("+two".to_string()),
            body: "needs a check".to_string(),
            status: None,
            visibility: None,
        })?;
        assert!(!id.is_empty());
        let ctx = db.agent_context(&session_id)?;
        assert_eq!(ctx.open_comments.len(), 1);
        assert!(ctx.open_comments[0].diff_line_id.is_none());
        Ok(())
    }

    #[test]
    fn add_note_with_synthetic_source_diff_line_id_persists_with_null_anchor() -> Result<()> {
        let temp = tempdir()?;
        let (db, session_id) = fixture_session(temp.path())?;
        db.add_note(NewNote {
            repo_id: None,
            session_id: Some(session_id.clone()),
            title: "style".to_string(),
            body: "prefer explicit review intent".to_string(),
            note_type: "preference".to_string(),
            status: None,
            visibility: None,
            source_context: Some(SourceContext {
                file_path: "a.txt".to_string(),
                diff_line_id: Some("a.txt#h0#l0".to_string()),
                old_line: None,
                new_line: Some(2),
                line_kind: Some("add".to_string()),
                content: Some("+two".to_string()),
            }),
        })?;
        let ctx = db.agent_context(&session_id)?;
        assert_eq!(ctx.agent_visible_notes.len(), 1);
        let source = ctx.agent_visible_notes[0]
            .source_context
            .as_ref()
            .expect("source context");
        assert_eq!(source.file_path, "a.txt");
        assert!(source.diff_line_id.is_none());
        Ok(())
    }

    #[test]
    fn add_decision_with_synthetic_source_diff_line_id_persists_with_null_anchor() -> Result<()> {
        let temp = tempdir()?;
        let (db, session_id) = fixture_session(temp.path())?;
        db.add_decision(NewDecision {
            repo_id: None,
            session_id: Some(session_id.clone()),
            title: "opaque names".to_string(),
            context: "model drift".to_string(),
            decision: "treat names as opaque".to_string(),
            rationale: "provider-owned strings drift".to_string(),
            alternatives: None,
            consequences: None,
            status: None,
            visibility: None,
            source_context: Some(SourceContext {
                file_path: "a.txt".to_string(),
                diff_line_id: Some("a.txt#h0#l0".to_string()),
                old_line: None,
                new_line: Some(2),
                line_kind: Some("add".to_string()),
                content: Some("+two".to_string()),
            }),
        })?;
        let ctx = db.agent_context(&session_id)?;
        assert_eq!(ctx.accepted_decisions.len(), 1);
        let source = ctx.accepted_decisions[0]
            .source_context
            .as_ref()
            .expect("source context");
        assert_eq!(source.file_path, "a.txt");
        assert!(source.diff_line_id.is_none());
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
