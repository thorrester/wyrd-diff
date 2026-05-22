//! Shared domain models.

use serde::{Deserialize, Serialize};

/// Local git repository registered in Wyrd Diff.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoRecord {
    /// Stable database id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Absolute repository path.
    pub path: String,
    /// Creation timestamp.
    pub created_at: String,
    /// Last update timestamp.
    pub updated_at: String,
}

/// Review session over a base/head ref pair.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewSessionRecord {
    /// Stable database id.
    pub id: String,
    /// Repository id.
    pub repo_id: String,
    /// Display title.
    pub title: String,
    /// Base ref.
    pub base_ref: String,
    /// Head ref.
    pub head_ref: String,
    /// Branch this review targets. Used as the join key for agent sessions.
    /// Defaults to `head_ref` when not set explicitly.
    pub branch: Option<String>,
    /// Resolved base sha.
    pub base_sha: String,
    /// Resolved head sha.
    pub head_sha: String,
    /// Session status.
    pub status: String,
    /// Creation timestamp.
    pub created_at: String,
    /// Last update timestamp.
    pub updated_at: String,
}

/// Active review session selected for a repository + branch tuple.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveReviewSessionRecord {
    /// Repository record.
    pub repo: RepoRecord,
    /// Branch this active session covers.
    pub branch: String,
    /// Active review session.
    pub session: ReviewSessionRecord,
    /// Baseline sha used by agent recording hooks.
    pub start_sha: String,
    /// Last activation timestamp.
    pub updated_at: String,
}

/// Live or recently active coding-agent session attached to a repo + branch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSessionRecord {
    /// Stable database id.
    pub id: String,
    /// Harness-provided session id (Claude/Codex/etc.). May repeat across agents.
    pub agent_session_id: String,
    /// Agent name, e.g. "claude" or "codex".
    pub agent_name: String,
    /// Repo this agent is attached to. Null when registered outside a known repo.
    pub repo_id: Option<String>,
    /// Branch at registration time. Null on detached HEAD or non-git contexts.
    pub branch: Option<String>,
    /// Linked review session id resolved by (repo, branch) at lookup time.
    pub review_session_id: Option<String>,
    /// Registration timestamp.
    pub started_at: String,
    /// Last hook ping timestamp.
    pub last_activity_at: String,
    /// Explicit end timestamp, when known.
    pub ended_at: Option<String>,
}

/// Aggregated entry for one (repo, branch) active review used by the fleet view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverviewEntry {
    /// Repository.
    pub repo: RepoRecord,
    /// Branch this entry covers.
    pub branch: String,
    /// Active review session for the branch.
    pub session: ReviewSessionRecord,
    /// Open thread count.
    pub open_thread_count: i64,
    /// Threads with new reviewer input awaiting agent pull.
    pub pending_thread_count: i64,
    /// Open threads whose latest visible message is from an agent and is
    /// therefore awaiting reviewer action. Heuristic — no human read
    /// watermark exists yet.
    pub agent_reply_thread_count: i64,
    /// Agent sessions seen on this branch within the idle window.
    pub agent_sessions: Vec<AgentSessionRecord>,
    /// Last activation timestamp.
    pub updated_at: String,
}

/// Commit captured in a review session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitRecord {
    /// Stable database id.
    pub id: String,
    /// Review session id.
    pub session_id: String,
    /// Full commit sha.
    pub sha: String,
    /// Short commit sha.
    pub short_sha: String,
    /// Commit subject.
    pub subject: String,
    /// Commit body.
    pub body: Option<String>,
    /// Author name.
    pub author_name: Option<String>,
    /// Author email.
    pub author_email: Option<String>,
    /// Author timestamp.
    pub authored_at: Option<String>,
}

/// File captured in a review session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileRecord {
    /// Stable database id.
    pub id: String,
    /// Review session id.
    pub session_id: String,
    /// New path.
    pub path: String,
    /// Old path for renames.
    pub old_path: Option<String>,
    /// Git file status.
    pub status: String,
    /// Added line count.
    pub additions: i64,
    /// Deleted line count.
    pub deletions: i64,
    /// Review state.
    pub review_state: String,
}

/// Hunk and lines for UI rendering.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewFileDiff {
    /// File metadata.
    pub file: FileRecord,
    /// Hunks in display order.
    pub hunks: Vec<ReviewHunk>,
}

/// Review hunk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewHunk {
    /// Stable database id.
    pub id: String,
    /// Old start.
    pub old_start: Option<i64>,
    /// Old line count.
    pub old_lines: Option<i64>,
    /// New start.
    pub new_start: Option<i64>,
    /// New line count.
    pub new_lines: Option<i64>,
    /// Raw hunk patch.
    pub patch: String,
    /// Parsed diff lines.
    pub lines: Vec<ReviewDiffLine>,
}

/// Review diff line.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewDiffLine {
    /// Stable database id.
    pub id: String,
    /// File path.
    pub file_path: String,
    /// Old-side line number.
    pub old_line: Option<i64>,
    /// New-side line number.
    pub new_line: Option<i64>,
    /// Line kind.
    pub line_kind: String,
    /// Content.
    pub content: String,
}

/// Optional source location captured with a note or decision.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceContext {
    /// File path visible when the record was created.
    pub file_path: String,
    /// Diff line id, when the source is anchored to a parsed diff line.
    pub diff_line_id: Option<String>,
    /// Old-side line number.
    pub old_line: Option<i64>,
    /// New-side line number.
    pub new_line: Option<i64>,
    /// Diff line kind.
    pub line_kind: Option<String>,
    /// Diff line content.
    pub content: Option<String>,
}

/// Inline review thread anchored to a diff line or range.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewThreadRecord {
    /// Stable database id.
    pub id: String,
    /// Review session id.
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
    pub range_start_old_line: Option<i64>,
    /// New-side start line for a selected range.
    pub range_start_new_line: Option<i64>,
    /// Old-side end line for a selected range.
    pub range_end_old_line: Option<i64>,
    /// New-side end line for a selected range.
    pub range_end_new_line: Option<i64>,
    /// Selected diff text for the thread.
    pub selected_text: Option<String>,
    /// Thread status.
    pub status: String,
    /// Visibility policy.
    pub visibility: String,
    /// Creation timestamp.
    pub created_at: String,
    /// Last update timestamp.
    pub updated_at: String,
    /// Last thread message id delivered to an agent (delivery watermark).
    pub last_delivered_message_id: Option<String>,
    /// Messages attached to this thread.
    pub messages: Vec<ThreadMessageRecord>,
}

/// Message inside an inline review thread.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadMessageRecord {
    /// Stable database id.
    pub id: String,
    /// Parent thread id.
    pub thread_id: String,
    /// Author kind, such as human or agent.
    pub author_kind: String,
    /// Optional author display name.
    pub author_name: Option<String>,
    /// Message type: comment, thinking_note, decision, agent_instruction, agent_response.
    pub message_type: String,
    /// Message body.
    pub body: String,
    /// Message status.
    pub status: String,
    /// Visibility policy.
    pub visibility: String,
    /// Linked fix import id, when the message came from an agent trajectory import.
    pub fix_import_id: Option<String>,
    /// Creation timestamp.
    pub created_at: String,
    /// Last update timestamp.
    pub updated_at: String,
}

/// Freeform engineering note.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoteRecord {
    /// Stable database id.
    pub id: String,
    /// Optional repository id.
    pub repo_id: Option<String>,
    /// Optional review session id.
    pub session_id: Option<String>,
    /// Note title.
    pub title: String,
    /// Note body.
    pub body: String,
    /// Note type.
    pub note_type: String,
    /// Note status.
    pub status: String,
    /// Visibility policy.
    pub visibility: String,
    /// Optional source location active when the note was recorded.
    pub source_context: Option<SourceContext>,
    /// Creation timestamp.
    pub created_at: String,
    /// Last update timestamp.
    pub updated_at: String,
}

/// Structured trajectory decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionRecord {
    /// Stable database id.
    pub id: String,
    /// Optional repository id.
    pub repo_id: Option<String>,
    /// Optional review session id.
    pub session_id: Option<String>,
    /// Decision title.
    pub title: String,
    /// Decision context.
    pub context: String,
    /// Decision text.
    pub decision: String,
    /// Decision rationale.
    pub rationale: String,
    /// Alternatives considered.
    pub alternatives: Option<String>,
    /// Consequences.
    pub consequences: Option<String>,
    /// Decision status.
    pub status: String,
    /// Visibility policy.
    pub visibility: String,
    /// Optional source location active when the decision was recorded.
    pub source_context: Option<SourceContext>,
    /// Creation timestamp.
    pub created_at: String,
    /// Last update timestamp.
    pub updated_at: String,
}

/// Feedback batch dispatched to an agent for resolution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackBatchRecord {
    /// Stable database id.
    pub id: String,
    /// Review session id.
    pub session_id: String,
    /// Optional agent session id captured at dispatch.
    pub agent_session_id: Option<String>,
    /// Status: pending | delivered | superseded.
    pub status: String,
    /// Markdown payload prepared for the agent.
    pub payload: String,
    /// Number of threads included in this batch.
    pub thread_count: i64,
    /// Creation timestamp.
    pub created_at: String,
    /// Delivery timestamp.
    pub delivered_at: Option<String>,
    /// Threads included in this batch.
    pub threads: Vec<FeedbackBatchThreadRecord>,
}

/// Thread snapshot captured in a feedback batch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackBatchThreadRecord {
    /// Parent batch id.
    pub batch_id: String,
    /// Review thread id.
    pub thread_id: String,
    /// Delivery kind: full | delta.
    pub delivery_kind: String,
    /// Thread message ids included in this snapshot.
    pub message_ids: Vec<String>,
}

/// Imported fix commit and test result payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixImportRecord {
    /// Stable database id.
    pub id: String,
    /// Review session id.
    pub session_id: String,
    /// Fix commit sha.
    pub commit_sha: String,
    /// Fix diff.
    pub diff_patch: String,
    /// Agent name or runtime that produced the fix.
    pub agent_name: Option<String>,
    /// Agent response or completion transcript.
    pub response_text: Option<String>,
    /// Test results JSON.
    pub tests_json: Option<String>,
    /// Whether the fix was accepted.
    pub accepted: bool,
    /// Creation timestamp.
    pub created_at: String,
}
