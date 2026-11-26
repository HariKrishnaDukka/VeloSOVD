//! R7: every error response carries a SOVD-style GenericError JSON body.
//!
//! Many handlers return a bare `StatusCode` (empty body) and axum's own
//! rejections (404 no route, 405, 413, 415, 422) return plain text. This
//! middleware rewrites any >= 400 response that is not already JSON into:
//!
//!     {"error_code": "...", "message": "..."}
//!
//! The HTTP status is never changed.

use axum::{
    Json,
    body::Body,
    http::{HeaderValue, Request, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde_json::json;

/// Stable error code per HTTP status (kebab-case, as in ASAM SOVD GenericError).
pub fn error_code(status: StatusCode) -> &'static str {
    match status {
        StatusCode::BAD_REQUEST => "invalid-request",
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => "insufficient-access-rights",
        StatusCode::NOT_FOUND => "not-found",
        StatusCode::METHOD_NOT_ALLOWED => "method-not-allowed",
        StatusCode::CONFLICT => "precondition-not-fulfilled",
        StatusCode::PAYLOAD_TOO_LARGE => "payload-too-large",
        StatusCode::UNSUPPORTED_MEDIA_TYPE => "unsupported-media-type",
        StatusCode::UNPROCESSABLE_ENTITY => "invalid-request",
        StatusCode::NOT_IMPLEMENTED => "not-implemented",
        StatusCode::BAD_GATEWAY => "not-responding",
        StatusCode::SERVICE_UNAVAILABLE => "server-busy",
        StatusCode::GATEWAY_TIMEOUT => "request-timeout",
        s if s.is_server_error() => "sovd-server-failure",
        _ => "invalid-request",
    }
}

fn is_json(response: &Response) -> bool {
    response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("application/json"))
}

/// Axum middleware: apply with `middleware::from_fn(errors::json_errors)` as the OUTERMOST layer.
pub async fn json_errors(request: Request<Body>, next: Next) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    let response = next.run(request).await;
    let status = response.status();

    if !(status.is_client_error() || status.is_server_error()) || is_json(&response) {
        return response;
    }

    let detail = if status == StatusCode::FORBIDDEN && path.ends_with("/diagnostics/requests") {
        "UDS service is not permitted by the raw passthrough allow-list".to_string()
    } else {
        format!(
            "{method} {path}: {}",
            status.canonical_reason().unwrap_or("error")
        )
    };

    let mut rewritten = (
        status,
        Json(json!({ "error_code": error_code(status), "message": detail })),
    )
        .into_response();
    // Keep headers such as Allow (405) or Location from the original response.
    for (name, value) in response.headers() {
        if name != header::CONTENT_TYPE && name != header::CONTENT_LENGTH {
            rewritten.headers_mut().insert(name.clone(), value.clone());
        }
    }
    rewritten.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    rewritten
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, middleware, routing::get, routing::post};

    async fn serve() -> String {
        let app = Router::new()
            .route("/bare", get(|| async { StatusCode::BAD_GATEWAY }))
            .route(
                "/json",
                get(|| async {
                    (
                        StatusCode::CONFLICT,
                        Json(json!({"error_code": "custom", "message": "keep"})),
                    )
                }),
            )
            .route("/ok", get(|| async { "fine" }))
            .route(
                "/api/ecus/ECM/diagnostics/requests",
                post(|| async { StatusCode::FORBIDDEN }),
            )
            .route(
                "/needs-json",
                post(|_: Json<serde_json::Value>| async { "x" }),
            )
            .layer(middleware::from_fn(json_errors));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}")
    }

    async fn body(r: reqwest::Response) -> (u16, String, serde_json::Value) {
        let status = r.status().as_u16();
        let ct = r
            .headers()
            .get("content-type")
            .map(|v| v.to_str().unwrap().to_string())
            .unwrap_or_default();
        (
            status,
            ct,
            r.json().await.unwrap_or(serde_json::Value::Null),
        )
    }

    #[tokio::test]
    async fn bare_status_gets_generic_error_body() {
        let base = serve().await;
        let (s, ct, b) = body(reqwest::get(format!("{base}/bare")).await.unwrap()).await;
        assert_eq!(s, 502);
        assert_eq!(ct, "application/json");
        assert_eq!(b["error_code"], "not-responding");
    }

    #[tokio::test]
    async fn existing_json_error_is_untouched() {
        let base = serve().await;
        let (s, _, b) = body(reqwest::get(format!("{base}/json")).await.unwrap()).await;
        assert_eq!(s, 409);
        assert_eq!(b["error_code"], "custom");
    }

    #[tokio::test]
    async fn success_and_unknown_route_and_rejections() {
        let base = serve().await;
        let ok = reqwest::get(format!("{base}/ok")).await.unwrap();
        assert_eq!(ok.text().await.unwrap(), "fine");

        let (s, _, b) = body(reqwest::get(format!("{base}/nope")).await.unwrap()).await;
        assert_eq!((s, b["error_code"].as_str()), (404, Some("not-found")));

        let c = reqwest::Client::new();
        let (s, _, b) = body(c.post(format!("{base}/ok")).send().await.unwrap()).await;
        assert_eq!(
            (s, b["error_code"].as_str()),
            (405, Some("method-not-allowed"))
        );

        let (s, _, b) = body(
            c.post(format!("{base}/needs-json"))
                .body("x")
                .send()
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(
            (s, b["error_code"].as_str()),
            (415, Some("unsupported-media-type"))
        );
    }

    #[tokio::test]
    async fn raw_uds_denial_has_specific_message() {
        let base = serve().await;
        let r = reqwest::Client::new()
            .post(format!("{base}/api/ecus/ECM/diagnostics/requests"))
            .send()
            .await
            .unwrap();
        let (s, _, b) = body(r).await;
        assert_eq!(s, 403);
        assert_eq!(b["error_code"], "insufficient-access-rights");
        assert!(b["message"].as_str().unwrap().contains("allow-list"));
    }
}
