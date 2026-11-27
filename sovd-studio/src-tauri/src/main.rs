//! VeloSOVD Studio desktop shell (M2).
//!
//! Thin layer: every command forwards to `studio-core`. The web view never
//! sees the user token or the device key - it can only ask Rust to perform
//! an allow-listed central API request on behalf of the logged-in user.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};

use serde::Serialize;
use studio_core::{
    ApiResponse, SessionInfo, Studio, StudioConfig, StudioError, faults, license, operations,
    polling, trace::TraceEntry,
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

// ---- M3: faults, operations, polling ----
// Every rule (roles, confirmation, paths, capabilities) lives in studio-core and is
// unit-tested there; these commands only look up the session and call it.

fn role_of(studio: &Studio) -> Result<String, StudioError> {
    studio
        .session_info()
        .map(|s| s.role)
        .ok_or(StudioError::NotLoggedIn)
}

/// Central answered something other than 2xx: tell the UI which status.
fn require_ok(response: &ApiResponse) -> Result<(), StudioError> {
    if (200..300).contains(&response.status) {
        Ok(())
    } else {
        Err(StudioError::InvalidRequest(format!(
            "central answered HTTP {}",
            response.status
        )))
    }
}

/// The one polling interval for every repeated request to central.
#[tauri::command]
fn poll_interval_secs() -> u64 {
    polling::POLL_INTERVAL_SECS
}

#[tauri::command]
fn decode_dtc_status(status: u8) -> [faults::StatusBit; 8] {
    faults::decode_status(status)
}

/// Whether the logged-in user may clear faults or start operations.
#[tauri::command]
fn can_change(state: State<'_, AppState>) -> bool {
    state
        .studio()
        .ok()
        .and_then(|s| s.session_info())
        .is_some_and(|s| faults::may_change(&s.role))
}

#[tauri::command]
async fn fault_list(
    state: State<'_, AppState>,
    ecu: String,
    active: Option<bool>,
) -> Result<ApiResponse, StudioError> {
    let path = faults::fault_list_path(&ecu, active)?;
    state.studio()?.api_request("GET", &path, None).await
}

/// `code = None` clears all faults of the ECU. `confirm` must repeat the ECU id.
#[tauri::command]
async fn clear_faults(
    state: State<'_, AppState>,
    ecu: String,
    code: Option<String>,
    confirm: Option<String>,
) -> Result<ApiResponse, StudioError> {
    let studio = state.studio()?;
    let role = role_of(studio)?;
    let plan = faults::plan_clear(&role, &ecu, code.as_deref(), confirm.as_deref())?;
    studio.api_request(plan.method, &plan.path, None).await
}

#[tauri::command]
async fn operation_list(
    state: State<'_, AppState>,
    ecu: String,
) -> Result<Vec<operations::OperationInfo>, StudioError> {
    let path = operations::descriptors_path(&ecu)?;
    let response = state.studio()?.api_request("GET", &path, None).await?;
    require_ok(&response)?;
    operations::parse_operations(&response.body)
}

#[tauri::command]
async fn operation_history(
    state: State<'_, AppState>,
    ecu: String,
    operation: String,
) -> Result<ApiResponse, StudioError> {
    let path = operations::history_path(&ecu, &operation)?;
    state.studio()?.api_request("GET", &path, None).await
}

/// Returns the new execution id.
#[tauri::command]
async fn operation_start(
    state: State<'_, AppState>,
    ecu: String,
    operation: String,
) -> Result<String, StudioError> {
    let studio = state.studio()?;
    let role = role_of(studio)?;
    let plan = operations::plan_start(&role, &ecu, &operation)?;
    let response = studio
        .api_request(plan.method, &plan.path, plan.body)
        .await?;
    require_ok(&response)?;
    operations::parse_start_id(&response.body)
}

#[tauri::command]
async fn operation_status(
    state: State<'_, AppState>,
    ecu: String,
    operation: String,
    execution: String,
) -> Result<operations::RunState, StudioError> {
    let path = operations::execution_path(&ecu, &operation, &execution)?;
    let response = state.studio()?.api_request("GET", &path, None).await?;
    require_ok(&response)?;
    operations::parse_run_state(&response.body)
}

/// Stop needs the role AND the operation's own `capabilities.stop`, read from central here
/// (never supplied by the web view).
#[tauri::command]
async fn operation_stop(
    state: State<'_, AppState>,
    ecu: String,
    operation: String,
    execution: String,
) -> Result<(), StudioError> {
    let studio = state.studio()?;
    let role = role_of(studio)?;
    let list_path = operations::descriptors_path(&ecu)?;
    let listed = studio.api_request("GET", &list_path, None).await?;
    require_ok(&listed)?;
    let capabilities = operations::parse_operations(&listed.body)?
        .into_iter()
        .find(|o| o.id == operation)
        .and_then(|o| o.capabilities);
    let plan = operations::plan_stop(&role, capabilities.as_ref(), &ecu, &operation, &execution)?;
    let response = studio
        .api_request(plan.method, &plan.path, plan.body)
        .await?;
    require_ok(&response)
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
        Ok(s) => println!(
            "[STUDIO] config {:?}, central {}",
            config_path,
            s.config().central_url
        ),
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
            license_status,
            poll_interval_secs,
            decode_dtc_status,
            can_change,
            fault_list,
            clear_faults,
            operation_list,
            operation_history,
            operation_start,
            operation_status,
            operation_stop
        ])
        .run(tauri::generate_context!())
        .expect("VeloSOVD Studio failed to start");
}
