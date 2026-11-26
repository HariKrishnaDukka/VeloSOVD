//! Headless check of the Studio connection path (same code the GUI uses).
//!
//!   echo "<password>" | cargo run -p studio-core --example studio_smoke -- \
//!       platform/config/studio.toml <user>
//!
//! Logs in through auth-service (mTLS, pinned server name), then calls
//! central with the token and prints the trace.

use std::io::BufRead;

use studio_core::{Studio, StudioConfig};

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (Some(config), Some(user)) = (args.get(1), args.get(2)) else {
        eprintln!("usage: studio_smoke <studio.toml> <user>   (password on stdin)");
        std::process::exit(2);
    };
    let mut password = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut password)
        .unwrap_or_default();
    let password = password.trim_end_matches(['\r', '\n']);

    let cfg = StudioConfig::load(std::path::Path::new(config)).unwrap_or_else(|e| {
        eprintln!("config: {e}");
        std::process::exit(2)
    });
    let studio = Studio::new(cfg).unwrap_or_else(|e| {
        eprintln!("tls: {e}");
        std::process::exit(2)
    });

    match studio.login(user, password).await {
        Ok(info) => println!(
            "login OK: user={} role={} expires_unix={}",
            info.user, info.role, info.expires_unix
        ),
        Err(e) => {
            println!("login FAILED: {e}");
            std::process::exit(1)
        }
    }
    for (method, path) in [
        ("GET", "/api/ecus/ECM/diagnostics/faults"),
        ("DELETE", "/api/ecus/ECM/diagnostics/faults"),
        ("POST", "/api/ecus/ECM/diagnostics/requests"),
    ] {
        let body = (method == "POST").then(|| r#"{"payload_hex":"22F190"}"#.to_string());
        match studio.api_request(method, path, body).await {
            Ok(r) => println!(
                "{method:6} {path:40} -> {} {}",
                r.status,
                r.body.chars().take(70).collect::<String>()
            ),
            Err(e) => println!("{method:6} {path:40} -> error: {e}"),
        }
    }
    println!("trace entries: {}", studio.trace.since(0).len());
    studio.logout();
    println!(
        "after logout: {:?}",
        studio.api_request("GET", "/api/ecus", None).await.err()
    );
}
