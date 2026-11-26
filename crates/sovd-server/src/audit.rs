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
    path::{Path, PathBuf},
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

pub const DEFAULT_MAX_BYTES: u64 = 10 * 1024 * 1024;

struct Chain {
    sink: Box<dyn Write + Send>,
    seq: u64,
    prev_hash: String,
    bytes: u64,
    /// `None` for in-memory sinks (tests): rotation disabled.
    path: Option<PathBuf>,
    max_bytes: u64,
}

pub struct AuditLog {
    chain: Mutex<Chain>,
}

impl AuditLog {
    /// Open (append) or create the audit file and continue its hash chain.
    #[cfg(test)]
    pub fn open(path: &Path) -> Result<Self, Box<dyn Error + Send + Sync>> {
        Self::open_with_limit(path, DEFAULT_MAX_BYTES)
    }

    pub fn open_with_limit(
        path: &Path,
        max_bytes: u64,
    ) -> Result<Self, Box<dyn Error + Send + Sync>> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent)?;
        }
        let (seq, prev_hash, bytes) = match fs::read_to_string(path) {
            Ok(text) => {
                let summary = verify_from(&text, None).map_err(|e| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("existing audit log fails verification: {e}"),
                    )
                })?;
                (summary.last_seq, summary.last_hash, text.len() as u64)
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => (0, GENESIS_HASH.to_string(), 0),
            Err(e) => return Err(e.into()),
        };
        let file: File = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Self {
            chain: Mutex::new(Chain {
                sink: Box::new(file),
                seq,
                prev_hash,
                bytes,
                path: Some(path.to_path_buf()),
                max_bytes: max_bytes.max(1),
            }),
        })
    }

    #[cfg(test)]
    fn with_sink(sink: Box<dyn Write + Send>, seq: u64, prev_hash: String) -> Self {
        Self {
            chain: Mutex::new(Chain {
                sink,
                seq,
                prev_hash,
                bytes: 0,
                path: None,
                max_bytes: u64::MAX,
            }),
        }
    }

    /// Append one event. `fields` must not contain seq/prev_hash/hash/ts.
    pub fn record(&self, fields: Map<String, Value>) -> io::Result<()> {
        let mut chain = self
            .chain
            .lock()
            .map_err(|_| io::Error::other("audit lock poisoned"))?;
        if chain.path.is_some() && chain.bytes >= chain.max_bytes {
            rotate(&mut chain)?;
        }
        append(&mut chain, fields)
    }

    /// Record a lifecycle event such as `server-start` / `server-stop`.
    pub fn record_event(&self, event: &str, detail: &str) -> io::Result<()> {
        let mut fields = Map::new();
        fields.insert("event".into(), json!(event));
        fields.insert("detail".into(), json!(detail));
        self.record(fields)
    }
}

fn append(chain: &mut Chain, fields: Map<String, Value>) -> io::Result<()> {
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
    chain.bytes += line.len() as u64;
    Ok(())
}

/// Close the current segment and continue the chain in a fresh file.
fn rotate(chain: &mut Chain) -> io::Result<()> {
    let Some(path) = chain.path.clone() else {
        return Ok(());
    };
    chain.sink.flush()?;
    let mut rotated = path.clone().into_os_string();
    rotated.push(format!(".{:012}", chain.seq));
    let rotated = PathBuf::from(rotated);
    fs::rename(&path, &rotated)?;
    chain.sink = Box::new(OpenOptions::new().create(true).append(true).open(&path)?);
    chain.bytes = 0;

    let mut fields = Map::new();
    fields.insert("event".into(), json!("segment-start"));
    fields.insert(
        "previous_segment".into(),
        json!(
            rotated
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
        ),
    );
    append(chain, fields)
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or_default()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainSummary {
    pub entries: usize,
    pub last_seq: u64,
    pub last_hash: String,
    /// True when the text starts at a `segment-start` line instead of genesis.
    pub anchored: bool,
}

/// Verify one segment. `start = None`: the text must begin at genesis or at a
/// `segment-start` line. `start = Some((prev_hash, seq))`: it must continue
/// exactly from there (used to chain several files).
pub fn verify_from(text: &str, start: Option<(String, u64)>) -> Result<ChainSummary, String> {
    let mut expected: Option<(String, u64)> = start.clone();
    let mut anchored = false;
    let mut count = 0usize;
    let mut last = start.unwrap_or((GENESIS_HASH.to_string(), 0));

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
        let prev = object
            .get("prev_hash")
            .and_then(Value::as_str)
            .unwrap_or("");
        let seq = object.get("seq").and_then(Value::as_u64).unwrap_or(0);

        let (want_prev, want_seq) = match &expected {
            Some(e) => e.clone(),
            None => {
                let is_segment =
                    object.get("event").and_then(Value::as_str) == Some("segment-start");
                if prev == GENESIS_HASH && seq == 1 {
                    (GENESIS_HASH.to_string(), 1)
                } else if is_segment && seq > 1 {
                    anchored = true;
                    (prev.to_string(), seq)
                } else {
                    return Err(format!(
                        "line {n}: chain does not start at genesis or a segment-start"
                    ));
                }
            }
        };
        if prev != want_prev {
            return Err(format!(
                "line {n}: prev_hash does not match the previous entry"
            ));
        }
        if seq != want_seq {
            return Err(format!("line {n}: sequence gap (expected {want_seq})"));
        }
        if line_hash(&object) != stored {
            return Err(format!("line {n}: content was modified (hash mismatch)"));
        }
        expected = Some((stored.to_string(), seq + 1));
        last = (stored.to_string(), seq);
        count += 1;
    }
    Ok(ChainSummary {
        entries: count,
        last_seq: last.1,
        last_hash: last.0,
        anchored,
    })
}

/// Verify a whole audit file (genesis or anchored segment). Returns line count.
#[cfg(test)]
pub fn verify_chain(text: &str) -> Result<usize, String> {
    verify_from(text, None).map(|summary| summary.entries)
}

/// Verify several segment files in order (oldest first, current file last).
pub fn verify_files(paths: &[String]) -> Result<ChainSummary, String> {
    let mut total: Option<ChainSummary> = None;
    for path in paths {
        let text = fs::read_to_string(path).map_err(|e| format!("{path}: cannot read ({e})"))?;
        let start = total
            .as_ref()
            .map(|t| (t.last_hash.clone(), t.last_seq + 1));
        let summary = verify_from(&text, start).map_err(|e| format!("{path}: {e}"))?;
        total = Some(match total {
            None => summary,
            Some(t) => ChainSummary {
                entries: t.entries + summary.entries,
                last_seq: if summary.entries == 0 {
                    t.last_seq
                } else {
                    summary.last_seq
                },
                last_hash: if summary.entries == 0 {
                    t.last_hash
                } else {
                    summary.last_hash
                },
                anchored: t.anchored,
            },
        });
    }
    total.ok_or_else(|| "no audit files given".to_string())
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

    fn rotation_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sovd-rot-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn segments(dir: &Path) -> Vec<String> {
        let mut rotated: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path().to_string_lossy().into_owned())
            .filter(|p| !p.ends_with(".jsonl"))
            .collect();
        rotated.sort();
        rotated.push(dir.join("audit.jsonl").to_string_lossy().into_owned());
        rotated
    }

    #[test]
    fn rotation_keeps_chain_continuous_across_files() {
        let dir = rotation_dir("ok");
        let path = dir.join("audit.jsonl");
        let log = AuditLog::open_with_limit(&path, 300).unwrap();
        for i in 0..8 {
            log.record(event(&format!("/req/{i}"))).unwrap();
        }
        drop(log);
        let files = segments(&dir);
        assert!(files.len() >= 3, "expected rotation, got {files:?}");

        let all = verify_files(&files).unwrap();
        assert!(!all.anchored);
        assert_eq!(all.entries as u64, all.last_seq);

        // The current file alone verifies as an anchored segment.
        let current = fs::read_to_string(&path).unwrap();
        assert!(verify_from(&current, None).unwrap().anchored);

        // Reopen continues after the last seq.
        let log = AuditLog::open_with_limit(&path, 300).unwrap();
        log.record(event("/after-restart")).unwrap();
        drop(log);
        assert!(verify_files(&segments(&dir)).is_ok());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_or_tampered_segment_is_detected() {
        let dir = rotation_dir("bad");
        let path = dir.join("audit.jsonl");
        let log = AuditLog::open_with_limit(&path, 300).unwrap();
        for i in 0..8 {
            log.record(event(&format!("/req/{i}"))).unwrap();
        }
        drop(log);
        let files = segments(&dir);

        let mut gap = files.clone();
        gap.remove(1);
        assert!(verify_files(&gap).is_err());

        let text = fs::read_to_string(&files[0]).unwrap();
        fs::write(&files[0], text.replace("/req/0", "/evil")).unwrap();
        assert!(verify_files(&files).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn truncated_head_without_segment_start_is_rejected() {
        let text = three_lines();
        let tail: Vec<&str> = text.lines().skip(1).collect();
        let err = verify_chain(&tail.join("\n")).unwrap_err();
        assert!(err.contains("genesis"));
    }
}
