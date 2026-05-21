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
        .route("/api/review-sessions/:id/diff", get(review_diff))
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
    let statuses =
        wyrd_diff_core::agent_config::agent_status(&state.mcp_url).map_err(|e| ApiError {
            code: "agent_status_failed".to_string(),
            message: format!("{e:#}"),
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
    let writes = wyrd_diff_core::agent_config::configure_agents(&url).map_err(|e| ApiError {
        code: "configure_agents_failed".to_string(),
        message: format!("{e:#}"),
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
    let write = wyrd_diff_core::agent_config::configure_agent(&id, &url).map_err(|e| ApiError {
        code: "configure_agent_failed".to_string(),
        message: format!("{e:#}"),
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
        Ok(None) => StatusCode::ACCEPTED.into_response(),
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
    Ok(Json(
        serde_json::json!({ "repos": state.db.repos().map_err(api_err)? }),
    ))
}

async fn upsert_repo(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Json(input): Json<NewRepo>,
) -> ApiResult<Json<serde_json::Value>> {
    authorize(&state, &headers)?;
    Ok(Json(
        serde_json::json!({ "repo": state.db.upsert_repo(input).map_err(api_err)? }),
    ))
}

async fn review_sessions(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
) -> ApiResult<Json<serde_json::Value>> {
    authorize(&state, &headers)?;
    Ok(Json(
        serde_json::json!({ "review_sessions": state.db.review_sessions().map_err(api_err)? }),
    ))
}

async fn create_review_session(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Json(input): Json<NewReviewSession>,
) -> ApiResult<Json<serde_json::Value>> {
    authorize(&state, &headers)?;
    Ok(Json(serde_json::json!({
        "review_session": state.db.create_review_session(input).map_err(api_err)?
    })))
}

async fn review_session(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    authorize(&state, &headers)?;
    Ok(Json(
        serde_json::json!({ "review_session": state.db.review_session(&id).map_err(api_err)? }),
    ))
}

async fn review_diff(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    authorize(&state, &headers)?;
    Ok(Json(
        serde_json::json!({ "files": state.db.review_diff(&id).map_err(api_err)? }),
    ))
}

async fn refresh_review_session(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    authorize(&state, &headers)?;
    Ok(Json(serde_json::json!({
        "review_session": state.db.refresh_review_session(&id).map_err(api_err)?
    })))
}

async fn add_comment(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(mut input): Json<NewComment>,
) -> ApiResult<Json<serde_json::Value>> {
    authorize(&state, &headers)?;
    input.session_id = id;
    Ok(Json(
        serde_json::json!({ "id": state.db.add_comment(input).map_err(api_err)? }),
    ))
}

async fn review_threads(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    authorize(&state, &headers)?;
    Ok(Json(
        serde_json::json!({ "threads": state.db.review_threads(&id).map_err(api_err)? }),
    ))
}

async fn add_review_thread(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(mut input): Json<NewReviewThread>,
) -> ApiResult<Json<serde_json::Value>> {
    authorize(&state, &headers)?;
    input.session_id = id;
    Ok(Json(
        serde_json::json!({ "thread": state.db.add_review_thread(input).map_err(api_err)? }),
    ))
}

async fn delete_review_thread(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    authorize(&state, &headers)?;
    let removed = state.db.delete_review_thread(&id).map_err(api_err)?;
    Ok(Json(serde_json::json!({ "deleted": removed })))
}

async fn resolve_thread(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    authorize(&state, &headers)?;
    let updated = state.db.resolve_thread(&id).map_err(api_err)?;
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
    let updated = state.db.reopen_thread(&id).map_err(api_err)?;
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
    let batch = state
        .db
        .create_feedback_batch(&id, input.agent_session_id)
        .map_err(api_err)?;
    Ok(Json(serde_json::json!({ "batch": batch })))
}

async fn list_feedback_batches(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    authorize(&state, &headers)?;
    Ok(Json(
        serde_json::json!({ "batches": state.db.feedback_batches(&id).map_err(api_err)? }),
    ))
}

async fn add_thread_message(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(mut input): Json<NewThreadMessage>,
) -> ApiResult<Json<serde_json::Value>> {
    authorize(&state, &headers)?;
    input.thread_id = id;
    Ok(Json(
        serde_json::json!({ "message": state.db.add_thread_message(input).map_err(api_err)? }),
    ))
}

async fn add_note(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(mut input): Json<NewNote>,
) -> ApiResult<Json<serde_json::Value>> {
    authorize(&state, &headers)?;
    input.session_id = Some(id);
    Ok(Json(
        serde_json::json!({ "id": state.db.add_note(input).map_err(api_err)? }),
    ))
}

async fn add_decision(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(mut input): Json<NewDecision>,
) -> ApiResult<Json<serde_json::Value>> {
    authorize(&state, &headers)?;
    input.session_id = Some(id);
    Ok(Json(
        serde_json::json!({ "id": state.db.add_decision(input).map_err(api_err)? }),
    ))
}

async fn import_fix(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(mut input): Json<NewFixImport>,
) -> ApiResult<Json<serde_json::Value>> {
    authorize(&state, &headers)?;
    input.session_id = id;
    Ok(Json(
        serde_json::json!({ "id": state.db.import_fix(input).map_err(api_err)? }),
    ))
}

async fn fix_imports(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    authorize(&state, &headers)?;
    Ok(Json(
        serde_json::json!({ "fix_imports": state.db.fix_imports(&id).map_err(api_err)? }),
    ))
}

async fn agent_context(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    authorize(&state, &headers)?;
    Ok(Json(serde_json::json!(
        state.db.agent_context(&id).map_err(api_err)?
    )))
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
    let records = state
        .db
        .trajectory_records(query.repo_id.as_deref())
        .map_err(api_err)?;
    let mut lines = String::new();
    for record in records {
        lines.push_str(&serde_json::to_string(&record).map_err(api_err)?);
        lines.push('\n');
    }
    Ok(lines)
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
