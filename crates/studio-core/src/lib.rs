//! M2: VeloSOVD Studio core - everything the desktop tool does that is not GUI.
//!
//! The Tauri shell is a thin layer that calls this crate, so all of this is
//! unit-tested in CI without a display.
//!
//! | Module | Job |
//! |---|---|
//! | `config` | `studio.toml`: endpoints, certificates, idle timeout |
//! | `tls` | mTLS client with a pinned server identity (tunnels through localhost) |
//! | `session` | login, token held in memory only, idle/expiry logout |
//! | `trace` | ring buffer of every request for the Trace window (F7) |
//! | `faults` | M3: DTC status decoding, role gate, clear confirmation, explorer paths |
//! | `operations` | M3: start/stop rules, `/sovd/v1` execution paths, status parsing |
//! | `polling` | M3: the one polling interval (5 s) for every repeated request to central |
//! | `license` | feature gates - all enabled until the licensing milestone (F16) |

pub mod config;
pub mod faults;
pub mod license;
pub mod operations;
pub mod polling;
pub mod session;
pub mod tls;
pub mod trace;

pub use config::StudioConfig;
pub use session::{ApiResponse, SessionInfo, Studio, StudioError};
