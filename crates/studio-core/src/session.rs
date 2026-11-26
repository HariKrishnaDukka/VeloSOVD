//! Login session and guarded API access.
//!
//! The user token lives only in this process's memory: never in the web
//! view, never on disk. The UI asks Rust to make requests; Rust adds the
//! device certificate (TLS) and the token (Authorization header).

use std::{
    fmt,
    sync::Mutex,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::{
    StudioConfig,
    trace::{Trace, TraceEntry, excerpt},
};

pub const MAX_PATH_LEN: usize = 2048;
pub const MAX_BODY_LEN: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "message", rename_all = "kebab-case")]
pub enum StudioError {
    NotLoggedIn,
    SessionExpired(String),
    LoginFailed(String),
    InvalidRequest(String),
    Transport(String),
}

impl fmt::Display for StudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotLoggedIn => f.write_str("not logged in"),
            Self::SessionExpired(m) => write!(f, "session expired: {m}"),
            Self::LoginFailed(m) => write!(f, "login failed: {m}"),
            Self::InvalidRequest(m) => write!(f, "invalid request: {m}"),
            Self::Transport(m) => write!(f, "connection error: {m}"),
        }
    }
}

impl std::error::Error for StudioError {}

/// What the UI may know about the session (no token).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SessionInfo {
    pub user: String,
    pub role: String,
    pub expires_unix: u64,
    pub idle_timeout_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ApiResponse {
    pub status: u16,
    pub content_type: Option<String>,
    pub body: String,
    pub elapsed_ms: u64,
}

struct Session {
    info: SessionInfo,
    token: String,
    last_activity: Instant,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
    role: String,
}

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

/// Full error chain ("error sending request: ... : invalid peer certificate: ...").
fn chain(e: &dyn std::error::Error) -> String {
    let mut text = e.to_string();
    let mut source = e.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

/// Only central API paths may be requested from the UI.
pub fn validate_path(path: &str) -> Result<(), StudioError> {
    let bad = |m: &str| Err(StudioError::InvalidRequest(m.into()));
    if path.len() > MAX_PATH_LEN {
        return bad("path too long");
    }
    if !(path.starts_with("/api/") || path.starts_with("/sovd/")) {
        return bad("path must start with /api/ or /sovd/");
    }
    if path.contains("//") || path.contains("..") || path.contains('\\') || path.contains('#') {
        return bad("path contains forbidden sequences");
    }
    if path.chars().any(|c| c.is_control() || c == ' ') {
        return bad("path contains control characters or spaces");
    }
    Ok(())
}

pub struct Studio {
    cfg: StudioConfig,
    client: reqwest::Client,
    session: Mutex<Option<Session>>,
    pub trace: Trace,
}

impl Studio {
    /// Production: mTLS client from the configuration.
    pub fn new(cfg: StudioConfig) -> Result<Self, String> {
        cfg.validate()?;
        let client = crate::tls::build_client(&cfg)?;
        Ok(Self::with_client(cfg, client))
    }

    /// Tests: any client (e.g. plain HTTP against mock servers).
    pub fn with_client(cfg: StudioConfig, client: reqwest::Client) -> Self {
        Self {
            cfg,
            client,
            session: Mutex::new(None),
            trace: Trace::default(),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Session>> {
        self.session.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn config(&self) -> &StudioConfig {
        &self.cfg
    }

    pub async fn login(&self, username: &str, password: &str) -> Result<SessionInfo, StudioError> {
        if username.trim().is_empty() || password.is_empty() {
            return Err(StudioError::LoginFailed(
                "enter user name and password".into(),
            ));
        }
        self.logout();
        let started = Instant::now();
        let result = self
            .client
            .post(format!("{}/token", self.cfg.auth_url))
            .form(&[
                ("grant_type", "password"),
                ("username", username.trim()),
                ("password", password),
            ])
            .send()
            .await;
        let elapsed_ms = started.elapsed().as_millis() as u64;
        let mut trace = TraceEntry {
            seq: 0,
            ts_unix_ms: unix_ms(),
            user: Some(username.trim().to_string()),
            method: "POST".into(),
            path: "/token (auth-service)".into(),
            status: None,
            elapsed_ms,
            request_excerpt: Some("grant_type=password&username=…&password=***".into()),
            response_excerpt: None,
            error: None,
        };
        let response = match result {
            Ok(r) => r,
            Err(e) => {
                trace.error = Some(chain(&e));
                self.trace.push(trace);
                return Err(StudioError::Transport(format!(
                    "auth-service unreachable: {}",
                    chain(&e)
                )));
            }
        };
        trace.status = Some(response.status().as_u16());
        if !response.status().is_success() {
            trace.response_excerpt = Some("invalid_grant".into());
            self.trace.push(trace);
            return Err(StudioError::LoginFailed(
                "invalid user name or password".into(),
            ));
        }
        let token: TokenResponse = response
            .json()
            .await
            .map_err(|e| StudioError::Transport(format!("bad token response: {e}")))?;
        trace.response_excerpt = Some(format!("token issued, role={}", token.role));
        self.trace.push(trace);

        let info = SessionInfo {
            user: username.trim().to_string(),
            role: token.role,
            expires_unix: unix_now() + token.expires_in,
            idle_timeout_secs: self.cfg.idle_timeout_secs,
        };
        *self.lock() = Some(Session {
            info: info.clone(),
            token: token.access_token,
            last_activity: Instant::now(),
        });
        Ok(info)
    }

    pub fn logout(&self) {
        *self.lock() = None;
    }

    /// Current session; ends it (returns `Err`) on idle timeout or token expiry.
    fn check_session(&self, now_unix: u64, now: Instant) -> Result<(), StudioError> {
        let mut guard = self.lock();
        let Some(session) = guard.as_ref() else {
            return Err(StudioError::NotLoggedIn);
        };
        let idle = Duration::from_secs(self.cfg.idle_timeout_secs);
        let reason = if now.duration_since(session.last_activity) > idle {
            Some("logged out after inactivity")
        } else if now_unix >= session.info.expires_unix {
            Some("login expired, please log in again")
        } else {
            None
        };
        if let Some(reason) = reason {
            *guard = None;
            return Err(StudioError::SessionExpired(reason.into()));
        }
        Ok(())
    }

    pub fn session_info(&self) -> Option<SessionInfo> {
        self.check_session(unix_now(), Instant::now()).ok()?;
        self.lock().as_ref().map(|s| s.info.clone())
    }

    /// Call the central API as the logged-in user.
    pub async fn api_request(
        &self,
        method: &str,
        path: &str,
        body: Option<String>,
    ) -> Result<ApiResponse, StudioError> {
        self.api_request_at(method, path, body, unix_now(), Instant::now())
            .await
    }

    pub async fn api_request_at(
        &self,
        method: &str,
        path: &str,
        body: Option<String>,
        now_unix: u64,
        now: Instant,
    ) -> Result<ApiResponse, StudioError> {
        let method = match method {
            "GET" => reqwest::Method::GET,
            "POST" => reqwest::Method::POST,
            "PUT" => reqwest::Method::PUT,
            "DELETE" => reqwest::Method::DELETE,
            other => {
                return Err(StudioError::InvalidRequest(format!(
                    "method {other} not allowed"
                )));
            }
        };
        validate_path(path)?;
        if body.as_ref().is_some_and(|b| b.len() > MAX_BODY_LEN) {
            return Err(StudioError::InvalidRequest("request body too large".into()));
        }
        self.check_session(now_unix, now)?;
        let (token, user) = {
            let mut guard = self.lock();
            let session = guard.as_mut().ok_or(StudioError::NotLoggedIn)?;
            session.last_activity = now;
            (session.token.clone(), session.info.user.clone())
        };

        let mut request = self
            .client
            .request(method.clone(), format!("{}{path}", self.cfg.central_url))
            .bearer_auth(&token)
            .header("Accept", "application/json");
        if let Some(b) = &body {
            request = request
                .header("Content-Type", "application/json")
                .body(b.clone());
        }
        let started = Instant::now();
        let mut trace = TraceEntry {
            seq: 0,
            ts_unix_ms: unix_ms(),
            user: Some(user),
            method: method.to_string(),
            path: path.to_string(),
            status: None,
            elapsed_ms: 0,
            request_excerpt: body.as_deref().map(excerpt),
            response_excerpt: None,
            error: None,
        };
        let response = match request.send().await {
            Ok(r) => r,
            Err(e) => {
                trace.elapsed_ms = started.elapsed().as_millis() as u64;
                trace.error = Some(chain(&e));
                self.trace.push(trace);
                return Err(StudioError::Transport(chain(&e)));
            }
        };
        let status = response.status().as_u16();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let text = response
            .text()
            .await
            .map_err(|e| StudioError::Transport(e.to_string()))?;
        let elapsed_ms = started.elapsed().as_millis() as u64;
        trace.status = Some(status);
        trace.elapsed_ms = elapsed_ms;
        trace.response_excerpt = Some(excerpt(&text));
        self.trace.push(trace);

        if status == 401 {
            // Central no longer accepts this login (expired, key rotated, user disabled).
            self.logout();
            return Err(StudioError::SessionExpired(
                "the server rejected the login, please log in again".into(),
            ));
        }
        Ok(ApiResponse {
            status,
            content_type,
            body: text,
            elapsed_ms,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Form, Json, Router,
        http::{HeaderMap, StatusCode},
        routing::{get, post},
    };
    use std::collections::HashMap;

    fn cfg(base: &str) -> StudioConfig {
        StudioConfig {
            auth_url: format!("https://{base}"),
            central_url: format!("https://{base}"),
            tls_server_name: "10.77.10.1".into(),
            ca_cert: String::new(),
            device_cert: String::new(),
            device_key: String::new(),
            idle_timeout_secs: 60,
        }
    }

    /// Mock auth + central on one plain-HTTP port; URLs rewritten to http.
    async fn studio() -> Studio {
        async fn token(
            Form(f): Form<HashMap<String, String>>,
        ) -> (StatusCode, Json<serde_json::Value>) {
            if f.get("username").map(String::as_str) == Some("hari")
                && f.get("password").map(String::as_str) == Some("correct password")
            {
                (
                    StatusCode::OK,
                    Json(
                        serde_json::json!({"access_token":"tok-123","token_type":"Bearer","expires_in":900,"role":"technician"}),
                    ),
                )
            } else {
                (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error":"invalid_grant"})),
                )
            }
        }
        async fn ecus(headers: HeaderMap) -> (StatusCode, String) {
            match headers.get("authorization").and_then(|v| v.to_str().ok()) {
                Some("Bearer tok-123") => (StatusCode::OK, "[{\"id\":\"ECM\"}]".into()),
                _ => (StatusCode::UNAUTHORIZED, "{}".into()),
            }
        }
        async fn revoked() -> StatusCode {
            StatusCode::UNAUTHORIZED
        }
        let app = Router::new()
            .route("/token", post(token))
            .route("/api/ecus", get(ecus))
            .route("/api/revoked", get(revoked));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let mut c = cfg(&addr.to_string());
        c.auth_url = c.auth_url.replace("https://", "http://");
        c.central_url = c.central_url.replace("https://", "http://");
        Studio::with_client(c, reqwest::Client::new())
    }

    #[tokio::test]
    async fn login_then_requests_carry_the_token() {
        let s = studio().await;
        assert_eq!(
            s.api_request("GET", "/api/ecus", None).await,
            Err(StudioError::NotLoggedIn)
        );
        let info = s.login("hari", "correct password").await.unwrap();
        assert_eq!(
            (info.user.as_str(), info.role.as_str()),
            ("hari", "technician")
        );
        let r = s.api_request("GET", "/api/ecus", None).await.unwrap();
        assert_eq!(r.status, 200);
        assert!(r.body.contains("ECM"));
    }

    #[tokio::test]
    async fn wrong_password_fails_without_session() {
        let s = studio().await;
        assert!(matches!(
            s.login("hari", "wrong").await,
            Err(StudioError::LoginFailed(_))
        ));
        assert!(s.session_info().is_none());
        assert!(matches!(
            s.login("", "x").await,
            Err(StudioError::LoginFailed(_))
        ));
    }

    #[tokio::test]
    async fn server_401_ends_the_session() {
        let s = studio().await;
        s.login("hari", "correct password").await.unwrap();
        assert!(matches!(
            s.api_request("GET", "/api/revoked", None).await,
            Err(StudioError::SessionExpired(_))
        ));
        assert!(s.session_info().is_none());
    }

    #[tokio::test]
    async fn idle_timeout_and_token_expiry_log_out() {
        let s = studio().await;
        s.login("hari", "correct password").await.unwrap();
        let idle_later = Instant::now() + Duration::from_secs(61);
        assert!(matches!(
            s.api_request_at("GET", "/api/ecus", None, unix_now(), idle_later)
                .await,
            Err(StudioError::SessionExpired(_))
        ));
        assert!(s.session_info().is_none());

        s.login("hari", "correct password").await.unwrap();
        let after_expiry = unix_now() + 901;
        assert!(matches!(
            s.api_request_at("GET", "/api/ecus", None, after_expiry, Instant::now())
                .await,
            Err(StudioError::SessionExpired(_))
        ));
    }

    #[tokio::test]
    async fn trace_records_requests_but_never_secrets() {
        let s = studio().await;
        s.login("hari", "correct password").await.unwrap();
        s.api_request("GET", "/api/ecus", None).await.unwrap();
        let entries = s.trace.since(0);
        assert_eq!(entries.len(), 2);
        let all = serde_json::to_string(&entries).unwrap();
        assert!(!all.contains("tok-123"), "token must not be traced");
        assert!(
            !all.contains("correct password"),
            "password must not be traced"
        );
        assert_eq!(entries[1].path, "/api/ecus");
        assert_eq!(entries[1].user.as_deref(), Some("hari"));
    }

    #[test]
    fn only_central_api_paths_are_allowed() {
        for ok in [
            "/api/ecus",
            "/sovd/v1/components/ECM/operations/x/executions",
            "/api/ecus/ECM/diagnostics/faults?active=true",
        ] {
            assert!(validate_path(ok).is_ok(), "{ok}");
        }
        for bad in [
            "/token",
            "https://evil/x",
            "//evil/api/",
            "/api/../token",
            "/api/a b",
            "/api/x#y",
            "/api\\x",
            "/api/\u{0}",
        ] {
            assert!(validate_path(bad).is_err(), "{bad}");
        }
        assert!(validate_path(&format!("/api/{}", "a".repeat(MAX_PATH_LEN))).is_err());
    }

    #[tokio::test]
    async fn rejects_bad_methods_and_large_bodies() {
        let s = studio().await;
        s.login("hari", "correct password").await.unwrap();
        assert!(matches!(
            s.api_request("PATCH", "/api/ecus", None).await,
            Err(StudioError::InvalidRequest(_))
        ));
        let big = "x".repeat(MAX_BODY_LEN + 1);
        assert!(matches!(
            s.api_request("POST", "/api/ecus", Some(big)).await,
            Err(StudioError::InvalidRequest(_))
        ));
    }
}
