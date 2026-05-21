#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![forbid(unsafe_code)]

use std::{env, net::SocketAddr, path::PathBuf};
use tauri::Manager;
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

async fn run_local_bridge(app: &tauri::AppHandle) -> anyhow::Result<()> {
    let db = Database::new(database_path(app));
    db.migrate()?;

    let host = env::var("WYRD_DIFF_API_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port = env::var("WYRD_DIFF_API_PORT")
        .unwrap_or_else(|_| "8765".to_string())
        .parse::<u16>()?;
    let token = env::var("WYRD_DIFF_TOKEN").unwrap_or_else(|_| "dev-local-token".to_string());
    let addr: SocketAddr = format!("{host}:{port}").parse()?;

    println!("wyrd-diff local bridge listening on http://{addr}");
    wyrd_diff_api::serve(db, token, addr).await
}

fn database_path(app: &tauri::AppHandle) -> PathBuf {
    if let Ok(url) = env::var("DATABASE_URL")
        && let Some(path) = url.strip_prefix("sqlite://")
    {
        return PathBuf::from(path);
    }

    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from(".data"))
        .join("wyrd-diff.db")
}
