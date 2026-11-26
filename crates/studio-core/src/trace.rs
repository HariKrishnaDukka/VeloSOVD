//! Request trace for the Trace window (F7). Never stores tokens or passwords.

use std::{collections::VecDeque, sync::Mutex};

use serde::Serialize;

pub const CAPACITY: usize = 500;
pub const BODY_EXCERPT: usize = 4096;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TraceEntry {
    pub seq: u64,
    pub ts_unix_ms: u64,
    pub user: Option<String>,
    pub method: String,
    pub path: String,
    pub status: Option<u16>,
    pub elapsed_ms: u64,
    pub request_excerpt: Option<String>,
    pub response_excerpt: Option<String>,
    pub error: Option<String>,
}

#[derive(Default)]
pub struct Trace {
    inner: Mutex<(u64, VecDeque<TraceEntry>)>,
}

pub fn excerpt(text: &str) -> String {
    if text.len() <= BODY_EXCERPT {
        return text.to_string();
    }
    let mut end = BODY_EXCERPT;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}… ({} bytes)", &text[..end], text.len())
}

impl Trace {
    pub fn push(&self, mut entry: TraceEntry) -> u64 {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.0 += 1;
        entry.seq = guard.0;
        if guard.1.len() == CAPACITY {
            guard.1.pop_front();
        }
        guard.1.push_back(entry);
        guard.0
    }

    /// Entries with `seq > since`, oldest first.
    pub fn since(&self, since: u64) -> Vec<TraceEntry> {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.1.iter().filter(|e| e.seq > since).cloned().collect()
    }

    pub fn clear(&self) {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .1
            .clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str) -> TraceEntry {
        TraceEntry {
            seq: 0,
            ts_unix_ms: 0,
            user: None,
            method: "GET".into(),
            path: path.into(),
            status: Some(200),
            elapsed_ms: 1,
            request_excerpt: None,
            response_excerpt: None,
            error: None,
        }
    }

    #[test]
    fn keeps_order_capacity_and_since() {
        let t = Trace::default();
        for i in 0..(CAPACITY + 10) {
            t.push(entry(&format!("/api/{i}")));
        }
        let all = t.since(0);
        assert_eq!(all.len(), CAPACITY);
        assert_eq!(all.first().unwrap().seq, 11);
        assert_eq!(t.since(all.last().unwrap().seq - 2).len(), 2);
    }

    #[test]
    fn truncates_long_bodies_on_char_boundary() {
        let long = "é".repeat(BODY_EXCERPT);
        let e = excerpt(&long);
        assert!(e.len() < long.len() && e.contains("bytes)"));
        assert_eq!(excerpt("short"), "short");
    }
}
