# Evidence: M1 user accounts, live test (2026-11-26)

**Scope:** M1 user accounts and roles on the central SOVD server: device identity (mTLS) plus user identity (bearer token from `auth-service`).
**Environment:** Linux developer laptop, development PKI and auth keys from `tools/security/generate_auth_keys.sh`. Device `velosovd-studio` (device role engineer, `require_user_token = true`). Three users: `vino` (Viewer), `hari` (Technician), `ravi` (Engineer).
**Commit under test:** `025f025` (merge of PR #27, M1 user accounts).
**Requirements:** `PRJ-SEC-14`, `PRJ-SEC-15`, with supporting rows `PRJ-SEC-03`, `PRJ-SEC-01`, `PRJ-SEC-07` in `docs/requirements/requirements.csv`.

This record retains the observed results of one manual live run. It does not
establish ISO 17978, ISO 14229, ISO/SAE 21434 or UNECE R155 conformance.
Credentials and login commands are intentionally not recorded.

## Expected vs observed

All six checks matched. Base path `/api/ecus/ECM/diagnostics`.

| # | Caller | Request | Expected | Observed | Requirement |
|---|---|---|---|---|---|
| 1 | device only, no user token | GET `faults` | 401 | 401 `user-token-required` | PRJ-SEC-14 |
| 2 | `vino` (Viewer) | GET `faults` | 200 | 200 | PRJ-SEC-03, PRJ-SEC-14 |
| 3 | `vino` (Viewer) | DELETE `faults` | 403 | 403 `insufficient-access-rights` | PRJ-SEC-03, PRJ-SEC-14 |
| 4 | `hari` (Technician) | DELETE `faults` | 204 | 204 | PRJ-SEC-03 |
| 5 | `hari` (Technician) | POST `requests`, raw UDS `22 F1 90` | 403 | 403 `insufficient-access-rights` | PRJ-SEC-03, PRJ-SEC-14 |
| 6 | `ravi` (Engineer) | POST `requests`, raw UDS `22 F1 90` | 200 | 200 | PRJ-SEC-01, PRJ-SEC-03 |

Check 5 shows the effective role rule: device role is engineer, user role is
technician, so the effective role is technician and raw UDS is refused.

## Audit lines

Source: `audit/m1-test.jsonl`. Columns: seq, principal (device), user, role, method, path, status, decision. `-` means the record has no user.

| seq | principal | user | role | method | path | status | decision |
|---|---|---|---|---|---|---|---|
| 2 | velosovd-studio | - | engineer | GET | /ecus/ECM/diagnostics/faults | 401 | deny |
| 3 | velosovd-studio | vino | viewer | GET | /ecus/ECM/diagnostics/faults | 200 | allow |
| 4 | velosovd-studio | vino | viewer | DELETE | /ecus/ECM/diagnostics/faults | 403 | deny |
| 5 | velosovd-studio | hari | technician | DELETE | /ecus/ECM/diagnostics/faults | 204 | allow |
| 6 | velosovd-studio | hari | technician | POST | /ecus/ECM/diagnostics/requests | 403 | deny |
| 7 | velosovd-studio | ravi | engineer | POST | /ecus/ECM/diagnostics/requests | 200 | allow |

**Note on seq 2:** no user token was sent, so there is no user. The `role`
column shows the device role (engineer), not a user role. A reader must not
take seq 2 as an engineer user being denied.

Rows 1 to 6 of the table above map to seq 2 to 7 in order. Both 401 and 403
are recorded as `deny` (PRJ-SEC-15); allowed requests are recorded as `allow`.

## Not observed in this run

- Audit hash-chain verification (`--verify-audit`) was not run on `audit/m1-test.jsonl`. Chain integrity remains covered by PRJ-SEC-08 and its R4 evidence only.
- The token id (`token_id`) field of the audit record is not in the supplied lines. PRJ-SEC-15 states it is recorded; this run does not show it.
- Seq 1 is not in the supplied lines.
- Login events of `auth-service` (`auth/auth-events.jsonl`) were not inspected.

## Requirement coverage

| Requirement | What this run shows |
|---|---|
| PRJ-SEC-14 | Device requiring a token gets 401 without one (check 1). Effective role is the lower of device and user role (checks 3 and 5). |
| PRJ-SEC-15 | Verified user named in audit records for allow and deny (seq 3 to 7). 401 and 403 both recorded as `deny`. Token id not shown, see above. |
| PRJ-SEC-03 | Viewer reads only, Technician clears faults, raw UDS needs Engineer (checks 2 to 6). |
| PRJ-SEC-01 | Raw `22 F1 90` is allow-listed and forwarded for Engineer (check 6). |
| PRJ-SEC-07 | One audit record per request including denied ones (seq 2 to 7). |

## M1 behaviors with no PRJ row

| Behavior (source) | Existing coverage | Gap |
|---|---|---|
| `POST /token` contract: password grant, 200 body fields, other grant types refused (`docs/USER_ACCOUNTS.md`) | tests `auth-service::server::tests::issues_token_for_valid_login` and `auth-service::server::tests::rejects_other_grant_types` exist but are in no register row | no row for the endpoint itself |
| Login event log of auth-service (`auth/auth-events.jsonl`) | only mentioned in the notes of PRJ-SEC-15 and in DEV-AUTH-01 | no row requiring that logins and failures are recorded |
| User administration CLI: `passwd`, `set-role` (applies at next login), `enable`/`disable`, `list`, `--admin` | PRJ-SEC-11 covers hashing and length; PRJ-SEC-12 covers disable without restart | no row for `passwd`, `set-role`, `enable`, `list`, `--admin` |
| auth-service transport: TLS with client-certificate verification (`crates/auth-service/src/tls_config.rs`) | DEV-AUTH-06 (shared server certificate) has source `—` | no row for the service's own transport security |
| Central answer `401 invalid-token` for bad, expired or foreign tokens | test `sovd-server::authz::tests::bad_expired_or_foreign_tokens_get_401` is in PRJ-SEC-14 | covered, but not observed live in this run |
