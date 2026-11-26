# User accounts and login (M1)

VeloSOVD separates two identities:

| Identity | Proves | Mechanism |
|---|---|---|
| Device | the tool installation is trusted | mTLS client certificate (e.g. `DNS:velosovd-studio`) |
| User | a named person is logged in, with a role | bearer token from `auth-service` (JWT, ES256, 15 min) |

Central computes the **effective role = min(device role, user role)** and
records both in the audit log (`principal` = device, `user` = person).

## Components

| Component | Role |
|---|---|
| `crates/sovd-auth` | token format, signing (auth-service) and verification (central) |
| `crates/auth-service` | user store, password check, lockout, `POST /token` |
| `platform/config/access-policy.toml` | `require_user_token = true` per device |

## One-time setup (development)

```bash
bash tools/security/generate_auth_keys.sh      # signing key pair + Studio device cert
cargo build --workspace
echo "a-strong-password-1" | ./target/debug/auth-service user add hari --role technician --admin
./target/debug/auth-service user list
```

Files created: `pki/dev/auth/token-signing.{key,pub}`, `pki/dev/studio/velosovd-studio.{crt,key}`,
`auth/users.toml` (argon2 hashes, mode 600). All are git-ignored.

## User administration

| Command | Effect |
|---|---|
| `auth-service user add <name> --role <viewer\|technician\|engineer> [--admin]` | create (password from stdin, min 10 chars) |
| `auth-service user passwd <name>` | change password |
| `auth-service user set-role <name> <role>` | change role (next login) |
| `auth-service user disable <name>` / `enable <name>` | block / allow login (no restart) |
| `auth-service user list` | show users, roles, state |

## Runtime

| Variable | Service | Default |
|---|---|---|
| `SDV_USERS` | auth-service | `auth/users.toml` |
| `SDV_AUTH_BIND` | auth-service | `0.0.0.0:8020` |
| `SDV_TOKEN_SIGNING_KEY` | auth-service | `pki/dev/auth/token-signing.key` |
| `SDV_TOKEN_TTL_SECS` | auth-service | `900` |
| `SDV_AUTH_LOG` | auth-service | `auth/auth-events.jsonl` |
| `SDV_TOKEN_PUBLIC_KEY` | central | unset = tokens not accepted; startup fails if a device requires tokens |

Login (OAuth2 password grant, form-encoded):

```
POST /token   grant_type=password&username=hari&password=...
200 {"access_token":"<jwt>","token_type":"Bearer","expires_in":900,"role":"technician"}
400 {"error":"invalid_grant", ...}        same answer for every login failure
```

Use: `Authorization: Bearer <jwt>` on every central request from a token-requiring device.

| Central answer | Meaning |
|---|---|
| 401 `user-token-required` | device requires a logged-in user, no token sent |
| 401 `invalid-token` | bad signature, expired, wrong issuer/audience, unknown role |
| 403 `insufficient-access-rights` | effective role too low for the request |

Known limitations: `docs/sovd_deviations.md` section 11.
