# ADR 0002: SOVD API base-path strategy

- **Status:** Accepted direction; reopened for mount-path and discovery design; implementation pending
- **Date:** 2026-11-17
- **Decision owner:** Project owner
- **Scope:** Central Rust HTTP server, host proxy, Vite development proxy, frontend API client, tests and developer documentation
- **Related assessment:** [API gap matrix](../engineering/API_GAP_MATRIX.md)

## 1. Context

The supplied ISO 17978-3 OpenAPI package describes the versioned API under the server base path `/v1`. VeloSOVD currently registers its demonstrator's standard-shaped operation-execution routes under `/sovd/v1/...`, while its custom API is registered under `/api/...`.

The Eclipse OpenSOVD `opensovd-core` reference (inspected at commit `27cdad1437cc70bb704779ce025a465c6d32ea6b`) separates the deployment mount path from the API version path. Its documented default gateway example advertises `/sovd/v1` as the API base URI and exposes version discovery at `/sovd/version-info`. Its server composes a configurable mount path with the `/v1` version segment. Relevant source files are `opensovd-server/src/server.rs`, `opensovd-server/src/routes/mod.rs`, `opensovd-server/src/routes/version.rs`, and `opensovd-models/src/version.rs`.

This is an implementation reference, not evidence of a normative ISO requirement. The supplied OpenAPI package is informative; the normative REST API definition is in ISO 17978-3.

The VeloSOVD frontend currently calls custom `/api` endpoints. `sovd-studio/vite.config.ts` proxies only `/api` to `http://127.0.0.1:18010`; it has no proxy entry for `/v1` or `/sovd/v1`. The host proxy in `sovd-studio/tools/start_sovd_proxy.sh` is a TCP forwarder from host port 18010 into the central network namespace at port 8010; it does not rewrite HTTP paths.

## 2. Terminology and path model

This ADR distinguishes two independent path elements:

- **Deployment mount path:** configurable prefix where the SOVD service is mounted, e.g. `/sovd`. It may be empty.
- **API version path:** version segment `/v1`, relative to the mount path.

Therefore:

- API base URI: `{origin}{mount_path}/v1`
- Version discovery URI: `{origin}{mount_path}/version-info`

For the Eclipse default example, `mount_path=/sovd`, so the API base is `/sovd/v1` and discovery is `/sovd/version-info`. This ADR does not mandate `/sovd` as VeloSOVD's mount path.

The version-info response must advertise the effective API base URI. Any absolute or relative resource links and operation `Location` headers must be generated consistently from the same configured base-path value. The discovery endpoint is outside `/v1`, so a client can discover the API base before selecting a versioned resource path.

## 3. Decision drivers

1. Keep the API version segment aligned with the supplied OpenAPI package's `/v1` base path, subject to normative ISO review.
2. Separate deployment mounting from API versioning.
3. Provide version discovery at the configured mount root.
4. Avoid HTTP path rewriting where possible.
5. Preserve the custom demonstrator `/api` surface unless separately deprecated.
6. Keep local development, direct clients and deployment routing consistent.
7. Test externally visible paths at the central server boundary.
8. Do not claim SOVD conformance merely because a prefix matches.

## 4. Options

### Option A — Direct versioned routes below a configurable mount path (accepted direction)

Register standard routes at `{mount_path}/v1/...`, and register `GET {mount_path}/version-info` outside the versioned API. The mount path is configurable; `/sovd` is a possible deployment value, not a hard-coded requirement. Retain custom `/api/...` routes separately. Keep the host proxy as a transparent TCP forwarder.

Advantages:
- The version segment remains `/v1`.
- The deployment can choose a mount path without changing resource handlers.
- Clients can discover the effective base URI.
- No HTTP-aware rewrite proxy is required.
- Route and discovery behavior can be tested directly on the server.

Costs / risks:
- Route registration and tests must use the configured mount plus version segment.
- Vite must proxy the configured mounted version path and version-info endpoint.
- Generated `Location` headers and links must use the advertised base URI.
- A path migration must account for the existing `/sovd/v1` production references.

### Option B — Keep an internal path and rewrite externally

Keep the current internal `/sovd/v1` path and introduce an HTTP-aware proxy that exposes a selected mount/version path and version-info endpoint.

Costs / risks:
- The current host proxy is a TCP forwarder and cannot perform this rewrite.
- A new proxy component is required for non-Vite deployments.
- Rewriting must preserve query strings, headers, response paths and `Location` values.
- Vite-only rewriting would not define a consistent deployment contract.

### Option C — Serve compatibility aliases

Serve both a canonical mounted `/v1` path and the old `/sovd/v1` path during a transition.

Costs / risks:
- Aliases must remain behaviorally identical.
- Tests and API documentation must identify the canonical path.
- An explicit removal condition is required to prevent permanent duplicate APIs.

## 5. Decision

**Accepted direction: Option A — direct standard routes at `{mount_path}/v1`, with `GET {mount_path}/version-info` outside the versioned API; retain custom `/api` routes separately.**

The mount path is configurable and may be empty. Eclipse's `/sovd` default is a useful implementation example, not a required VeloSOVD value. The version-info response must advertise the effective `base_uri`. Route registration, generated `Location` headers and advertised links must derive from one consistent configuration.

This is an architectural direction only. Implementation remains on hold until the normative ISO 17978-3 base-path and discovery requirements have been checked. If the normative text requires a different arrangement, reopen this ADR before implementation.

## 6. Existing dependency inventory

The following references were found in the VeloSOVD GitHub `main` source tree at baseline commit `5b3854fa2e0d3c3c377b421fe429692a84e5cd22`:

| File | Location | Context | Classification |
|---|---|---|---|
| `crates/sovd-server/src/main.rs` | line 807 | Constructs production execution `Location` header | Production dependency |
| `crates/sovd-server/src/main.rs` | lines 1062 and 1066 | Registers execution collection and item routes | Production dependency |
| `crates/sovd-server/src/main.rs` | lines 1683–1794 | Integration-test requests and expected `Location` path | Test dependency |
| `docs/sovd_deviations.md` | lines 210–216 onward | Documents current execution paths and prefix choice | Documentation |
| `docs/engineering/API_GAP_MATRIX.md` | operation-execution rows | Records current paths and path mismatch | Documentation |
| `docs/adr/0002-api-base-path.md` | throughout | Records path decision | Documentation |

The production server depends on `/sovd/v1` in both route registration and the `Location` header. Those references must be migrated consistently. No references were found in the inspected Vite configuration, frontend API client or host proxy scripts; the frontend currently uses `/api` only.

This inventory covers the tracked source, tests, scripts and documentation inspected at the stated baseline. Re-run a repository-wide exact-string search immediately before implementation.

## 7. Migration and verification plan

1. Obtain and review the normative ISO 17978-3 text for the API base-path and discovery requirements. Record clause references and project-authored interpretation.
2. Select the VeloSOVD deployment mount path; do not assume it must be `/sovd`.
3. Implement `GET {mount_path}/version-info` and verify its response advertises the effective `{origin}{mount_path}/v1` base URI.
4. Move standard-shaped routes to `{mount_path}/v1`; retain custom `/api` routes.
5. Ensure operation `Location` headers and resource links use the same configured base URI.
6. Add Vite proxy entries for the selected mounted API and version-info endpoint.
7. Test discovery and versioned routes directly on central port 8010 and through host port 18010.
8. Verify query strings, status codes, response bodies, headers and `Location` values.
9. Re-run Rust formatting, Clippy, workspace tests, frontend lint and frontend build.
10. Update the API gap matrix only after route contracts and tests are reviewed. A path-prefix change alone must not promote a row to implemented.

## 8. Standards validation and remaining conditions

The Eclipse source is an implementation reference only. Its route layout and version-info response do not establish what ISO 17978-3 requires. The supplied OpenAPI package is informative and does not replace the normative document.

Before implementation is considered standards-aligned, compare both the versioned base-path rule and the version-info discovery behavior with the normative ISO 17978-3 text. If the standard specifies a different mount/version/discovery arrangement, reopen this ADR and record the correction before changing routes.

## 9. Decision record

**Decision:** Option A accepted as a configurable mount-path plus `/v1` version-path model, with version discovery at the mount root.

**ADR status:** Accepted direction; reopened for mount-path and discovery design; implementation pending.

**Implementation status:** Not implemented. This ADR update changes no source, route, proxy, frontend, test or runtime behavior.
