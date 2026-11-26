//! Login decision: password check, account state, brute-force lockout.

use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

use crate::store::{UserStore, verify_password};

pub const MAX_FAILURES: u32 = 5;
pub const LOCKOUT: Duration = Duration::from_secs(15 * 60);

/// Real argon2id hash (same parameters as user hashes) of a throw-away
/// password. Checked for unknown users so the response time does not reveal
/// whether a username exists.
fn dummy_hash() -> &'static str {
    static HASH: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    HASH.get_or_init(|| {
        crate::store::hash_password("dummy-password-for-timing").unwrap_or_default()
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Denied {
    BadCredentials,
    Disabled,
    Locked,
}

impl Denied {
    pub fn reason(self) -> &'static str {
        match self {
            Self::BadCredentials => "bad-credentials",
            Self::Disabled => "account-disabled",
            Self::Locked => "account-locked",
        }
    }
}

#[derive(Default)]
pub struct Lockout {
    state: Mutex<HashMap<String, (u32, Option<Instant>)>>,
}

impl Lockout {
    fn is_locked(&self, user: &str, now: Instant) -> bool {
        let mut map = self.state.lock().unwrap_or_else(|e| e.into_inner());
        match map.get(user) {
            Some((_, Some(until))) if now < *until => true,
            Some((_, Some(_))) => {
                map.remove(user); // lock expired
                false
            }
            _ => false,
        }
    }

    fn fail(&self, user: &str, now: Instant) {
        let mut map = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let entry = map.entry(user.to_string()).or_insert((0, None));
        entry.0 += 1;
        if entry.0 >= MAX_FAILURES {
            entry.1 = Some(now + LOCKOUT);
        }
    }

    fn success(&self, user: &str) {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(user);
    }
}

/// Returns the user's role on success.
pub fn authenticate(
    store: &UserStore,
    lockout: &Lockout,
    username: &str,
    password: &str,
    now: Instant,
) -> Result<String, Denied> {
    if lockout.is_locked(username, now) {
        return Err(Denied::Locked);
    }
    let Some(user) = store.find(username) else {
        let _ = verify_password(password, dummy_hash()); // equalize timing
        lockout.fail(username, now);
        return Err(Denied::BadCredentials);
    };
    if !verify_password(password, &user.password_hash) {
        lockout.fail(username, now);
        return Err(Denied::BadCredentials);
    }
    if !user.enabled {
        return Err(Denied::Disabled);
    }
    lockout.success(username);
    Ok(user.role.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> UserStore {
        let mut s = UserStore::default();
        s.add("hari", "technician", "correct password", false)
            .unwrap();
        s.add("old", "viewer", "correct password", false).unwrap();
        s.set_enabled("old", false).unwrap();
        s
    }

    #[test]
    fn dummy_hash_is_a_valid_argon2_hash() {
        assert!(argon2::password_hash::PasswordHash::new(dummy_hash()).is_ok());
    }

    #[test]
    fn accepts_good_and_rejects_bad_credentials() {
        let (s, l, now) = (store(), Lockout::default(), Instant::now());
        assert_eq!(
            authenticate(&s, &l, "hari", "correct password", now),
            Ok("technician".into())
        );
        assert_eq!(
            authenticate(&s, &l, "hari", "wrong", now),
            Err(Denied::BadCredentials)
        );
        assert_eq!(
            authenticate(&s, &l, "nobody", "x", now),
            Err(Denied::BadCredentials)
        );
        assert_eq!(
            authenticate(&s, &l, "old", "correct password", now),
            Err(Denied::Disabled)
        );
    }

    #[test]
    fn locks_after_five_failures_and_unlocks_later() {
        let (s, l, now) = (store(), Lockout::default(), Instant::now());
        for _ in 0..MAX_FAILURES {
            assert_eq!(
                authenticate(&s, &l, "hari", "wrong", now),
                Err(Denied::BadCredentials)
            );
        }
        // Even the right password is refused while locked.
        assert_eq!(
            authenticate(&s, &l, "hari", "correct password", now),
            Err(Denied::Locked)
        );
        let later = now + LOCKOUT + Duration::from_secs(1);
        assert_eq!(
            authenticate(&s, &l, "hari", "correct password", later),
            Ok("technician".into())
        );
    }

    #[test]
    fn success_resets_failure_count() {
        let (s, l, now) = (store(), Lockout::default(), Instant::now());
        for _ in 0..MAX_FAILURES - 1 {
            let _ = authenticate(&s, &l, "hari", "wrong", now);
        }
        assert!(authenticate(&s, &l, "hari", "correct password", now).is_ok());
        for _ in 0..MAX_FAILURES - 1 {
            let _ = authenticate(&s, &l, "hari", "wrong", now);
        }
        assert!(authenticate(&s, &l, "hari", "correct password", now).is_ok());
    }
}
