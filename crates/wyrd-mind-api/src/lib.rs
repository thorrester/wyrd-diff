//! Local API for Wyrd Mind.

#![forbid(unsafe_code)]

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, sync::Arc};
use tower_http::cors::CorsLayer;
use wyrd_mind_core::{
    Database, NewComment, NewDecision, NewFixImport, NewNote, NewRepo, NewReviewSession,
};

/// Shared API state.
#[derive(Clone)]
pub struct ApiState {
    db: Database,
    token: String,
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
    let state = Arc::new(ApiState { db, token });
    let app = router(state);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
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
        .route("/api/review-sessions/:id/comments", post(add_comment))
        .route("/api/review-sessions/:id/notes", post(add_note))
        .route("/api/review-sessions/:id/decisions", post(add_decision))
        .route(
            "/api/review-sessions/:id/fix-imports",
            get(fix_imports).post(import_fix),
        )
        .route("/api/review-sessions/:id/agent-context", get(agent_context))
        .route("/api/export/trajectory.jsonl", get(export_trajectory))
        .with_state(state)
        .layer(CorsLayer::permissive())
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
        code: "WYRD_MIND_AUTH_REQUIRED".to_string(),
        message: "missing or invalid local API token".to_string(),
    }
}

fn api_err(error: impl std::fmt::Display) -> ApiError {
    ApiError {
        code: "WYRD_MIND_REQUEST_FAILED".to_string(),
        message: error.to_string(),
    }
}
