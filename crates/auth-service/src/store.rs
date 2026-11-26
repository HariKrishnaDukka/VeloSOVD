//! User accounts file (`users.toml`): names, argon2 password hashes, roles.
//!
//! The file holds password HASHES only, never passwords. It must not be
//! committed to git (default location `auth/`, which is git-ignored).

use std::{fs, io, path::Path};

use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};

pub const ROLES: [&str; 3] = ["viewer", "technician", "engineer"];
pub const MIN_PASSWORD_LEN: usize = 10;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct User {
    pub username: String,
    pub password_hash: String,
    /// Diagnostic role sent to central: viewer | technician | engineer.
    pub role: String,
    /// Tool administrator (may manage users in the Studio app, later).
    #[serde(default)]
    pub admin: bool,
    #[serde(default = "enabled_default")]
    pub enabled: bool,
}

fn enabled_default() -> bool {
    true
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserStore {
    #[serde(rename = "user", default)]
    pub users: Vec<User>,
}

pub fn valid_username(name: &str) -> bool {
    (1..=32).contains(&name.len())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "._-".contains(c))
}

pub fn valid_role(role: &str) -> bool {
    ROLES.contains(&role)
}

pub fn hash_password(password: &str) -> Result<String, String> {
    if password.chars().count() < MIN_PASSWORD_LEN {
        return Err(format!(
            "password must have at least {MIN_PASSWORD_LEN} characters"
        ));
    }
    let mut salt = [0u8; 16];
    SystemRandom::new()
        .fill(&mut salt)
        .map_err(|_| "random generation failed".to_string())?;
    let salt = SaltString::encode_b64(&salt).map_err(|e| e.to_string())?;
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| e.to_string())
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    PasswordHash::new(hash)
        .map(|parsed| {
            Argon2::default()
                .verify_password(password.as_bytes(), &parsed)
                .is_ok()
        })
        .unwrap_or(false)
}

impl UserStore {
    pub fn load(path: &Path) -> Result<Self, String> {
        match fs::read_to_string(path) {
            Ok(text) => {
                let store: Self = toml::from_str(&text).map_err(|e| format!("{path:?}: {e}"))?;
                store.validate()?;
                Ok(store)
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(format!("{path:?}: {e}")),
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        self.validate()?;
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let text = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        // Write to a temp file then rename: never leave a half-written store.
        let tmp = path.with_extension("toml.tmp");
        fs::write(&tmp, text).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))
                .map_err(|e| e.to_string())?;
        }
        fs::rename(&tmp, path).map_err(|e| e.to_string())
    }

    pub fn validate(&self) -> Result<(), String> {
        let mut seen = std::collections::HashSet::new();
        for u in &self.users {
            if !valid_username(&u.username) {
                return Err(format!("invalid username '{}'", u.username));
            }
            if !valid_role(&u.role) {
                return Err(format!(
                    "user '{}' has invalid role '{}'",
                    u.username, u.role
                ));
            }
            if !seen.insert(&u.username) {
                return Err(format!("duplicate user '{}'", u.username));
            }
        }
        Ok(())
    }

    pub fn find(&self, name: &str) -> Option<&User> {
        self.users.iter().find(|u| u.username == name)
    }

    fn find_mut(&mut self, name: &str) -> Result<&mut User, String> {
        self.users
            .iter_mut()
            .find(|u| u.username == name)
            .ok_or_else(|| format!("no such user '{name}'"))
    }

    pub fn add(
        &mut self,
        name: &str,
        role: &str,
        password: &str,
        admin: bool,
    ) -> Result<(), String> {
        if !valid_username(name) {
            return Err("username: 1-32 chars of a-z 0-9 . _ -".into());
        }
        if !valid_role(role) {
            return Err(format!("role must be one of {ROLES:?}"));
        }
        if self.find(name).is_some() {
            return Err(format!("user '{name}' already exists"));
        }
        self.users.push(User {
            username: name.into(),
            password_hash: hash_password(password)?,
            role: role.into(),
            admin,
            enabled: true,
        });
        Ok(())
    }

    pub fn set_password(&mut self, name: &str, password: &str) -> Result<(), String> {
        let hash = hash_password(password)?;
        self.find_mut(name)?.password_hash = hash;
        Ok(())
    }

    pub fn set_role(&mut self, name: &str, role: &str) -> Result<(), String> {
        if !valid_role(role) {
            return Err(format!("role must be one of {ROLES:?}"));
        }
        self.find_mut(name)?.role = role.into();
        Ok(())
    }

    pub fn set_enabled(&mut self, name: &str, enabled: bool) -> Result<(), String> {
        self.find_mut(name)?.enabled = enabled;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_and_verifies_passwords() {
        let h = hash_password("correct horse 1").unwrap();
        assert!(h.starts_with("$argon2id$"));
        assert!(verify_password("correct horse 1", &h));
        assert!(!verify_password("wrong password", &h));
        assert!(!verify_password("correct horse 1", "not-a-hash"));
        assert_ne!(
            h,
            hash_password("correct horse 1").unwrap(),
            "salt must differ"
        );
    }

    #[test]
    fn enforces_password_length_username_and_role() {
        let mut s = UserStore::default();
        assert!(s.add("hari", "technician", "short", false).is_err());
        assert!(
            s.add("Hari!", "technician", "long enough pw", false)
                .is_err()
        );
        assert!(s.add("hari", "admin", "long enough pw", false).is_err());
        assert!(s.add("hari", "technician", "long enough pw", false).is_ok());
        assert!(
            s.add("hari", "viewer", "long enough pw", false).is_err(),
            "duplicate"
        );
    }

    #[test]
    fn save_load_roundtrip_and_management() {
        let dir = crate::test_util::unique_dir("users");
        let path = dir.join("users.toml");
        let mut s = UserStore::default();
        s.add("hari", "technician", "long enough pw", true).unwrap();
        s.add("ravi", "viewer", "another long pw", false).unwrap();
        s.set_role("ravi", "engineer").unwrap();
        s.set_enabled("hari", false).unwrap();
        s.set_password("ravi", "a brand new pw").unwrap();
        s.save(&path).unwrap();

        let text = fs::read_to_string(&path).unwrap();
        assert!(
            !text.contains("long enough pw"),
            "plain password must never be stored"
        );

        let loaded = UserStore::load(&path).unwrap();
        assert_eq!(loaded.users, s.users);
        assert!(!loaded.find("hari").unwrap().enabled);
        assert_eq!(loaded.find("ravi").unwrap().role, "engineer");
        assert!(verify_password(
            "a brand new pw",
            &loaded.find("ravi").unwrap().password_hash
        ));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_bad_store_files() {
        assert!(
            toml::from_str::<UserStore>(
                "[[user]]\nusername='a'\npassword_hash='x'\nrole='viewer'\nextra=1\n"
            )
            .is_err()
        );
        let s: UserStore =
            toml::from_str("[[user]]\nusername='a'\npassword_hash='x'\nrole='root'\n").unwrap();
        assert!(s.validate().is_err());
    }
}
