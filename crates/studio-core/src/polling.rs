//! M3 step 2: one polling interval for everything Studio repeats against central.
//!
//! In C terms: one `#define POLL_INTERVAL_SECS 5`. The UI reads it through the Tauri shell
//! (next step) and no panel may use its own number. The Trace window and the status bar
//! only read Studio's own in-memory trace and send nothing to central, so they are not
//! covered by this value.

use std::time::Duration;

/// Seconds between two repeated requests to central (Overview, Explorer, Faults, Operations).
pub const POLL_INTERVAL_SECS: u64 = 5;

pub const POLL_INTERVAL: Duration = Duration::from_secs(POLL_INTERVAL_SECS);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_interval_for_all_central_polls() {
        assert_eq!(POLL_INTERVAL_SECS, 5);
        assert_eq!(POLL_INTERVAL.as_secs(), POLL_INTERVAL_SECS);
        assert_eq!(POLL_INTERVAL.subsec_nanos(), 0);
    }
}
