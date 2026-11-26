# VeloSOVD Studio (desktop tool) - M2

Studio is the desktop diagnostic tool: the existing React UI inside a Tauri
shell, with login, a dockable workspace and a request trace.

```
Studio (host)                                   sdv-central-ns
 React UI ──invoke──► Rust core (studio-core)
                       │ device cert (mTLS) + user token
                       ├─► 127.0.0.1:18020 ══tunnel══► auth-service 10.77.10.1:8020
                       └─► 127.0.0.1:18010 ══tunnel══► central      10.77.10.1:8010
```

- The **user token and device key never enter the web view**. The UI can only
  ask Rust for an allow-listed request (`/api/…`, `/sovd/…`; GET/POST/PUT/DELETE;
  body ≤ 16 KiB).
- TLS is end-to-end through the tunnels. Server certificates are verified for
  `tls_server_name` (`10.77.10.1`), not for the dialled `127.0.0.1`.
- Logout: manual, after `idle_timeout_secs` without activity, at token expiry,
  or when central answers 401.

| Part | Location | Tested in CI |
|---|---|---|
| Core (session, TLS, trace, license hook) | `crates/studio-core` | unit tests (workspace job) |
| Desktop shell | `sovd-studio/src-tauri` | fmt + clippy (studio-desktop job) |
| UI | `sovd-studio/src/shell`, `services/transport.ts` | lint + build (frontend job) |

## Run (development)

Prerequisites (once):

```bash
sudo apt install -y socat libwebkit2gtk-4.1-dev build-essential curl wget file \
  libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
cargo install tauri-cli --version "^2" --locked
bash tools/security/generate_auth_keys.sh        # if not done for M1
```

1. Start network, zonal ECUs, auth-service and central (see `docs/USER_ACCOUNTS.md`).
2. `tools/studio_tunnels.sh up`
3. Headless check (same code path as the GUI):
   `echo '<password>' | cargo run -p studio-core --example studio_smoke -- platform/config/studio.toml <user>`
4. `cd sovd-studio && cargo tauri dev`

Config: `platform/config/studio.toml` (or `SDV_STUDIO_CONFIG`).

## Workspace

Windows: **Vehicle**, **SOVD Explorer**, **Trace**, **Session & License**.
Drag tabs to split or regroup; the layout is saved per user (layout only).
Status bar: central connection, user, role, login expiry, idle timeout, reset layout, logout.

Each window is declared in `src/shell/modules.ts` with a minimum role and a
license feature; the shell only shows windows the user and license allow.
`studio-core::license::feature_enabled` returns `true` for all features until F16.

## Build a package

```bash
cd sovd-studio && cargo tauri build       # .deb and AppImage in src-tauri/target/release/bundle/
```

## Verify and audit

    tools/verify_all.sh                  # fmt + clippy (root, src-tauri), tests, sovd-server gate (86 passed), traceability
    python3 tools/check_dep_age.py       # 14-day rule over every npm lock entry and every src-tauri crate (needs network; first run ~7 min)
