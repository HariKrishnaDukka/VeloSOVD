//! VeloSOVD Studio desktop shell (M2).
//!
//! Thin layer: every command forwards to `studio-core`. The web view never
//! sees the user token or the device key - it can only ask Rust to perform
//! an allow-listed central API request on behalf of the logged-in user.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};

use serde::Serialize;
use studio_core::{
    ApiResponse, SessionInfo, Studio, StudioConfig, StudioError, license, trace::TraceEntry,
};
use tauri::State;

/// Studio, or the reason it could not start (shown on the login screen).
struct AppState {
    studio: Result<Studio, String>,
    config_path: Option<PathBuf>,
}

impl AppState {
    fn studio(&self) -> Result<&Studio, StudioError> {
        self.studio
            .as_ref()
            .map_err(|e| StudioError::Transport(format!("Studio is not configured: {e}")))
    }
}

#[derive(Serialize)]
struct StudioInfo {
    version: &'static str,
    config_path: Option<String>,
    startup_error: Option<String>,
    auth_url: Option<String>,
    central_url: Option<String>,
    tls_server_name: Option<String>,
    device_cert: Option<String>,
    idle_timeout_secs: Option<u64>,
}

#[tauri::command]
fn studio_info(state: State<'_, AppState>) -> StudioInfo {
    let cfg = state.studio.as_ref().ok().map(|s| s.config().clone());
    StudioInfo {
        version: env!("CARGO_PKG_VERSION"),
        config_path: state.config_path.as_ref().map(|p| p.display().to_string()),
        startup_error: state.studio.as_ref().err().cloned(),
        auth_url: cfg.as_ref().map(|c| c.auth_url.clone()),
        central_url: cfg.as_ref().map(|c| c.central_url.clone()),
        tls_server_name: cfg.as_ref().map(|c| c.tls_server_name.clone()),
        device_cert: cfg.as_ref().map(|c| c.device_cert.clone()),
        idle_timeout_secs: cfg.as_ref().map(|c| c.idle_timeout_secs),
    }
}

#[tauri::command]
async fn login(
    state: State<'_, AppState>,
    username: String,
    password: String,
) -> Result<SessionInfo, StudioError> {
    state.studio()?.login(&username, &password).await
}

#[tauri::command]
fn logout(state: State<'_, AppState>) {
    if let Ok(studio) = state.studio() {
        studio.logout();
    }
}

#[tauri::command]
fn session_info(state: State<'_, AppState>) -> Option<SessionInfo> {
    state.studio().ok()?.session_info()
}

#[tauri::command]
async fn api_request(
    state: State<'_, AppState>,
    method: String,
    path: String,
    body: Option<String>,
) -> Result<ApiResponse, StudioError> {
    state.studio()?.api_request(&method, &path, body).await
}

#[tauri::command]
fn trace_since(state: State<'_, AppState>, since: u64) -> Vec<TraceEntry> {
    state
        .studio()
        .map(|s| s.trace.since(since))
        .unwrap_or_default()
}

#[tauri::command]
fn trace_clear(state: State<'_, AppState>) {
    if let Ok(s) = state.studio() {
        s.trace.clear();
    }
}

#[tauri::command]
fn license_status() -> license::LicenseStatus {
    license::status()
}

/// `SDV_STUDIO_CONFIG`, else `platform/config/studio.toml` searched upwards
/// from the working directory (works from the repo root and from src-tauri).
fn find_config() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("SDV_STUDIO_CONFIG") {
        return Some(PathBuf::from(p));
    }
    let mut dir = std::env::current_dir().ok()?;
    for _ in 0..5 {
        let candidate = dir.join("platform/config/studio.toml");
        if candidate.is_file() {
            return Some(candidate);
        }
        dir = dir.parent()?.to_path_buf();
    }
    None
}

fn load_studio(path: Option<&Path>) -> Result<Studio, String> {
    let path = path.ok_or("studio.toml not found (set SDV_STUDIO_CONFIG)")?;
    Studio::new(StudioConfig::load(path)?)
}

fn main() {
    let config_path = find_config();
    let studio = load_studio(config_path.as_deref());
    match &studio {
        Ok(s) => println!("[STUDIO] config {:?}, central {}", config_path, s.config().central_url),
        Err(e) => eprintln!("[STUDIO] not configured: {e}"),
    }

    tauri::Builder::default()
        .manage(AppState {
            studio,
            config_path,
        })
        .invoke_handler(tauri::generate_handler![
            studio_info,
            login,
            logout,
            session_info,
            api_request,
            trace_since,
            trace_clear,
            license_status
        ])
        .run(tauri::generate_context!())
        .expect("VeloSOVD Studio failed to start");
}
