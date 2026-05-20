//! SQLite persistence.

use crate::{
    ActiveReviewSessionRecord, FileRecord, FixImportRecord, GitRepo, RepoRecord, ReviewDiffLine,
    ReviewFileDiff, ReviewHunk, ReviewSessionRecord, SourceContext,
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
        let conn = self.connect()?;
        let id = new_id();
        conn.execute(
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
        "insert or ignore into schema_migrations(version, applied_at) values(2, ?1)",
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

#[cfg(test)]
mod tests {
    use super::{
        Database, NewComment, NewDecision, NewNote, NewRepo, NewReviewSession, SourceContext,
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

        let db = Database::new(temp.path().join("mind.db"));
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
        let context = db.agent_context(&session.id)?;
        assert_eq!(context.open_comments.len(), 1);
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
}
