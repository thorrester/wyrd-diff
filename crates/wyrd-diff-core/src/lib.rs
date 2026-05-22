//! Core persistence, git, and export logic for Wyrd Diff.

#![forbid(unsafe_code)]

pub mod agent_config;
pub mod db;
pub mod export;
pub mod git;
pub mod models;

pub use db::{
    Database, NewAgentSession, NewComment, NewDecision, NewFixImport, NewNote, NewRepo,
    NewReviewSession, NewReviewThread, NewThreadMessage,
};
pub use export::{AgentContext, TrajectoryRecord};
pub use git::{DiffFile, DiffHunk, DiffLine, GitRepo, RepoCandidate, scan_repos};
pub use models::*;
