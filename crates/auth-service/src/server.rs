//! HTTPS + mTLS token endpoint (OAuth2 resource-owner password grant, RFC 6749 §4.3).

use std::{
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Instant,
};

use axum::{
    Form, Json, Router,
    extract::State,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::post,
};
use serde::Deserialize;
use serde_json::json;
use sovd_auth::{TokenSigner, unix_now};

use crate::{
    env_or,
    login::{Lockout, authenticate},
    store::UserStore,
    tls_config, users_path,
};

pub struct AuthState {
    pub users_path: PathBuf,
    pub signer: TokenSigner,
    pub ttl_secs: u64,
    pub lockout: Lockout,
    pub events: Mutex<Option<std::fs::File>>,
}

#[derive(Deserialize)]
pub struct TokenRequest {
    grant_type: String,
    username: String,
    password: String,
}

fn oauth_error(status: StatusCode, error: &str, description: &str) -> Response {
    let mut r = (
        status,
        Json(json!({ "error": error, "error_description": description })),
    )
        .into_response();
    r.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    r
}

fn log_event(state: &AuthState, user: &str, outcome: &str, reason: &str) {
    let line = json!({
        "ts_unix": unix_now(), "event": "login", "user": user,
        "outcome": outcome, "reason": reason,
    });
    println!("[AUTH] {line}");
    if let Ok(mut guard) = state.events.lock()
        && let Some(file) = guard.as_mut()
    {
        let _ = writeln!(file, "{line}");
    }
}

pub async fn token(State(state): State<Arc<AuthState>>, Form(req): Form<TokenRequest>) -> Response {
    if req.grant_type != "password" {
        return oauth_error(
            StatusCode::BAD_REQUEST,
            "unsupported_grant_type",
            "only grant_type=password is supported",
        );
    }
    // Reload per request: CLI changes (disable, set-role) apply without restart.
    let store = match UserStore::load(&state.users_path) {
        Ok(store) => store,
        Err(e) => {
            eprintln!("[AUTH] users file error: {e}");
            return oauth_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "server_error",
                "user store unavailable",
            );
        }
    };
    let username = req.username.trim().to_string();
    let password = req.password;
    let state2 = Arc::clone(&state);
    let user = username.clone();
    // argon2 is deliberately slow: run it off the async worker threads.
    let result = tokio::task::spawn_blocking(move || {
        authenticate(&store, &state2.lockout, &user, &password, Instant::now())
    })
    .await
    .unwrap_or(Err(crate::login::Denied::BadCredentials));

    match result {
        Ok(role) => match state.signer.issue(&username, &role, state.ttl_secs) {
            Ok((access_token, claims)) => {
                log_event(
                    &state,
                    &username,
                    "success",
                    &format!("role={role} jti={}", claims.jti),
                );
                let mut r = Json(json!({
                    "access_token": access_token,
                    "token_type": "Bearer",
                    "expires_in": state.ttl_secs,
                    "role": role,
                }))
                .into_response();
                r.headers_mut()
                    .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
                r
            }
            Err(e) => {
                eprintln!("[AUTH] signing failed: {e}");
                oauth_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "server_error",
                    "token signing failed",
                )
            }
        },
        Err(denied) => {
            log_event(&state, &username, "failure", denied.reason());
            // Same answer for every failure: do not reveal which part was wrong.
            oauth_error(
                StatusCode::BAD_REQUEST,
                "invalid_grant",
                "invalid username or password",
            )
        }
    }
}

pub fn router(state: Arc<AuthState>) -> Router {
    Router::new()
        .route("/token", post(token))
        .layer(axum::extract::DefaultBodyLimit::max(4 * 1024))
        .with_state(state)
}

pub async fn serve() {
    let key_path = env_or("SDV_TOKEN_SIGNING_KEY", "pki/dev/auth/token-signing.key");
    let key_pem = std::fs::read(&key_path)
        .unwrap_or_else(|e| panic!("[AUTH] cannot read signing key {key_path}: {e}"));
    let signer = TokenSigner::from_pkcs8_pem(&key_pem)
        .unwrap_or_else(|e| panic!("[AUTH] signing key {key_path}: {e}"));
    let ttl_secs: u64 = env_or("SDV_TOKEN_TTL_SECS", "900")
        .parse()
        .unwrap_or_else(|_| panic!("[AUTH] SDV_TOKEN_TTL_SECS must be a number"));
    let path = users_path();
    let users = UserStore::load(&path).unwrap_or_else(|e| panic!("[AUTH] users file: {e}"));

    let log_path = PathBuf::from(env_or("SDV_AUTH_LOG", "auth/auth-events.jsonl"));
    if let Some(dir) = log_path.parent().filter(|p| !p.as_os_str().is_empty()) {
        let _ = std::fs::create_dir_all(dir);
    }
    let events = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .ok();

    let state = Arc::new(AuthState {
        users_path: path.clone(),
        signer,
        ttl_secs,
        lockout: Lockout::default(),
        events: Mutex::new(events),
    });

    let tls = tls_config::load_server_config(
        Path::new(&env_or(
            "SDV_TLS_CLIENT_CA_CERT",
            "pki/dev/client-ca/ca.crt",
        )),
        Path::new(&env_or(
            "SDV_TLS_SERVER_CERT",
            "pki/dev/central/central-server.crt",
        )),
        Path::new(&env_or(
            "SDV_TLS_SERVER_KEY",
            "pki/dev/central/central-server.key",
        )),
    )
    .unwrap_or_else(|e| panic!("[AUTH] TLS configuration failed: {e}"));

    let bind = env_or("SDV_AUTH_BIND", "0.0.0.0:8020");
    let addr: std::net::SocketAddr = bind
        .parse()
        .unwrap_or_else(|_| panic!("[AUTH] bad SDV_AUTH_BIND {bind}"));
    println!("[AUTH] Listening at https://{bind} (TLS 1.3, mTLS required)");
    println!("[AUTH] Users: {} in {}", users.users.len(), path.display());
    println!(
        "[AUTH] Token lifetime: {ttl_secs} s; lockout after {} failures",
        crate::login::MAX_FAILURES
    );

    axum_server::bind_rustls(addr, tls)
        .serve(router(state).into_make_service())
        .await
        .expect("[AUTH] server failed");
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair};

    fn signer() -> TokenSigner {
        use base64::Engine;
        let rng = ring::rand::SystemRandom::new();
        let der = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng).unwrap();
        let b64 = base64::engine::general_purpose::STANDARD.encode(der.as_ref());
        let pem = format!("-----BEGIN PRIVATE KEY-----\n{b64}\n-----END PRIVATE KEY-----\n");
        TokenSigner::from_pkcs8_pem(pem.as_bytes()).unwrap()
    }

    async fn serve_plain() -> (String, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "velo-auth-{}-{}",
            std::process::id(),
            unix_now() as u128 * 1000 + Instant::now().elapsed().as_nanos()
        ));
        let path = dir.join("users.toml");
        let mut store = UserStore::default();
        store
            .add("hari", "technician", "correct password", false)
            .unwrap();
        store.save(&path).unwrap();
        let state = Arc::new(AuthState {
            users_path: path.clone(),
            signer: signer(),
            ttl_secs: 900,
            lockout: Lockout::default(),
            events: Mutex::new(None),
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, router(state)).await.unwrap() });
        (format!("http://{addr}/token"), path)
    }

    async fn login(
        url: &str,
        user: &str,
        pw: &str,
        grant: &str,
    ) -> (u16, serde_json::Value, Option<String>) {
        let r = reqwest::Client::new()
            .post(url)
            .form(&[("grant_type", grant), ("username", user), ("password", pw)])
            .send()
            .await
            .unwrap();
        let cache = r
            .headers()
            .get("cache-control")
            .map(|v| v.to_str().unwrap().to_string());
        (r.status().as_u16(), r.json().await.unwrap(), cache)
    }

    #[tokio::test]
    async fn issues_token_for_valid_login() {
        let (url, _) = serve_plain().await;
        let (status, body, cache) = login(&url, "hari", "correct password", "password").await;
        assert_eq!(status, 200);
        assert_eq!(body["token_type"], "Bearer");
        assert_eq!(body["role"], "technician");
        assert_eq!(body["expires_in"], 900);
        assert_eq!(body["access_token"].as_str().unwrap().split('.').count(), 3);
        assert_eq!(cache.as_deref(), Some("no-store"));
    }

    #[tokio::test]
    async fn same_error_for_wrong_password_and_unknown_user() {
        let (url, _) = serve_plain().await;
        let (s1, b1, _) = login(&url, "hari", "wrong password", "password").await;
        let (s2, b2, _) = login(&url, "nobody", "whatever pw", "password").await;
        assert_eq!((s1, s2), (400, 400));
        assert_eq!(b1, b2);
        assert_eq!(b1["error"], "invalid_grant");
    }

    #[tokio::test]
    async fn rejects_other_grant_types() {
        let (url, _) = serve_plain().await;
        let (s, b, _) = login(&url, "hari", "correct password", "client_credentials").await;
        assert_eq!(s, 400);
        assert_eq!(b["error"], "unsupported_grant_type");
    }

    #[tokio::test]
    async fn disabling_a_user_applies_without_restart() {
        let (url, path) = serve_plain().await;
        assert_eq!(
            login(&url, "hari", "correct password", "password").await.0,
            200
        );
        let mut store = UserStore::load(&path).unwrap();
        store.set_enabled("hari", false).unwrap();
        store.save(&path).unwrap();
        assert_eq!(
            login(&url, "hari", "correct password", "password").await.0,
            400
        );
    }
}
