//! Integration tests for the diff + thread HTTP contracts.
//!
//! Each test builds a temp git repo with one base commit and one head commit
//! that adds lines, materializes a review session, and drives the Axum router
//! through `tower::ServiceExt::oneshot`.

use std::collections::HashSet;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
};
use http_body_util::BodyExt;
use serde_json::Value;
use tempfile::TempDir;
use tower::ServiceExt;
use wyrd_diff_api::{ApiState, router};
use wyrd_diff_core::{Database, NewRepo, NewReviewSession};

const TOKEN: &str = "test-token";

struct Fixture {
    _temp: TempDir,
    router: Router,
    session_id: String,
    file_path: String,
}

fn run(dir: &Path, args: &[&str]) {
    let status = Command::new(args[0])
        .args(&args[1..])
        .current_dir(dir)
        .status()
        .expect("spawn");
    assert!(status.success(), "{args:?}");
}

fn make_fixture() -> Fixture {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo_dir = temp.path().join("repo");
    std::fs::create_dir(&repo_dir).unwrap();
    run(&repo_dir, &["git", "init"]);
    run(&repo_dir, &["git", "config", "user.email", "t@example.com"]);
    run(&repo_dir, &["git", "config", "user.name", "T"]);
    std::fs::write(repo_dir.join("a.txt"), "one\n").unwrap();
    run(&repo_dir, &["git", "add", "."]);
    run(&repo_dir, &["git", "commit", "-m", "base"]);
    run(&repo_dir, &["git", "checkout", "-b", "feature"]);
    std::fs::write(repo_dir.join("a.txt"), "one\ntwo\nthree\n").unwrap();
    run(&repo_dir, &["git", "commit", "-am", "add lines"]);

    let db = Database::new(temp.path().join("diff.db"));
    db.migrate().expect("migrate");
    let repo = db
        .upsert_repo(NewRepo {
            name: "fixture".to_string(),
            path: repo_dir.display().to_string(),
        })
        .expect("repo");
    let session = db
        .create_review_session(NewReviewSession {
            repo_id: repo.id,
            title: "feature review".to_string(),
            base_ref: "master".to_string(),
            head_ref: "feature".to_string(),
            branch: Some("feature".to_string()),
        })
        .expect("session");
    let state = Arc::new(ApiState::new(
        db,
        TOKEN.to_string(),
        "http://127.0.0.1:0/mcp".to_string(),
    ));
    Fixture {
        _temp: temp,
        router: router(state),
        session_id: session.id,
        file_path: "a.txt".to_string(),
    }
}

async fn send(router: &Router, req: Request<Body>) -> (StatusCode, Value) {
    let response = router.clone().oneshot(req).await.expect("oneshot");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect")
        .to_bytes();
    let body: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or_else(|err| {
            panic!(
                "non-json body: {err}; raw: {}",
                String::from_utf8_lossy(&bytes)
            )
        })
    };
    (status, body)
}

fn auth(req: axum::http::request::Builder) -> axum::http::request::Builder {
    req.header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .header(header::CONTENT_TYPE, "application/json")
}

#[tokio::test]
async fn diff_summary_returns_file_list_without_hunks() {
    let f = make_fixture();
    let req = auth(
        Request::builder()
            .method("GET")
            .uri(format!("/api/review-sessions/{}/diff", f.session_id)),
    )
    .body(Body::empty())
    .unwrap();
    let (status, body) = send(&f.router, req).await;
    assert_eq!(status, StatusCode::OK);
    let files = body["files"].as_array().expect("files array");
    assert!(!files.is_empty(), "expected at least one file");
    let entry = &files[0];
    assert_eq!(entry["path"], Value::String(f.file_path.clone()));
    assert!(entry.get("hunks").is_none(), "summary must omit hunks");
    assert!(entry["additions"].as_u64().unwrap() >= 1);
}

#[tokio::test]
async fn diff_file_returns_unique_hunk_and_line_ids() {
    let f = make_fixture();
    let uri = format!(
        "/api/review-sessions/{}/diff/file?path={}",
        f.session_id, f.file_path
    );
    let req = auth(Request::builder().method("GET").uri(uri))
        .body(Body::empty())
        .unwrap();
    let (status, body) = send(&f.router, req).await;
    assert_eq!(status, StatusCode::OK);
    let file = &body["file"];
    assert!(!file.is_null(), "file payload missing");
    let hunks = file["hunks"].as_array().expect("hunks array");
    assert!(!hunks.is_empty(), "expected at least one hunk");

    let mut hunk_ids = HashSet::new();
    let mut line_ids = HashSet::new();
    for hunk in hunks {
        let id = hunk["id"].as_str().expect("hunk.id string");
        assert!(!id.is_empty(), "hunk.id must not be empty");
        assert!(hunk_ids.insert(id.to_string()), "duplicate hunk.id: {id}");

        for line in hunk["lines"].as_array().expect("lines array") {
            let lid = line["id"].as_str().expect("line.id string");
            assert!(!lid.is_empty(), "line.id must not be empty");
            assert!(line_ids.insert(lid.to_string()), "duplicate line.id: {lid}");
        }
    }
}

#[tokio::test]
async fn add_thread_with_synthetic_anchor_succeeds() {
    let f = make_fixture();
    let payload = serde_json::json!({
        "file_path": f.file_path,
        "anchor_diff_line_id": "a.txt#h0#l0",
        "old_line": null,
        "new_line": 2,
        "range_start_old_line": null,
        "range_start_new_line": 2,
        "range_end_old_line": null,
        "range_end_new_line": 2,
        "selected_text": "+two",
        "body": "needs a check",
        "message_type": "comment",
        "status": "open",
        "visibility": "agent",
    });
    let req = auth(
        Request::builder()
            .method("POST")
            .uri(format!("/api/review-sessions/{}/threads", f.session_id)),
    )
    .body(Body::from(payload.to_string()))
    .unwrap();
    let (status, body) = send(&f.router, req).await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    let thread = &body["thread"];
    assert_eq!(thread["file_path"], Value::String(f.file_path.clone()));
    assert_eq!(thread["new_line"], Value::from(2));
    assert!(
        thread["anchor_diff_line_id"].is_null(),
        "anchor_diff_line_id must be coerced to null; got: {}",
        thread["anchor_diff_line_id"]
    );
    let messages = thread["messages"].as_array().expect("messages");
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["body"], Value::String("needs a check".into()));
}

#[tokio::test]
async fn refresh_then_diff_file_still_renders_with_unique_ids() {
    let f = make_fixture();
    let refresh = auth(
        Request::builder()
            .method("POST")
            .uri(format!("/api/review-sessions/{}/refresh", f.session_id)),
    )
    .body(Body::from("{}"))
    .unwrap();
    let (status, _) = send(&f.router, refresh).await;
    assert_eq!(status, StatusCode::OK);

    let uri = format!(
        "/api/review-sessions/{}/diff/file?path={}",
        f.session_id, f.file_path
    );
    let req = auth(Request::builder().method("GET").uri(uri))
        .body(Body::empty())
        .unwrap();
    let (status, body) = send(&f.router, req).await;
    assert_eq!(status, StatusCode::OK);
    let hunks = body["file"]["hunks"].as_array().expect("hunks");
    assert!(!hunks.is_empty());
    let first_line = &hunks[0]["lines"].as_array().expect("lines")[0];
    assert!(
        !first_line["id"].as_str().unwrap_or_default().is_empty(),
        "line.id empty after refresh"
    );
}
