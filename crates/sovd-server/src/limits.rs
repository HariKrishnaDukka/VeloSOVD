//! R3: inbound request limits (DoS protection).
//!
//! - concurrency cap: excess requests get 503 immediately (no queueing)
//! - per-request deadline: slow handlers get 504
//! - body size cap is applied separately with `DefaultBodyLimit`

use std::{sync::Arc, time::Duration};

use axum::{
    Json,
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use tokio::sync::Semaphore;

pub const DEFAULT_MAX_CONCURRENT_REQUESTS: usize = 64;
pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
pub const DEFAULT_MAX_BODY_BYTES: usize = 16 * 1024;

#[derive(Debug)]
pub struct RequestLimits {
    permits: Arc<Semaphore>,
    timeout: Duration,
}

impl RequestLimits {
    pub fn new(max_concurrent: usize, timeout: Duration) -> Self {
        Self {
            permits: Arc::new(Semaphore::new(max_concurrent.max(1))),
            timeout,
        }
    }
}

fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(serde_json::json!({ "error_code": code, "message": message })),
    )
        .into_response()
}

/// Axum middleware: apply with `middleware::from_fn_with_state(limits, enforce)`.
pub async fn enforce(
    State(limits): State<Arc<RequestLimits>>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let Ok(_permit) = Arc::clone(&limits.permits).try_acquire_owned() else {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "server-busy",
            "too many concurrent requests",
        );
    };

    match tokio::time::timeout(limits.timeout, next.run(request)).await {
        Ok(response) => response,
        Err(_) => error(
            StatusCode::GATEWAY_TIMEOUT,
            "request-timeout",
            "request processing exceeded the server deadline",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, extract::DefaultBodyLimit, middleware, routing::get, routing::post};

    async fn serve(limits: RequestLimits) -> String {
        let app = Router::new()
            .route(
                "/slow",
                get(|| async {
                    tokio::time::sleep(Duration::from_millis(400)).await;
                    "done"
                }),
            )
            .route(
                "/echo",
                post(|body: String| async move { body.len().to_string() }),
            )
            .layer(DefaultBodyLimit::max(DEFAULT_MAX_BODY_BYTES))
            .layer(middleware::from_fn_with_state(Arc::new(limits), enforce));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn rejects_requests_over_concurrency_cap() {
        let base = serve(RequestLimits::new(1, Duration::from_secs(5))).await;
        let client = reqwest::Client::new();
        let first = tokio::spawn({
            let (c, u) = (client.clone(), format!("{base}/slow"));
            async move { c.get(u).send().await.unwrap().status() }
        });
        tokio::time::sleep(Duration::from_millis(100)).await;
        let second = client.get(format!("{base}/slow")).send().await.unwrap();
        assert_eq!(second.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(first.await.unwrap(), StatusCode::OK);
    }

    #[tokio::test]
    async fn times_out_slow_requests() {
        let base = serve(RequestLimits::new(4, Duration::from_millis(100))).await;
        let status = reqwest::get(format!("{base}/slow")).await.unwrap().status();
        assert_eq!(status, StatusCode::GATEWAY_TIMEOUT);
    }

    #[tokio::test]
    async fn rejects_oversized_body() {
        let base = serve(RequestLimits::new(4, Duration::from_secs(5))).await;
        let client = reqwest::Client::new();
        let ok = client
            .post(format!("{base}/echo"))
            .body(vec![b'a'; 1024])
            .send()
            .await
            .unwrap();
        assert_eq!(ok.status(), StatusCode::OK);
        let big = client
            .post(format!("{base}/echo"))
            .body(vec![b'a'; DEFAULT_MAX_BODY_BYTES + 1])
            .send()
            .await
            .unwrap();
        assert_eq!(big.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }
}
