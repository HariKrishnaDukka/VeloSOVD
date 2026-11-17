//! Diagnostic session state management.

use std::time::{Duration, Instant};

/// Demonstrator default only; production S3 timing must be ECU-configured.
pub const DEFAULT_S3_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum DiagnosticSession {
    #[default]
    Default,
    Extended,
}

#[derive(Debug)]
pub struct SessionManager {
    current: DiagnosticSession,
    s3_timeout: Duration,
    last_activity: Option<Instant>,
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::with_s3_timeout(DEFAULT_S3_TIMEOUT)
    }
}

impl SessionManager {
    pub fn with_s3_timeout(s3_timeout: Duration) -> Self {
        Self {
            current: DiagnosticSession::Default,
            s3_timeout,
            last_activity: None,
        }
    }

    pub fn current(&self) -> DiagnosticSession {
        self.current
    }

    pub fn transition_at(&mut self, session: DiagnosticSession, now: Instant) {
        self.current = session;
        self.last_activity = if session == DiagnosticSession::Extended {
            Some(now)
        } else {
            None
        };
    }

    pub fn refresh_activity_at(&mut self, now: Instant) {
        if self.current == DiagnosticSession::Extended {
            self.last_activity = Some(now);
        }
    }

    pub fn expire_if_needed(&mut self, now: Instant) -> bool {
        let expired = self.current == DiagnosticSession::Extended
            && self
                .last_activity
                .is_some_and(|last| now.saturating_duration_since(last) >= self.s3_timeout);
        if expired {
            self.transition_at(DiagnosticSession::Default, now);
        }
        expired
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn s3_expiry_returns_to_default() {
        let start = Instant::now();
        let mut manager = SessionManager::with_s3_timeout(Duration::from_secs(5));
        manager.transition_at(DiagnosticSession::Extended, start);
        assert!(!manager.expire_if_needed(start + Duration::from_secs(4)));
        assert!(manager.expire_if_needed(start + Duration::from_secs(5)));
        assert_eq!(manager.current(), DiagnosticSession::Default);
    }

    #[test]
    fn activity_refreshes_s3_deadline() {
        let start = Instant::now();
        let mut manager = SessionManager::with_s3_timeout(Duration::from_secs(5));
        manager.transition_at(DiagnosticSession::Extended, start);
        manager.refresh_activity_at(start + Duration::from_secs(4));
        assert!(!manager.expire_if_needed(start + Duration::from_secs(8)));
        assert!(manager.expire_if_needed(start + Duration::from_secs(9)));
    }
}
