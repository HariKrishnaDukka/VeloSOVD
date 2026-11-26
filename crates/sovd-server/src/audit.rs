//! R4: tamper-evident audit log (UNECE R155 / ISO/SAE 21434 evidence).
//!
//! One JSON object per line (JSON Lines). Each line carries `prev_hash` and
//! `hash = SHA-256(line without "hash")`, so deleting, reordering or editing
//! any line breaks the chain. Verify with:
//!
//!     sovd-server --verify-audit <path>
//!
//! Logged: time, sequence, client principal + role, method, path, HTTP status,
//! allow/deny, duration and, for raw UDS requests, the service ID only.
//! Never logged: request bodies, payload bytes, certificates, keys.

use std::{
    error::Error,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::Path,
    sync::{Arc, Mutex},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use axum::{
    Json,
    body::{Body, to_bytes},
    extract::State,
    http::{Method, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use axum_server::tls::TlsInfo;
use ring::digest::{SHA256, digest};
use serde_json::{Map, Value, json};

use crate::authz::AccessPolicy;
use crate::limits::DEFAULT_MAX_BODY_BYTES;

const GENESIS_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Hash of a line object without its `hash` field. serde_json's default map
/// is sorted by key, so serialization is deterministic.
fn line_hash(object: &Map<String, Value>) -> String {
    let mut copy = object.clone();
    copy.remove("hash");
    let text = Value::Object(copy).to_string();
    hex(digest(&SHA256, text.as_bytes()).as_ref())
}

struct Chain {
    sink: Box<dyn Write + Send>,
    seq: u64,
    prev_hash: String,
}

pub struct AuditLog {
    chain: Mutex<Chain>,
}

impl AuditLog {
    /// Open (append) or create the audit file and continue its hash chain.
    pub fn open(path: &Path) -> Result<Self, Box<dyn Error + Send + Sync>> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent)?;
        }
        let (seq, prev_hash) = match fs::read_to_string(path) {
            Ok(text) => {
                let count = verify_chain(&text).map_err(|e| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("existing audit log fails verification: {e}"),
                    )
                })?;
                let last = text
                    .lines()
                    .rev()
                    .find(|l| !l.trim().is_empty())
                    .and_then(|l| serde_json::from_str::<Map<String, Value>>(l).ok());
                match last {
                    Some(obj) => (
                        count as u64,
                        obj.get("hash")
                            .and_then(Value::as_str)
                            .unwrap_or(GENESIS_HASH)
                            .to_string(),
                    ),
                    None => (0, GENESIS_HASH.to_string()),
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => (0, GENESIS_HASH.to_string()),
            Err(e) => return Err(e.into()),
        };
        let file: File = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Self::with_sink(Box::new(file), seq, prev_hash))
    }

    fn with_sink(sink: Box<dyn Write + Send>, seq: u64, prev_hash: String) -> Self {
        Self {
            chain: Mutex::new(Chain {
                sink,
                seq,
                prev_hash,
            }),
        }
    }

    /// Append one event. `fields` must not contain seq/prev_hash/hash/ts.
    pub fn record(&self, fields: Map<String, Value>) -> io::Result<()> {
        let mut chain = self
            .chain
            .lock()
            .map_err(|_| io::Error::other("audit lock poisoned"))?;
        let mut object = fields;
        object.insert("seq".into(), json!(chain.seq + 1));
        object.insert("ts_unix_ms".into(), json!(unix_ms()));
        object.insert("prev_hash".into(), json!(chain.prev_hash));
        let hash = line_hash(&object);
        object.insert("hash".into(), json!(hash));

        let mut line = Value::Object(object).to_string();
        line.push('\n');
        chain.sink.write_all(line.as_bytes())?;
        chain.sink.flush()?;
        chain.seq += 1;
        chain.prev_hash = hash;
        Ok(())
    }
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or_default()
}

/// Verify a whole audit file. Returns the number of valid lines.
pub fn verify_chain(text: &str) -> Result<usize, String> {
    let mut prev = GENESIS_HASH.to_string();
    let mut expected_seq = 1u64;
    let mut count = 0usize;
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let n = index + 1;
        let object: Map<String, Value> =
            serde_json::from_str(line).map_err(|e| format!("line {n}: not JSON ({e})"))?;
        let stored = object
            .get("hash")
            .and_then(Value::as_str)
            .ok_or(format!("line {n}: missing hash"))?;
        if object.get("prev_hash").and_then(Value::as_str) != Some(prev.as_str()) {
            return Err(format!("line {n}: prev_hash does not match line {}", n - 1));
        }
        if object.get("seq").and_then(Value::as_u64) != Some(expected_seq) {
            return Err(format!("line {n}: sequence gap (expected {expected_seq})"));
        }
        if line_hash(&object) != stored {
            return Err(format!("line {n}: content was modified (hash mismatch)"));
        }
        prev = stored.to_string();
        expected_seq += 1;
        count += 1;
    }
    Ok(count)
}

/// First UDS byte of a raw request body `{"payload_hex": "..."}`.
fn uds_sid(body: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(body).ok()?;
    let hex: String = value
        .get("payload_hex")?
        .as_str()?
        .chars()
        .filter(|c| !c.is_ascii_whitespace())
        .take(2)
        .collect();
    (hex.len() == 2 && hex.chars().all(|c| c.is_ascii_hexdigit()))
        .then(|| format!("0x{}", hex.to_ascii_uppercase()))
}

#[derive(Clone)]
pub struct AuditState {
    pub log: Arc<AuditLog>,
    pub policy: Arc<AccessPolicy>,
}

/// Axum middleware: place OUTSIDE authz so denials are recorded too.
pub async fn audit(
    State(state): State<AuditState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let started = Instant::now();
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    let identity = request
        .extensions()
        .get::<Arc<TlsInfo>>()
        .and_then(|info| state.policy.identify(&info.peer_certificates));

    let mut request = request;
    let mut sid = None;
    if method == Method::POST && path.ends_with("/diagnostics/requests") {
        let (parts, body) = request.into_parts();
        let bytes = match to_bytes(body, DEFAULT_MAX_BODY_BYTES).await {
            Ok(bytes) => bytes,
            Err(_) => {
                let response = (
                    StatusCode::PAYLOAD_TOO_LARGE,
                    Json(json!({"error_code": "invalid-request", "message": "body too large"})),
                )
                    .into_response();
                write_event(&state, &identity, &method, &path, None, &response, started);
                return response;
            }
        };
        sid = uds_sid(&bytes);
        request = Request::from_parts(parts, Body::from(bytes));
    }

    let response = next.run(request).await;
    write_event(&state, &identity, &method, &path, sid, &response, started);
    response
}

fn write_event(
    state: &AuditState,
    identity: &Option<crate::authz::ClientIdentity>,
    method: &Method,
    path: &str,
    sid: Option<String>,
    response: &Response,
    started: Instant,
) {
    let status = response.status();
    let mut fields = Map::new();
    fields.insert("event".into(), json!("http-request"));
    fields.insert(
        "principal".into(),
        json!(
            identity
                .as_ref()
                .map_or("unmapped", |i| i.principal.as_str())
        ),
    );
    fields.insert(
        "role".into(),
        json!(
            identity
                .as_ref()
                .map(|i| format!("{:?}", i.role).to_lowercase())
        ),
    );
    fields.insert("method".into(), json!(method.as_str()));
    fields.insert("path".into(), json!(path));
    fields.insert("status".into(), json!(status.as_u16()));
    fields.insert(
        "decision".into(),
        json!(if status == StatusCode::FORBIDDEN {
            "deny"
        } else {
            "allow"
        }),
    );
    fields.insert(
        "duration_ms".into(),
        json!(started.elapsed().as_millis() as u64),
    );
    if let Some(sid) = sid {
        fields.insert("uds_sid".into(), json!(sid));
    }
    if let Err(error) = state.log.record(fields) {
        eprintln!("[AUDIT] write failed: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Default)]
    struct Shared(Arc<Mutex<Vec<u8>>>);
    impl Write for Shared {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn event(path: &str) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("event".into(), json!("http-request"));
        m.insert("path".into(), json!(path));
        m
    }

    fn three_lines() -> String {
        let buf = Shared::default();
        let log = AuditLog::with_sink(Box::new(buf.clone()), 0, GENESIS_HASH.into());
        for p in ["/a", "/b", "/c"] {
            log.record(event(p)).unwrap();
        }
        String::from_utf8(buf.0.lock().unwrap().clone()).unwrap()
    }

    #[test]
    fn valid_chain_verifies() {
        assert_eq!(verify_chain(&three_lines()), Ok(3));
        assert_eq!(verify_chain(""), Ok(0));
    }

    #[test]
    fn detects_modified_line() {
        let text = three_lines().replace("\"/b\"", "\"/x\"");
        assert!(verify_chain(&text).unwrap_err().contains("line 2"));
    }

    #[test]
    fn detects_deleted_line() {
        let text = three_lines();
        let l: Vec<&str> = text.lines().collect();
        let gap = format!("{}\n{}\n", l[0], l[2]);
        assert!(verify_chain(&gap).unwrap_err().contains("line 2"));
    }

    #[test]
    fn detects_reordered_lines() {
        let text = three_lines();
        let l: Vec<&str> = text.lines().collect();
        let swapped = format!("{}\n{}\n{}\n", l[1], l[0], l[2]);
        assert!(verify_chain(&swapped).is_err());
    }

    #[test]
    fn reopen_continues_chain_and_refuses_tampered_file() {
        let dir = std::env::temp_dir().join(format!("sovd-audit-{}", std::process::id()));
        let path = dir.join("audit.jsonl");
        let _ = fs::remove_dir_all(&dir);

        AuditLog::open(&path)
            .unwrap()
            .record(event("/one"))
            .unwrap();
        AuditLog::open(&path)
            .unwrap()
            .record(event("/two"))
            .unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(verify_chain(&text), Ok(2));

        fs::write(&path, text.replace("/one", "/evil")).unwrap();
        assert!(AuditLog::open(&path).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn extracts_only_service_id() {
        assert_eq!(
            uds_sid(br#"{"payload_hex":"22 F1 90"}"#),
            Some("0x22".into())
        );
        assert_eq!(
            uds_sid(br#"{"payload_hex":"2ef19001"}"#),
            Some("0x2E".into())
        );
        assert_eq!(uds_sid(br#"{"payload_hex":"z"}"#), None);
        assert_eq!(uds_sid(b"not json"), None);
    }
}
