# Evidence: security hardening R1–R10 (2026-11-26)

**Scope:** central SOVD server hardening items R1–R10e.
**Environment:** Linux developer laptop, virtual network namespaces (`sdv-central-ns`, `sdv-front-ns`, `sdv-rear-ns`), development PKI from `tools/security/generate_dev_pki.sh`.
**Requirements:** `PRJ-SEC-01` … `PRJ-SEC-10`, `PRJ-SW-01` … `PRJ-SW-05` in `docs/requirements/requirements.csv`.

This record retains the observed results of manual live runs. It does not
establish ISO 17978, ISO 14229, ISO/SAE 21434 or UNECE R155 conformance.

## Commits under test

| Item | Commit / PR | Unit tests at that commit |
|---|---|---|
| R1 raw UDS allow-list | `1ea7a56` (PR #19) | sovd-server 54 passed |
| R2 authorization, R3 limits | `21a6f3f` (PR #20) | sovd-server 64 passed |
| R4 audit log | `9f2b0a8` (PR #21) | sovd-server 70 passed |
| R5 single store, R7 JSON errors | `f8bce30` | sovd-server 74 passed |
| R8 zonal port, R9 eager manager, R10 shutdown + rotation | `548d739` | sovd-server 79 passed, vehicle-topology 4 passed |
| R10e strict audit verification | `d1fec02` | sovd-server 80 passed |

`cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets -- -D warnings` passed at each commit.

## R1 — raw UDS allow-list (PRJ-SEC-01)

Client: `harness-client` (engineer). Endpoint: `POST /api/ecus/ECM/diagnostics/requests`.

| Payload | Expected | Observed |
|---|---|---|
| `22 F1 90` (read VIN) | 200 | 200, `62F190…` |
| `10 03` (extended session) | 200 | 200, `5003003201F4` |
| `11 01` (ECU reset) | 403 | 403 |
| `10 02` (programming session) | 403 | 403 |
| `2E F1 90 00` (write DID) | 403 | 403 |

## R2/R3 — authorization (PRJ-SEC-02, PRJ-SEC-03)

| Client certificate | Request | Expected | Observed |
|---|---|---|---|
| `harness-client` (engineer) | GET faults | 200 | 200 |
| `harness-client` (engineer) | raw `22F190` | 200 | 200 |
| `harness-client` (engineer) | raw `1101` | 403 | 403 |
| `workshop-viewer` (viewer) | GET faults | 200 | 200 |
| `workshop-viewer` (viewer) | DELETE faults | 403 | 403 `insufficient-access-rights`, requires Technician |
| `workshop-viewer` (viewer) | raw `22F190` | 403 | 403 `insufficient-access-rights`, requires Engineer |
| `central-client` (not in policy) | GET `/api/ecus` | 403 | 403 `client certificate is not mapped to any role` |

Limits (PRJ-SEC-04..06) were verified by unit tests only in this run.

## R4 — audit log (PRJ-SEC-07, PRJ-SEC-08)

Audit lines produced by four requests:

```
1 harness-client  GET    /api/ecus/ECM/diagnostics/faults   200 allow
2 workshop-viewer DELETE /api/ecus/ECM/diagnostics/faults   403 deny
3 harness-client  POST   /api/ecus/ECM/diagnostics/requests 200 allow 0x22
4 harness-client  POST   /api/ecus/ECM/diagnostics/requests 403 deny  0x11
```

Verification of the unmodified file: `[AUDIT] OK: 4 entries, hash chain intact`.
After changing the first `"decision":"deny"` to `"allow"` in a copy:
`[AUDIT] FAILED: line 2: content was modified (hash mismatch)`, exit 2.

## R5/R7 — single execution store, JSON errors (PRJ-SW-01, PRJ-SW-02)

| Request | Observed |
|---|---|
| `POST /api/ecus/ECM/operations/clear-faults/executions` | 200, `exec-0000000000000001`, `completed`, `success: true` |
| `GET /sovd/v1/components/ECM/operations/clear-faults/executions` | 200, `{"items":["exec-0000000000000001"]}` (same ID) |
| raw `1101` | 403 `application/json`, `insufficient-access-rights` |
| `GET /api/nope` | 404 `application/json`, `not-found` |
| raw `zz` | 400 `application/json`, `invalid-request` |
| `GET /api/ecus/XYZ/…/executions` | 404 `application/json`, `not-found` |

Frontend `npm run build` passed (existing bundle-size warning unchanged).

## R8/R9/R10 — startup, shutdown, rotation (PRJ-SW-03..05, PRJ-SEC-09, PRJ-SEC-10)

Startup log (`SDV_AUDIT_MAX_BYTES=1500`):

```
[SOVD] Zonal endpoint: https://<zonal_ip>:9001 (from topology)
[SOVD] Execution manager: ready
[SOVD] Audit log: …/audit/rot-test/central-audit.jsonl (SHA-256 hash chain, rotate at 1500 bytes)
```

12 × GET faults → 12 × 200. SIGTERM:

```
[SOVD] SIGTERM received; draining requests for up to 10s
[SOVD] Shutdown complete (signal=SIGTERM unfinished_executions=0)
```

Files: `central-audit.jsonl`, `.000000000005`, `.000000000010`, `.000000000015`.
Lifecycle events: seq 1 `server-start`, seq 6/11/16 `segment-start`, seq 17 `server-stop signal=SIGTERM unfinished_executions=0`.

Offline verification (`d1fec02`):

| Files given | Expected exit | Observed |
|---|---|---|
| all segments + current | 0 | 0, `17 entries in 4 file(s) … (complete from genesis)` |
| current only | 2 | 2, `older segment(s) missing before seq 16` |
| oldest segment removed | 2 | 2, `older segment(s) missing before seq 6` |
| oldest removed + `--allow-partial` | 0 | 0, `PARTIAL` |
| middle segment removed + `--allow-partial` | 2 | 2, `prev_hash does not match the previous entry` |

The first run of the R10 check at `548d739` accepted a removed oldest
segment with only a note (exit 0). This was recorded as a defect and fixed in
`d1fec02` (R10e) with regression test `dropping_the_oldest_segment_fails_strict_check`.
