# ADR 0002: SOVD API base-path strategy

- **Status:** Accepted, implementation pending
- **Date:** 2026-11-17
- **Decision owner:** Project owner
- **Scope:** Central Rust HTTP server, host proxy, Vite development proxy, frontend API client, tests and developer documentation
- **Related assessment:** [API gap matrix](../engineering/API_GAP_MATRIX.md)

## 1. Context

The supplied ISO 17978-3 OpenAPI package describes its API under the server base path `/v1`. The current Rust router registers the demonstrator's standard-shaped execution routes under `/sovd/v1/...`, while the custom demonstrator API is registered under `/api/...`.

The current frontend calls the custom `/api` endpoints. In `sovd-studio/vite.config.ts`, Vite proxies only `/api` to `http://127.0.0.1:18010`; it has no proxy entry for `/v1` or `/sovd/v1`. The host proxy in `sovd-studio/tools/start_sovd_proxy.sh` is a TCP forwarder from host port 18010 into the central network namespace at port 8010. It does not perform HTTP path rewriting.

Therefore, a standard API path decision affects more than Axum route registration. The proxy and any frontend feature that consumes the standard API must agree on the externally visible path. At present, the frontend is dependent on the custom `/api` contract and does not consume the standard-shaped execution routes through a `/v1` proxy.

The project owner has approved Option A, subject to the inventory and standards-review conditions recorded below. This ADR records an accepted architectural direction; implementation remains pending. No route, proxy, frontend or runtime change is authorized by this document yet.

## 2. Decision drivers

1. Make the externally visible API base path unambiguous.
2. Keep the public path aligned with the supplied OpenAPI server base path (`/v1`).
3. Avoid hidden path rewriting where possible.
4. Preserve the existing custom `/api` demonstrator endpoints unless separately deprecated.
5. Keep local development and any future non-Vite client consistent.
6. Make route behavior directly testable at the central server boundary.
7. Avoid claiming full SOVD conformance merely because a prefix matches.

## 3. Options considered

### Option A — Serve standard routes directly at `/v1`; retain custom routes at `/api`

The Rust server registers standard API routes under `/v1/...`. Existing demonstrator-specific routes remain under `/api/...`. The host TCP proxy remains a transparent transport forwarder. Vite adds a `/v1` proxy entry alongside the existing `/api` entry, both targeting the host proxy on port 18010.

**Advantages**
- The path seen by clients matches the OpenAPI package's `/v1` base path.
- No HTTP path rewrite is required in the host proxy or Vite.
- Direct server tests can exercise the externally visible route path.
- Custom and standard API surfaces remain explicitly separate.

**Costs / risks**
- Rust route registration and route tests must change from `/sovd/v1` to `/v1`.
- Any client currently calling `/sovd/v1` must be updated.
- Vite must proxy `/v1`; otherwise browser calls to that path will not reach the server.
- The custom `/api` endpoints remain non-standard and need to be documented as demonstrator APIs.

### Option B — Keep Rust routes at `/sovd/v1`; expose `/v1` through a rewriting proxy

The Rust router retains its current internal `/sovd/v1/...` paths. An HTTP-aware proxy accepts external `/v1/...` requests and rewrites the path to `/sovd/v1/...` before forwarding to the central server.

**Advantages**
- Existing Rust route paths and server-side tests can remain unchanged initially.
- External clients can use `/v1`.
- The internal prefix can remain an implementation detail.

**Costs / risks**
- The current host proxy is `socat` TCP forwarding and cannot rewrite HTTP paths.
- Vite's proxy rewrite would affect only Vite development traffic; it would not define a consistent path for direct host clients or future deployments.
- A new HTTP-aware reverse-proxy component or equivalent server-side rewrite is required for a general deployment.
- Debugging and test coverage must distinguish external and internal paths.
- Proxy rewrite mistakes can produce double prefixes, incorrect Location links, or inconsistent OpenAPI URLs.

### Option C — Serve both `/v1` and `/sovd/v1` during a transition

The Rust server registers aliases for both prefixes, or a controlled compatibility layer maps one to the other. The custom `/api` routes remain unchanged.

**Advantages**
- Existing clients using `/sovd/v1` can continue to work while new clients adopt `/v1`.
- Migration can be staged.
- No immediate client cutover is required.

**Costs / risks**
- Two externally visible paths represent the same capability and must remain behaviorally identical.
- Every relevant test must cover both aliases or prove that the shared handler mapping is correct.
- Documentation and generated API descriptions must clearly identify the canonical path.
- Compatibility aliases can become permanent unless a removal condition and date are defined.
- More route registrations increase maintenance and the risk of inconsistent future changes.

## 4. Impact comparison

| Area | Option A: direct `/v1` | Option B: proxy rewrite | Option C: dual paths |
|---|---|---|---|
| Rust router | Change standard-shaped route registrations from `/sovd/v1` to `/v1`; keep `/api` routes | Keep current `/sovd/v1` registrations | Register both prefixes or add a shared alias layer |
| Host proxy | No path rewrite; existing TCP forwarding can remain | Replace/augment TCP forwarder with an HTTP-aware rewrite proxy for general clients | No rewrite required if Axum serves both; otherwise proxy must map aliases |
| Vite proxy | Add `/v1` target to existing `/api` proxy configuration | Add `/v1` with a rewrite to `/sovd/v1`; this alone is dev-only | Add `/v1` proxy entry; optionally preserve `/sovd/v1` for compatibility |
| Frontend API client | Keep existing custom `/api` calls; use `/v1` for any standard API feature | Client calls `/v1`; Vite rewrites in development; production path needs equivalent proxy | New calls use canonical `/v1`; old calls may continue temporarily |
| Rust route tests | Update expected paths to `/v1`; test `/api` separately | Test internal `/sovd/v1`; add proxy integration tests for external `/v1` | Test both prefixes and assert equivalent status/body/headers |
| Proxy/integration tests | Verify `/v1` and `/api` traverse the transparent proxy | Verify rewrite, query preservation, response headers and Location handling | Verify both prefixes through actual proxy and direct server |
| API documentation | Document `/v1` as standard-shaped surface and `/api` as custom demonstrator surface | Document external `/v1` and internal rewrite mapping | Mark `/v1` canonical and `/sovd/v1` temporary compatibility alias |
| Deployment | Simplest path model; no HTTP-aware rewrite dependency | Adds proxy behavior/configuration and another failure point | Temporary route duplication and eventual alias-removal work |

## 5. Decision

**Accepted direction: Option A — serve the standard-shaped routes directly at `/v1`; retain the custom demonstrator routes under `/api`.**

Acceptance is conditional on completing the `/sovd/v1` dependency inventory below and reviewing the base-path rule against the normative ISO 17978-3 text when the project owner has access to it. Acceptance does not establish that `/v1` is normatively correct; the supplied OpenAPI package is informative and is not a substitute for the normative document.

### 5.1 Existing `/sovd/v1` reference inventory

The following references were found in the GitHub `main` source tree at baseline commit `5b3854fa2e0d3c3c377b421fe429692a84e5cd22`:

| File | Location | Context | Classification |
|---|---|---|---|
| `crates/sovd-server/src/main.rs` | line 807 | Constructs the production `Location` header returned by the POST execution handler | Production dependency |
| `crates/sovd-server/src/main.rs` | lines 1062 and 1066 | Registers the POST/GET execution collection and GET/DELETE/PUT execution-item routes | Production dependency |
| `crates/sovd-server/src/main.rs` | lines 1683–1794 | HTTP integration test requests and expected `Location` path for SOVD execution routes | Test dependency |
| `docs/sovd_deviations.md` | lines 210–214 | Lists the implemented SOVD-facing execution paths | Documentation reference |
| `docs/sovd_deviations.md` | line 216 onward | Explains that the prefix is a local implementation choice | Documentation reference |
| `docs/adr/0002-api-base-path.md` | throughout | Compares current path and migration options | Architecture documentation reference |
| `docs/engineering/API_GAP_MATRIX.md` | operation-execution rows | Records current route paths and path mismatch | Engineering documentation reference |

**Dependency finding:** It is not correct to say that nothing outside tests and documentation depends on `/sovd/v1`. The Rust production server uses it in both route registration and the `Location` response header. No references were found in the inspected Vite configuration, frontend API client, or host proxy scripts. The frontend currently uses `/api` only. The production server references must be changed together with the route registrations if Option A is implemented; otherwise POST may return a stale `/sovd/v1` Location even after the routes move to `/v1`.

This inventory covers the repository's tracked source, test, script and documentation files inspected at the stated baseline. Re-run a repository-wide exact-string search immediately before implementation to catch references introduced after this baseline.

## 6. Recommendation

**Recommend Option A: direct `/v1` routes in the Rust server, with the existing custom `/api` surface retained separately.**

This is the least surprising architecture for clients and tests: the server itself owns the externally visible standard-shaped path, and the host proxy remains a transport-only component. It also avoids making Vite development configuration the only place where the standard path exists.

This recommendation is not an approval or implementation decision. The project owner must select an option before any route or proxy change is made.

## 7. Migration and verification plan

1. Confirm the exact route set and avoid implying that all 94 OpenAPI operations are implemented.
2. Change only the existing standard-shaped execution routes from `/sovd/v1` to `/v1`; retain `/api` routes.
3. Add a Vite `/v1` proxy entry targeting the same host proxy as `/api`.
4. Update API documentation and any frontend call sites that are intentionally moved to the standard-shaped API.
5. Update route tests to assert the canonical `/v1` path and retain regression tests for custom `/api` behavior.
6. Add an integration check through host port 18010 for both prefixes. Confirm query strings, status codes, response bodies and any `Location` headers.
7. Re-run Rust formatting, Clippy, workspace tests, frontend lint and frontend build.
8. Update the API gap matrix only after the changed routes and contracts are reviewed. A path-prefix change alone must not promote a row to implemented.

## 8. Standards validation and remaining conditions

Before implementation is considered complete, compare the selected `/v1` base-path rule and the relevant route templates against the normative ISO 17978-3 text when it becomes available to the project owner. Record the applicable clause/definition and any required corrections in project-authored wording. Do not infer normative requirements solely from the informative OpenAPI package. If the normative text requires a different base-path arrangement, reopen this ADR before changing routes.

## 9. Remaining implementation checks

- Complete a fresh exact-string search for `/sovd/v1` before implementation.
- Confirm the normative ISO 17978-3 base-path rule when the document is available.
- Confirm whether the frontend should remain exclusively on the custom `/api` demonstrator API for now, or whether a specific feature should begin consuming `/v1`.
- Confirm that the canonical path is reachable directly on central port 8010 and through host port 18010.

## 10. Decision record

**Decision:** Option A accepted by the project owner, subject to the dependency inventory and normative ISO 17978-3 base-path check.

**ADR status:** Accepted, implementation pending.

**Implementation status:** Not implemented. No source, route, proxy, frontend, test or runtime behavior has been changed.
