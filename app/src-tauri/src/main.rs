#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![forbid(unsafe_code)]

use std::{env, net::SocketAddr, path::PathBuf};
use wyrd_diff_core::Database;

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            start_local_bridge(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to run Wyrd Diff");
}

fn start_local_bridge(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        if let Err(error) = run_local_bridge(&app).await {
            eprintln!("failed to start Wyrd Diff local bridge: {error:#}");
        }
    });
}

async fn run_local_bridge(_app: &tauri::AppHandle) -> anyhow::Result<()> {
    let db = Database::new(database_path());
    db.migrate()?;

    let host = env::var("WYRD_DIFF_API_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port = env::var("WYRD_DIFF_API_PORT")
        .unwrap_or_else(|_| "8765".to_string())
        .parse::<u16>()?;
    let token = env::var("WYRD_DIFF_TOKEN").unwrap_or_else(|_| "dev-local-token".to_string());
    let addr: SocketAddr = format!("{host}:{port}").parse()?;

    let (listener, bound) = wyrd_diff_api::bind_with_fallback(addr, 10).await?;
    let mcp_url = wyrd_diff_api::mcp_url_for(bound);
    write_runtime_marker(&bound, &mcp_url);
    println!("wyrd-diff local bridge listening on http://{bound}");
    println!("wyrd-diff MCP available at {mcp_url}");
    wyrd_diff_api::serve_on(db, token, listener, mcp_url).await
}

fn config_dir() -> PathBuf {
    if let Ok(override_dir) = env::var("WYRD_DIFF_CONFIG_DIR") {
        return PathBuf::from(override_dir);
    }
    let home = env::var("HOME")
        .or_else(|_| env::var("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    home.join(".config").join("wyrd-diff")
}

fn write_runtime_marker(addr: &SocketAddr, mcp_url: &str) {
    let dir = config_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let path = dir.join("runtime.json");
    let body = serde_json::json!({
        "api_base": format!("http://{addr}"),
        "mcp_url": mcp_url,
        "port": addr.port(),
    });
    let _ = std::fs::write(path, format!("{body:#}\n"));
}

fn database_path() -> PathBuf {
    if let Ok(url) = env::var("DATABASE_URL")
        && let Some(path) = url.strip_prefix("sqlite://")
    {
        return PathBuf::from(path);
    }
    config_dir().join("wyrd-diff.db")
}
