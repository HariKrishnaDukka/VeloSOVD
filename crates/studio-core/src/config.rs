//! `studio.toml` - where the tool connects and with which identity.

use std::{fs, path::Path};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StudioConfig {
    /// auth-service base URL, e.g. `https://127.0.0.1:18020` (via tunnel).
    pub auth_url: String,
    /// Central SOVD server base URL, e.g. `https://127.0.0.1:18010` (via tunnel).
    pub central_url: String,
    /// Name the server certificates must be valid for (DNS name or IP),
    /// independent of the address actually dialled. Dev PKI: `10.77.10.1`.
    pub tls_server_name: String,
    /// CA that signs the server certificates.
    pub ca_cert: String,
    /// Device identity of this Studio installation (mTLS client certificate).
    pub device_cert: String,
    pub device_key: String,
    /// Log the user out after this many seconds without activity.
    #[serde(default = "default_idle")]
    pub idle_timeout_secs: u64,
}

fn default_idle() -> u64 {
    15 * 60
}

impl StudioConfig {
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut cfg: Self =
            toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        // Relative certificate paths are relative to the config file.
        if let Some(dir) = path.parent() {
            for p in [&mut cfg.ca_cert, &mut cfg.device_cert, &mut cfg.device_key] {
                if Path::new(p.as_str()).is_relative() {
                    *p = dir.join(p.as_str()).to_string_lossy().into_owned();
                }
            }
        }
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn validate(&self) -> Result<(), String> {
        for (name, url) in [
            ("auth_url", &self.auth_url),
            ("central_url", &self.central_url),
        ] {
            if !url.starts_with("https://") {
                return Err(format!("{name} must use https:// (got {url})"));
            }
        }
        if self.tls_server_name.trim().is_empty() {
            return Err("tls_server_name must not be empty".into());
        }
        if !(60..=8 * 3600).contains(&self.idle_timeout_secs) {
            return Err("idle_timeout_secs must be between 60 and 28800".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"
auth_url = "https://127.0.0.1:18020"
central_url = "https://127.0.0.1:18010"
tls_server_name = "10.77.10.1"
ca_cert = "pki/ca.crt"
device_cert = "/abs/studio.crt"
device_key = "pki/studio.key"
"#;

    #[test]
    fn loads_and_resolves_relative_paths() {
        let dir = std::env::temp_dir().join(format!("studio-cfg-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("studio.toml");
        fs::write(&path, GOOD).unwrap();
        let cfg = StudioConfig::load(&path).unwrap();
        assert_eq!(cfg.idle_timeout_secs, 900);
        assert!(
            cfg.ca_cert.ends_with("pki/ca.crt") && cfg.ca_cert.starts_with(dir.to_str().unwrap())
        );
        assert_eq!(cfg.device_cert, "/abs/studio.crt");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_plain_http_and_bad_timeouts() {
        let mut cfg: StudioConfig = toml::from_str(GOOD).unwrap();
        cfg.central_url = "http://127.0.0.1:18010".into();
        assert!(cfg.validate().unwrap_err().contains("https"));
        let mut cfg: StudioConfig = toml::from_str(GOOD).unwrap();
        cfg.idle_timeout_secs = 5;
        assert!(cfg.validate().is_err());
        assert!(toml::from_str::<StudioConfig>(&format!("{GOOD}\nextra = 1")).is_err());
    }
}
