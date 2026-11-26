//! VeloSOVD auth-service (M1): user accounts and access tokens.
//!
//! Server:  `auth-service serve`  (HTTPS + mTLS, `POST /token`)
//! Admin:   `auth-service user add <name> --role <viewer|technician|engineer> [--admin]`
//!          `auth-service user passwd <name>`
//!          `auth-service user set-role <name> <role>`
//!          `auth-service user disable <name>` / `enable <name>`
//!          `auth-service user list`
//!
//! Passwords for `add` / `passwd` are read from standard input (one line).
//!
//! Environment:
//!   SDV_USERS             users file            (default auth/users.toml)
//!   SDV_AUTH_BIND         listen address        (default 0.0.0.0:8020)
//!   SDV_TOKEN_SIGNING_KEY PKCS#8 P-256 key PEM  (default pki/dev/auth/token-signing.key)
//!   SDV_TOKEN_TTL_SECS    token lifetime        (default 900)
//!   SDV_AUTH_LOG          login event log       (default auth/auth-events.jsonl)
//!   SDV_TLS_CLIENT_CA_CERT / SDV_TLS_SERVER_CERT / SDV_TLS_SERVER_KEY   (mTLS)

mod login;
mod server;
mod store;
#[cfg(test)]
mod test_util;
mod tls_config;

use std::{io::BufRead, path::PathBuf};

use store::UserStore;

fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_string())
}

fn users_path() -> PathBuf {
    PathBuf::from(env_or("SDV_USERS", "auth/users.toml"))
}

fn read_password() -> Result<String, String> {
    eprint!("password: ");
    let mut line = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    Ok(line.trim_end_matches(['\r', '\n']).to_string())
}

fn user_command(args: &[String]) -> Result<String, String> {
    let path = users_path();
    let mut store = UserStore::load(&path)?;
    let arg = |i: usize| args.get(i).map(String::as_str).ok_or("missing argument");
    let message = match arg(0)? {
        "list" => {
            let mut out = format!(
                "{:<20} {:<11} {:<6} {}\n",
                "USER", "ROLE", "ADMIN", "ENABLED"
            );
            for u in &store.users {
                out += &format!(
                    "{:<20} {:<11} {:<6} {}\n",
                    u.username, u.role, u.admin, u.enabled
                );
            }
            return Ok(out);
        }
        "add" => {
            let name = arg(1)?;
            let role_at = args
                .iter()
                .position(|a| a == "--role")
                .ok_or("--role is required")?;
            let role = arg(role_at + 1)?;
            let admin = args.iter().any(|a| a == "--admin");
            store.add(name, role, &read_password()?, admin)?;
            format!(
                "added user '{name}' with role {role}{}",
                if admin { " (admin)" } else { "" }
            )
        }
        "passwd" => {
            let name = arg(1)?;
            store.set_password(name, &read_password()?)?;
            format!("password changed for '{name}'")
        }
        "set-role" => {
            let (name, role) = (arg(1)?, arg(2)?);
            store.set_role(name, role)?;
            format!("role of '{name}' set to {role}")
        }
        "disable" | "enable" => {
            let name = arg(1)?;
            let enable = arg(0)? == "enable";
            store.set_enabled(name, enable)?;
            format!(
                "user '{name}' {}",
                if enable { "enabled" } else { "disabled" }
            )
        }
        other => return Err(format!("unknown user command '{other}'")),
    };
    store.save(&path)?;
    Ok(message)
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("user") => match user_command(&args[1..]) {
            Ok(out) => println!("{}", out.trim_end()),
            Err(e) => {
                eprintln!("error: {e}");
                std::process::exit(2);
            }
        },
        None | Some("serve") => server::serve().await,
        Some(other) => {
            eprintln!(
                "usage: auth-service [serve | user <list|add|passwd|set-role|disable|enable> ...] (got '{other}')"
            );
            std::process::exit(2);
        }
    }
}
