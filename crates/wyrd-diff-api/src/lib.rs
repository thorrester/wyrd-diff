//! Local API for Wyrd Diff.

#![forbid(unsafe_code)]

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, sync::Arc};
use tower_http::cors::CorsLayer;
use wyrd_diff_core::{
    Database, NewComment, NewDecision, NewFixImport, NewNote, NewRepo, NewReviewSession,
    NewReviewThread, NewThreadMessage,
};

/// Shared API state.
#[derive(Clone)]
pub struct ApiState {
    db: Database,
    token: String,
    mcp_url: String,
}

impl ApiState {
    /// Build shared state for tests and embedders.
    #[must_use]
    pub fn new(db: Database, token: String, mcp_url: String) -> Self {
        Self { db, token, mcp_url }
    }
}

/// API error response.
#[derive(Debug, Serialize)]
pub struct ApiError {
    code: String,
    message: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (StatusCode::BAD_REQUEST, Json(self)).into_response()
    }
}

/// API result alias.
pub type ApiResult<T> = Result<T, ApiError>;

/// Serve the local API.
///
/// # Errors
/// Returns an error when the listener cannot bind or the server fails.
pub async fn serve(db: Database, token: String, addr: SocketAddr) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let bound = listener.local_addr()?;
    serve_on(db, token, listener, mcp_url_for(bound)).await
}

/// Bind on `addr`. If the port is taken, walk forward up to `max_attempts - 1`
/// extra ports before giving up. Returns the bound listener and the actual
/// address used.
///
/// # Errors
/// Returns an error when every port in the window is in use, or the OS
/// rejects the bind for a reason other than `AddrInUse`.
pub async fn bind_with_fallback(
    addr: SocketAddr,
    max_attempts: u16,
) -> anyhow::Result<(tokio::net::TcpListener, SocketAddr)> {
    let base = addr.port();
    let mut last_err = None;
    for offset in 0..max_attempts {
        let port = base.saturating_add(offset);
        let candidate = SocketAddr::new(addr.ip(), port);
        match tokio::net::TcpListener::bind(candidate).await {
            Ok(listener) => return Ok((listener, candidate)),
            Err(err) if err.kind() == std::io::ErrorKind::AddrInUse => {
                last_err = Some(err);
                continue;
            }
            Err(err) => return Err(err.into()),
        }
    }
    Err(anyhow::anyhow!(
        "no free port in window {base}..{} ({})",
        base + max_attempts,
        last_err
            .map(|e| e.to_string())
            .unwrap_or_else(|| "unknown".to_string())
    ))
}

/// Serve the local API on a pre-bound listener with an explicit MCP URL.
///
/// # Errors
/// Returns an error when the server task fails.
pub async fn serve_on(
    db: Database,
    token: String,
    listener: tokio::net::TcpListener,
    mcp_url: String,
) -> anyhow::Result<()> {
    let state = Arc::new(ApiState { db, token, mcp_url });
    let app = router(state);
    axum::serve(listener, app).await?;
    Ok(())
}

/// Build the canonical MCP URL for a bound socket.
pub fn mcp_url_for(addr: SocketAddr) -> String {
    format!("http://{addr}/mcp")
}

/// Build an API router.
pub fn router(state: Arc<ApiState>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/api/repos", get(repos).post(upsert_repo))
        .route(
            "/api/review-sessions",
            get(review_sessions).post(create_review_session),
        )
        .route("/api/review-sessions/:id", get(review_session))
        .route("/api/review-sessions/:id/diff", get(review_diff_summary))
        .route("/api/review-sessions/:id/diff/file", get(review_diff_file))
        .route(
            "/api/review-sessions/:id/refresh",
            post(refresh_review_session),
        )
        .route(
            "/api/review-sessions/:id/threads",
            get(review_threads).post(add_review_thread),
        )
        .route("/api/review-threads/:id", delete(delete_review_thread))
        .route("/api/review-threads/:id/messages", post(add_thread_message))
        .route("/api/review-threads/:id/resolve", post(resolve_thread))
        .route("/api/review-threads/:id/reopen", post(reopen_thread))
        .route("/api/review-sessions/:id/comments", post(add_comment))
        .route("/api/review-sessions/:id/notes", post(add_note))
        .route("/api/review-sessions/:id/decisions", post(add_decision))
        .route(
            "/api/review-sessions/:id/fix-imports",
            get(fix_imports).post(import_fix),
        )
        .route(
            "/api/review-sessions/:id/feedback-batches",
            get(list_feedback_batches).post(create_feedback_batch),
        )
        .route("/api/review-sessions/:id/agent-context", get(agent_context))
        .route(
            "/api/review-sessions/:id/agent-sessions",
            get(agent_sessions_for_review),
        )
        .route("/api/overview", get(overview))
        .route(
            "/api/settings/home-dir",
            get(get_home_dir).put(put_home_dir),
        )
        .route("/api/repo-scan", get(repo_scan))
        .route("/api/repo-branches", get(repo_branches))
        .route("/api/export/trajectory.jsonl", get(export_trajectory))
        .route("/mcp", post(mcp_endpoint).get(mcp_get))
        .route("/api/configure-agents", post(configure_agents_endpoint))
        .route("/api/configure-agents/:id", post(configure_agent_endpoint))
        .route("/api/status", get(status))
        .route("/api/agent-status", get(agent_status_endpoint))
        .with_state(state)
        .layer(CorsLayer::permissive())
}

async fn status(State(state): State<Arc<ApiState>>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "ok": true,
        "mcp_url": state.mcp_url,
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

async fn agent_status_endpoint(
    State(state): State<Arc<ApiState>>,
) -> ApiResult<Json<serde_json::Value>> {
    let mcp_url = state.mcp_url.clone();
    let statuses = run_blocking(move || wyrd_diff_core::agent_config::agent_status(&mcp_url))
        .await
        .map_err(|e| ApiError {
            code: "agent_status_failed".to_string(),
            message: e.message,
        })?;
    let results: Vec<serde_json::Value> = statuses
        .into_iter()
        .map(|s| {
            serde_json::json!({
                "id": s.id,
                "display": s.display,
                "path": s.path.display().to_string(),
                "detected": s.detected,
                "state": match s.state {
                    wyrd_diff_core::agent_config::AgentConfigState::Ok => "ok",
                    wyrd_diff_core::agent_config::AgentConfigState::Mismatch => "mismatch",
                    wyrd_diff_core::agent_config::AgentConfigState::Missing => "missing",
                    wyrd_diff_core::agent_config::AgentConfigState::NoFile => "no_file",
                },
                "configured_url": s.configured_url,
                "snippet": s.snippet,
            })
        })
        .collect();
    Ok(Json(serde_json::json!({
        "mcp_url": state.mcp_url,
        "agents": results,
    })))
}

async fn configure_agents_endpoint(
    State(state): State<Arc<ApiState>>,
) -> ApiResult<Json<serde_json::Value>> {
    let url = state.mcp_url.clone();
    let url_for_task = url.clone();
    let writes =
        run_blocking(move || wyrd_diff_core::agent_config::configure_agents(&url_for_task))
            .await
            .map_err(|e| ApiError {
                code: "configure_agents_failed".to_string(),
                message: e.message,
            })?;
    let results: Vec<serde_json::Value> = writes
        .into_iter()
        .map(|w| {
            serde_json::json!({
                "id": w.id,
                "display": w.display,
                "path": w.path.display().to_string(),
                "action": match w.action {
                    wyrd_diff_core::agent_config::ConfigAction::Created => "created",
                    wyrd_diff_core::agent_config::ConfigAction::Updated => "updated",
                    wyrd_diff_core::agent_config::ConfigAction::Unchanged => "unchanged",
                }
            })
        })
        .collect();
    Ok(Json(serde_json::json!({ "url": url, "results": results })))
}

async fn configure_agent_endpoint(
    State(state): State<Arc<ApiState>>,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let url = state.mcp_url.clone();
    let url_for_task = url.clone();
    let write =
        run_blocking(move || wyrd_diff_core::agent_config::configure_agent(&id, &url_for_task))
            .await
            .map_err(|e| ApiError {
                code: "configure_agent_failed".to_string(),
                message: e.message,
            })?;
    Ok(Json(serde_json::json!({
        "url": url,
        "result": {
            "id": write.id,
            "display": write.display,
            "path": write.path.display().to_string(),
            "action": match write.action {
                wyrd_diff_core::agent_config::ConfigAction::Created => "created",
                wyrd_diff_core::agent_config::ConfigAction::Updated => "updated",
                wyrd_diff_core::agent_config::ConfigAction::Unchanged => "unchanged",
            },
        }
    })))
}

async fn mcp_endpoint(
    State(state): State<Arc<ApiState>>,
    Json(request): Json<serde_json::Value>,
) -> Response {
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || wyrd_diff_mcp::dispatch(&db, request)).await;
    match result {
        Ok(Some(response)) => Json(response).into_response(),
        // JSON-RPC notifications have no response, but Codex's bundled
        // rmcp 0.15 client unconditionally `serde_json::from_slice`s the
        // response body and chokes on an empty 202. Return 200 OK with an
        // empty JSON object so the handshake completes.
        Ok(None) => Json(serde_json::json!({})).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

async fn mcp_get() -> Response {
    (StatusCode::METHOD_NOT_ALLOWED, "MCP requires POST").into_response()
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "ok": true }))
}

async fn repos(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
) -> ApiResult<Json<serde_json::Value>> {
    authorize(&state, &headers)?;
    let db = state.db.clone();
    let repos = run_blocking(move || db.repos()).await?;
    Ok(Json(serde_json::json!({ "repos": repos })))
}

async fn upsert_repo(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Json(input): Json<NewRepo>,
) -> ApiResult<Json<serde_json::Value>> {
    authorize(&state, &headers)?;
    let db = state.db.clone();
    let repo = run_blocking(move || db.upsert_repo(input)).await?;
    Ok(Json(serde_json::json!({ "repo": repo })))
}

async fn review_sessions(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
) -> ApiResult<Json<serde_json::Value>> {
    authorize(&state, &headers)?;
    let db = state.db.clone();
    let sessions = run_blocking(move || db.review_sessions()).await?;
    Ok(Json(serde_json::json!({ "review_sessions": sessions })))
}

async fn create_review_session(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Json(input): Json<NewReviewSession>,
) -> ApiResult<Json<serde_json::Value>> {
    authorize(&state, &headers)?;
    let db = state.db.clone();
    let session = run_blocking(move || db.create_review_session(input)).await?;
    Ok(Json(serde_json::json!({ "review_session": session })))
}

async fn review_session(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    authorize(&state, &headers)?;
    let db = state.db.clone();
    let session = run_blocking(move || db.review_session(&id)).await?;
    Ok(Json(serde_json::json!({ "review_session": session })))
}

async fn review_diff_summary(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    authorize(&state, &headers)?;
    let db = state.db.clone();
    let files = run_blocking(move || db.review_diff_summary(&id)).await?;
    Ok(Json(serde_json::json!({ "files": files })))
}

#[derive(Debug, Deserialize)]
struct DiffFileQuery {
    path: String,
}

async fn review_diff_file(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    axum::extract::Query(query): axum::extract::Query<DiffFileQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    authorize(&state, &headers)?;
    let db = state.db.clone();
    let file_path = query.path;
    let file = run_blocking(move || db.review_diff_file(&id, &file_path)).await?;
    Ok(Json(serde_json::json!({ "file": file })))
}

async fn refresh_review_session(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    authorize(&state, &headers)?;
    let db = state.db.clone();
    let session = run_blocking(move || db.refresh_review_session(&id)).await?;
    Ok(Json(serde_json::json!({ "review_session": session })))
}

async fn add_comment(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(mut input): Json<NewComment>,
) -> ApiResult<Json<serde_json::Value>> {
    authorize(&state, &headers)?;
    input.session_id = id;
    let db = state.db.clone();
    let comment_id = run_blocking(move || db.add_comment(input)).await?;
    Ok(Json(serde_json::json!({ "id": comment_id })))
}

async fn review_threads(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    authorize(&state, &headers)?;
    let db = state.db.clone();
    let threads = run_blocking(move || db.review_threads(&id)).await?;
    Ok(Json(serde_json::json!({ "threads": threads })))
}

async fn add_review_thread(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(mut input): Json<NewReviewThread>,
) -> ApiResult<Json<serde_json::Value>> {
    authorize(&state, &headers)?;
    input.session_id = id;
    let db = state.db.clone();
    let thread = run_blocking(move || db.add_review_thread(input)).await?;
    Ok(Json(serde_json::json!({ "thread": thread })))
}

async fn delete_review_thread(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    authorize(&state, &headers)?;
    let db = state.db.clone();
    let removed = run_blocking(move || db.delete_review_thread(&id)).await?;
    Ok(Json(serde_json::json!({ "deleted": removed })))
}

async fn resolve_thread(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    authorize(&state, &headers)?;
    let db = state.db.clone();
    let updated = run_blocking(move || db.resolve_thread(&id)).await?;
    Ok(Json(
        serde_json::json!({ "updated": updated, "status": "resolved" }),
    ))
}

async fn reopen_thread(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    authorize(&state, &headers)?;
    let db = state.db.clone();
    let updated = run_blocking(move || db.reopen_thread(&id)).await?;
    Ok(Json(
        serde_json::json!({ "updated": updated, "status": "open" }),
    ))
}

#[derive(Debug, Deserialize, Default)]
struct CreateFeedbackBatchInput {
    #[serde(default)]
    agent_session_id: Option<String>,
}

async fn create_feedback_batch(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Option<Json<CreateFeedbackBatchInput>>,
) -> ApiResult<Json<serde_json::Value>> {
    authorize(&state, &headers)?;
    let input = body.map(|Json(value)| value).unwrap_or_default();
    let db = state.db.clone();
    let batch = run_blocking(move || db.create_feedback_batch(&id, input.agent_session_id)).await?;
    Ok(Json(serde_json::json!({ "batch": batch })))
}

async fn list_feedback_batches(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    authorize(&state, &headers)?;
    let db = state.db.clone();
    let batches = run_blocking(move || db.feedback_batches(&id)).await?;
    Ok(Json(serde_json::json!({ "batches": batches })))
}

async fn add_thread_message(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(mut input): Json<NewThreadMessage>,
) -> ApiResult<Json<serde_json::Value>> {
    authorize(&state, &headers)?;
    input.thread_id = id;
    let db = state.db.clone();
    let message = run_blocking(move || db.add_thread_message(input)).await?;
    Ok(Json(serde_json::json!({ "message": message })))
}

async fn add_note(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(mut input): Json<NewNote>,
) -> ApiResult<Json<serde_json::Value>> {
    authorize(&state, &headers)?;
    input.session_id = Some(id);
    let db = state.db.clone();
    let note_id = run_blocking(move || db.add_note(input)).await?;
    Ok(Json(serde_json::json!({ "id": note_id })))
}

async fn add_decision(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(mut input): Json<NewDecision>,
) -> ApiResult<Json<serde_json::Value>> {
    authorize(&state, &headers)?;
    input.session_id = Some(id);
    let db = state.db.clone();
    let decision_id = run_blocking(move || db.add_decision(input)).await?;
    Ok(Json(serde_json::json!({ "id": decision_id })))
}

async fn import_fix(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(mut input): Json<NewFixImport>,
) -> ApiResult<Json<serde_json::Value>> {
    authorize(&state, &headers)?;
    input.session_id = id;
    let db = state.db.clone();
    let fix_id = run_blocking(move || db.import_fix(input)).await?;
    Ok(Json(serde_json::json!({ "id": fix_id })))
}

async fn fix_imports(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    authorize(&state, &headers)?;
    let db = state.db.clone();
    let imports = run_blocking(move || db.fix_imports(&id)).await?;
    Ok(Json(serde_json::json!({ "fix_imports": imports })))
}

async fn agent_context(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    authorize(&state, &headers)?;
    let db = state.db.clone();
    let context = run_blocking(move || db.agent_context(&id)).await?;
    Ok(Json(serde_json::json!(context)))
}

#[derive(Debug, Deserialize)]
struct AgentSessionsQuery {
    idle_seconds: Option<i64>,
}

async fn agent_sessions_for_review(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    axum::extract::Query(query): axum::extract::Query<AgentSessionsQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    authorize(&state, &headers)?;
    let cutoff = query.idle_seconds.unwrap_or(24 * 3600);
    let db = state.db.clone();
    let sessions = run_blocking(move || db.agent_sessions_for_review(&id, cutoff)).await?;
    Ok(Json(serde_json::json!({ "agent_sessions": sessions })))
}

#[derive(Debug, Deserialize)]
struct HomeDirInput {
    path: String,
}

async fn get_home_dir(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
) -> ApiResult<Json<serde_json::Value>> {
    authorize(&state, &headers)?;
    let (view, settings_path) = run_blocking(move || {
        let view = wyrd_diff_core::agent_config::home_dir_view_env()?;
        let settings_path = wyrd_diff_core::agent_config::user_config_path_env()
            .ok()
            .map(|p| p.to_string_lossy().to_string());
        Ok((view, settings_path))
    })
    .await?;
    Ok(Json(serde_json::json!({
        "path": view.home_dir,
        "inferred": view.inferred,
        "settings_path": settings_path,
    })))
}

async fn put_home_dir(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Json(input): Json<HomeDirInput>,
) -> ApiResult<Json<serde_json::Value>> {
    authorize(&state, &headers)?;
    let raw = input.path;
    let expanded = run_blocking(move || {
        let expanded = wyrd_diff_core::agent_config::expand_user_path_env(&raw);
        wyrd_diff_core::agent_config::set_home_dir_env(&expanded)?;
        Ok(expanded)
    })
    .await?;
    Ok(Json(serde_json::json!({
        "path": expanded,
        "inferred": false,
    })))
}

#[derive(Debug, Deserialize)]
struct RepoScanQuery {
    path: Option<String>,
}

async fn repo_scan(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    axum::extract::Query(query): axum::extract::Query<RepoScanQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    authorize(&state, &headers)?;
    let query_path = query.path;
    let (home, repos) = run_blocking(move || {
        let home_raw = match query_path {
            Some(p) if !p.is_empty() => p,
            _ => {
                let view = wyrd_diff_core::agent_config::home_dir_view_env()?;
                view.home_dir.ok_or_else(|| {
                    anyhow::anyhow!("home_dir is not configured and could not be inferred")
                })?
            }
        };
        let home = wyrd_diff_core::agent_config::expand_user_path_env(&home_raw);
        let repos = wyrd_diff_core::scan_repos(std::path::Path::new(&home))?;
        Ok((home, repos))
    })
    .await?;
    Ok(Json(serde_json::json!({ "home": home, "repos": repos })))
}

#[derive(Debug, Deserialize)]
struct RepoBranchesQuery {
    path: String,
}

async fn repo_branches(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    axum::extract::Query(query): axum::extract::Query<RepoBranchesQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    authorize(&state, &headers)?;
    let raw_path = query.path;
    let (branches, default_branch) = run_blocking(move || {
        let path = wyrd_diff_core::agent_config::expand_user_path_env(&raw_path);
        let repo = wyrd_diff_core::GitRepo::open(&path)?;
        let branches = repo.branches()?;
        let default_branch = repo.default_branch()?;
        Ok((branches, default_branch))
    })
    .await?;
    Ok(Json(serde_json::json!({
        "branches": branches,
        "default_branch": default_branch,
    })))
}

async fn overview(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    axum::extract::Query(query): axum::extract::Query<AgentSessionsQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    authorize(&state, &headers)?;
    let cutoff = query.idle_seconds.unwrap_or(24 * 3600);
    let db = state.db.clone();
    let entries = run_blocking(move || db.overview(cutoff)).await?;
    Ok(Json(serde_json::json!({ "entries": entries })))
}

#[derive(Debug, Deserialize)]
struct ExportQuery {
    repo_id: Option<String>,
}

async fn export_trajectory(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    axum::extract::Query(query): axum::extract::Query<ExportQuery>,
) -> ApiResult<String> {
    authorize(&state, &headers)?;
    let db = state.db.clone();
    let repo_id = query.repo_id;
    run_blocking(move || {
        let records = db.trajectory_records(repo_id.as_deref())?;
        let mut lines = String::new();
        for record in records {
            lines.push_str(&serde_json::to_string(&record)?);
            lines.push('\n');
        }
        Ok(lines)
    })
    .await
}

fn authorize(state: &ApiState, headers: &HeaderMap) -> ApiResult<()> {
    let Some(value) = headers.get("authorization") else {
        return Err(auth_err());
    };
    let Ok(value) = value.to_str() else {
        return Err(auth_err());
    };
    if value == format!("Bearer {}", state.token) {
        Ok(())
    } else {
        Err(auth_err())
    }
}

fn auth_err() -> ApiError {
    ApiError {
        code: "WYRD_DIFF_AUTH_REQUIRED".to_string(),
        message: "missing or invalid local API token".to_string(),
    }
}

fn api_err(error: impl std::fmt::Display) -> ApiError {
    ApiError {
        code: "WYRD_DIFF_REQUEST_FAILED".to_string(),
        message: error.to_string(),
    }
}

/// Run a blocking closure on the Tokio blocking pool and normalize errors into
/// `ApiError`. Use for SQLite, filesystem, or git subprocess work that would
/// otherwise stall the async runtime.
async fn run_blocking<F, T>(f: F) -> ApiResult<T>
where
    F: FnOnce() -> anyhow::Result<T> + Send + 'static,
    T: Send + 'static,
{
    match tokio::task::spawn_blocking(f).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(err)) => Err(api_err(err)),
        Err(join) => Err(ApiError {
            code: "WYRD_DIFF_BLOCKING_TASK_FAILED".to_string(),
            message: join.to_string(),
        }),
    }
}
