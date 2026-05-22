//! Agent context and trajectory exports.

use crate::{
    CommentRecord, Database, DecisionRecord, FixImportRecord, NoteRecord, RepoRecord,
    ReviewSessionRecord, ReviewThreadRecord, SourceContext,
};
use anyhow::{Context, Result};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};

/// Agent-readable context for a review session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentContext {
    /// Repository record.
    pub repo: RepoRecord,
    /// Review session.
    pub session: ReviewSessionRecord,
    /// Open agent-visible comments.
    pub open_comments: Vec<CommentRecord>,
    /// Open agent-visible inline threads.
    pub open_threads: Vec<ReviewThreadRecord>,
    /// Accepted agent-visible decisions.
    pub accepted_decisions: Vec<DecisionRecord>,
    /// Agent-visible notes.
    pub agent_visible_notes: Vec<NoteRecord>,
}

/// Training/export trajectory record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrajectoryRecord {
    /// Review session.
    pub session: ReviewSessionRecord,
    /// Comment that drove the fix.
    pub comment: Option<CommentRecord>,
    /// Accepted decisions available for the fix.
    pub decisions: Vec<DecisionRecord>,
    /// Accepted fix import.
    pub fix: FixImportRecord,
}

/// Build agent context.
///
/// # Errors
/// Returns an error when database reads fail or records are missing.
pub fn agent_context(db: &Database, session_id: &str) -> Result<AgentContext> {
    let conn = db.export_connection()?;
    let session: ReviewSessionRecord = conn
        .query_row(
            "select id, repo_id, title, base_ref, head_ref, branch, base_sha, head_sha, status, created_at, updated_at
             from review_sessions where id = ?1",
            [session_id],
            |row| {
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
            },
        )
        .optional()?
        .with_context(|| format!("session not found: {session_id}"))?;
    let repo: RepoRecord = conn
        .query_row(
            "select id, name, path, created_at, updated_at from repos where id = ?1",
            [&session.repo_id],
            |row| {
                Ok(RepoRecord {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    path: row.get(2)?,
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
                })
            },
        )
        .optional()?
        .with_context(|| format!("repo not found: {}", session.repo_id))?;

    Ok(AgentContext {
        repo,
        session,
        open_comments: comments(
            &conn,
            session_id,
            "status = 'open' and visibility = 'agent'",
        )?,
        open_threads: db
            .review_threads(session_id)?
            .into_iter()
            .filter(|thread| thread.status == "open" && thread.visibility == "agent")
            .collect(),
        accepted_decisions: decisions(
            &conn,
            session_id,
            "status = 'accepted' and visibility = 'agent'",
        )?,
        agent_visible_notes: notes(&conn, session_id, "visibility = 'agent'")?,
    })
}

/// Export accepted trajectory records.
///
/// # Errors
/// Returns an error when database reads fail.
pub fn trajectory_records(db: &Database, repo_id: Option<&str>) -> Result<Vec<TrajectoryRecord>> {
    let conn = db.export_connection()?;
    let sql = if repo_id.is_some() {
        "select fi.id, fi.session_id, fi.commit_sha, fi.diff_patch, fi.agent_name, fi.response_text, fi.tests_json, fi.accepted, fi.created_at
         from fix_imports fi
         join review_sessions rs on rs.id = fi.session_id
         where fi.accepted = 1 and rs.repo_id = ?1
         order by fi.created_at"
    } else {
        "select fi.id, fi.session_id, fi.commit_sha, fi.diff_patch, fi.agent_name, fi.response_text, fi.tests_json, fi.accepted, fi.created_at
         from fix_imports fi
         where fi.accepted = 1
         order by fi.created_at"
    };
    let mut stmt = conn.prepare(sql)?;
    let fixes = if let Some(repo_id) = repo_id {
        stmt.query_map([repo_id], map_fix)?
            .collect::<rusqlite::Result<Vec<_>>>()?
    } else {
        stmt.query_map([], map_fix)?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };

    let mut records = Vec::new();
    for fix in fixes {
        let session = session(&conn, &fix.session_id)?;
        let mut open = comments(&conn, &fix.session_id, "visibility = 'agent'")?;
        records.push(TrajectoryRecord {
            decisions: decisions(&conn, &fix.session_id, "visibility = 'agent'")?,
            comment: open.pop(),
            session,
            fix,
        });
    }
    Ok(records)
}

fn session(conn: &rusqlite::Connection, id: &str) -> Result<ReviewSessionRecord> {
    conn.query_row(
        "select id, repo_id, title, base_ref, head_ref, branch, base_sha, head_sha, status, created_at, updated_at
         from review_sessions where id = ?1",
        [id],
        |row| {
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
        },
    )
    .map_err(Into::into)
}

fn comments(
    conn: &rusqlite::Connection,
    session_id: &str,
    filter: &str,
) -> Result<Vec<CommentRecord>> {
    let mut stmt = conn.prepare(&format!(
        "select id, session_id, file_path, diff_line_id, old_line, new_line, range_start_old_line, range_start_new_line, range_end_old_line, range_end_new_line, selected_text, body, status, visibility, created_at, updated_at
         from comments where session_id = ?1 and {filter} order by created_at"
    ))?;
    let rows = stmt.query_map([session_id], |row| {
        Ok(CommentRecord {
            id: row.get(0)?,
            session_id: row.get(1)?,
            file_path: row.get(2)?,
            diff_line_id: row.get(3)?,
            old_line: row.get(4)?,
            new_line: row.get(5)?,
            range_start_old_line: row.get(6)?,
            range_start_new_line: row.get(7)?,
            range_end_old_line: row.get(8)?,
            range_end_new_line: row.get(9)?,
            selected_text: row.get(10)?,
            body: row.get(11)?,
            status: row.get(12)?,
            visibility: row.get(13)?,
            created_at: row.get(14)?,
            updated_at: row.get(15)?,
        })
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

fn notes(conn: &rusqlite::Connection, session_id: &str, filter: &str) -> Result<Vec<NoteRecord>> {
    let mut stmt = conn.prepare(&format!(
        "select id, repo_id, session_id, title, body, note_type, status, visibility, source_file_path, source_diff_line_id, source_old_line, source_new_line, source_line_kind, source_content, created_at, updated_at
         from notes where session_id = ?1 and {filter} order by created_at"
    ))?;
    let rows = stmt.query_map([session_id], |row| {
        Ok(NoteRecord {
            id: row.get(0)?,
            repo_id: row.get(1)?,
            session_id: row.get(2)?,
            title: row.get(3)?,
            body: row.get(4)?,
            note_type: row.get(5)?,
            status: row.get(6)?,
            visibility: row.get(7)?,
            source_context: source_context(row, 8)?,
            created_at: row.get(14)?,
            updated_at: row.get(15)?,
        })
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

fn decisions(
    conn: &rusqlite::Connection,
    session_id: &str,
    filter: &str,
) -> Result<Vec<DecisionRecord>> {
    let mut stmt = conn.prepare(&format!(
        "select id, repo_id, session_id, title, context, decision, rationale, alternatives, consequences, status, visibility, source_file_path, source_diff_line_id, source_old_line, source_new_line, source_line_kind, source_content, created_at, updated_at
         from decisions where session_id = ?1 and {filter} order by created_at"
    ))?;
    let rows = stmt.query_map([session_id], |row| {
        Ok(DecisionRecord {
            id: row.get(0)?,
            repo_id: row.get(1)?,
            session_id: row.get(2)?,
            title: row.get(3)?,
            context: row.get(4)?,
            decision: row.get(5)?,
            rationale: row.get(6)?,
            alternatives: row.get(7)?,
            consequences: row.get(8)?,
            status: row.get(9)?,
            visibility: row.get(10)?,
            source_context: source_context(row, 11)?,
            created_at: row.get(17)?,
            updated_at: row.get(18)?,
        })
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

fn source_context(
    row: &rusqlite::Row<'_>,
    start: usize,
) -> rusqlite::Result<Option<SourceContext>> {
    let file_path: Option<String> = row.get(start)?;
    Ok(file_path.map(|file_path| SourceContext {
        file_path,
        diff_line_id: row.get(start + 1).ok().flatten(),
        old_line: row.get(start + 2).ok().flatten(),
        new_line: row.get(start + 3).ok().flatten(),
        line_kind: row.get(start + 4).ok().flatten(),
        content: row.get(start + 5).ok().flatten(),
    }))
}

fn map_fix(row: &rusqlite::Row<'_>) -> rusqlite::Result<FixImportRecord> {
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
}
