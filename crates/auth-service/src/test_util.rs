//! Test-only helpers shared by the unit tests in this crate.

use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

/// Process-wide counter: every call gets a number nobody else has.
static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

/// A fresh, empty, private directory for one test.
///
/// `tag` names the test area, the process id separates test processes, and the
/// counter separates tests inside one process. Never derive the name from a
/// clock: parallel tests read the clock within the same tick and collide.
pub fn unique_dir(tag: &str) -> PathBuf {
    let n = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("velo-{tag}-{}-{n}", std::process::id()));
    // Leftover from an earlier run whose process id was reused.
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create test dir");
    dir
}
