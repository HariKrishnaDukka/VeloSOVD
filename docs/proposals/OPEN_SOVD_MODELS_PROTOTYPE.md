# Isolated prototype proposal: reuse OpenSOVD models for errors and version discovery

**Status:** Proposal only — no code or dependency changes authorized  
**Reference:** `eclipse-opensovd/opensovd-core` commit `27cdad1437cc70bb704779ce025a465c6d32ea6b`  
**Target repository:** VeloSOVD

## 1. Objective

Build a small, isolated Rust prototype that evaluates whether VeloSOVD can depend on the Eclipse OpenSOVD `opensovd-models` crate for two narrowly scoped contracts:

1. The common SOVD error response types.
2. Version discovery response types used by `GET {mount_path}/version-info`.

The prototype must not replace the existing HTTP server, alter existing `/api` endpoints, migrate operation routes, or change runtime behavior. Its purpose is to test dependency compatibility and serialization behavior before any production integration decision.

## 2. Scope and non-goals

### In scope

- Pin and compile the selected `opensovd-models` crate revision.
- Instantiate the upstream error model and version-info model.
- Serialize representative values to JSON.
- Deserialize the serialized values back into the same upstream types.
- Compare JSON field names, optional-field behavior and status/error mapping assumptions with the upstream source.
- Record the dependency graph, build impact and any incompatibilities with the current VeloSOVD Rust toolchain.

### Explicitly out of scope

- No Axum route registration or server integration.
- No changes to `/api`, `/sovd/v1`, `/v1` or proxy configuration.
- No implementation of version discovery in the running server.
- No adoption of Eclipse data, log, update, DoIP or UDS implementations.
- No claim of ISO 17978-3 conformance.
- No copying of upstream code into production crates during this experiment.

## 3. Proposed prototype layout

Create a standalone workspace member or isolated example crate, subject to the current workspace's conventions:

```text
prototypes/
  opensovd-models-interop/
    Cargo.toml
    src/
      main.rs
    tests/
      serialization.rs
    README.md
```

The prototype should depend only on the minimum required upstream crate and serialization support. Prefer a pinned Git revision rather than a floating branch reference. Record the exact upstream commit and crate package version in its README and lockfile.

## 4. Upstream API surface to evaluate

At the inspected core revision, the relevant model definitions are in:

- `opensovd-models/src/error.rs`: common error representation and data-error representation.
- `opensovd-models/src/version.rs`: version-info, SOVD info and vendor-info structures.

The source review identified a common error JSON shape with `error_code`, optional `vendor_code`, `message`, and optional translation/parameter fields. The version-info model represents a `sovd_info` collection containing version, base URI and vendor information; the server's version route optionally includes schema information when requested.

The prototype must inspect the exact type definitions and serialization attributes at the pinned revision before writing tests. Do not assume field optionality, defaults or serde behavior from this proposal alone.

## 5. Dependency footprint assessment

The prototype should record the resolved dependency graph using Cargo metadata/tree output, including:

- Direct dependency: the exact `opensovd-models` crate revision/version.
- Transitive dependencies required to compile the models crate.
- Serialization dependencies (including the resolved serde ecosystem versions).
- Any type/feature dependencies pulled in by unrelated model modules because the crate is not split into smaller feature-gated packages.
- Total additional compiled crates, incremental build time and clean build time compared with the current workspace baseline.
- Whether the dependency graph introduces duplicate versions of crates already used by VeloSOVD.
- Whether the upstream crate's minimum supported Rust version and feature flags are compatible with our pinned toolchain.

Do not estimate the footprint from the repository's top-level Cargo workspace alone. Capture actual `cargo tree -e normal`, `cargo metadata`, and clean/incremental build measurements for the prototype.

### Footprint acceptance

- No dependency version is silently overridden to force compilation.
- No broad feature set is enabled without justification.
- No duplicate major versions are accepted without documenting the reason.
- The dependency is pinned to an immutable revision.
- The prototype's added compile time and dependency count are reported, not assumed.

## 6. Technical risks

| Risk | Why it matters | Evaluation / mitigation |
|---|---|---|
| Crate coupling | `opensovd-models` may pull more model and utility dependencies than the two desired types require | Measure the actual dependency tree; consider a narrow upstream extraction only if licensing and maintenance costs are acceptable |
| Serialization mismatch | Field naming, omitted optional fields or enum representation may differ from our current API contract | Golden JSON tests and round-trip tests against upstream types |
| Error semantic mismatch | Upstream error codes and HTTP mapping may not match our existing handler semantics | Keep HTTP status mapping in our adapter; do not blindly replace current error handling |
| Version discovery mismatch | The upstream version model may include schema/vendor fields or assumptions not needed by our deployment | Construct the minimum valid response and test optional schema behavior |
| Rust/dependency compatibility | Upstream revision may use different toolchain or dependency versions | Compile against the project's pinned toolchain without forced overrides |
| Upstream API churn | Depending on a moving branch creates unpredictable builds | Pin the exact commit and record an upgrade procedure |
| License/NOTICE obligations | Reuse requires preserving Apache 2.0 notices and reviewing applicable NOTICE content | Record source URL, commit, license and notices in the third-party attribution register before production reuse |
| Accidental runtime scope creep | A prototype can unintentionally affect existing builds or routes | Keep it isolated; no server dependency or route imports |

## 7. Prototype test cases

1. Construct and serialize a representative generic SOVD error.
2. Construct and serialize an error with optional vendor/translation/parameter fields omitted.
3. Construct and serialize a data error with a JSON Pointer path, if supported by the upstream type.
4. Deserialize each serialized JSON value into the same upstream type.
5. Construct a version-info response with one SOVD API entry and a configured base URI.
6. Serialize version-info with optional schema omitted.
7. If the upstream model/server supports it, serialize version-info with schema included and verify the query-controlled behavior is not accidentally made mandatory.
8. Verify the advertised base URI composes the selected deployment mount path and `/v1`.
9. Verify that no existing VeloSOVD crate or server binary gains a dependency on the prototype.

Use upstream type definitions and source tests as the expected behavior. Do not invent compatibility requirements unsupported by the pinned source.

## 8. Success criteria

The prototype is successful only if all of the following are demonstrated:

- It compiles with the VeloSOVD pinned Rust toolchain without source patches to the upstream crate.
- The two targeted model families can be constructed and serialized/deserialized.
- Golden JSON tests pass for the selected error and version-info examples.
- The effective `base_uri` can be represented without hard-coding `/sovd` into the model.
- The actual dependency graph and build-time impact are measured and documented.
- Existing workspace tests/builds remain unchanged because the prototype is isolated.
- Apache 2.0 attribution and NOTICE requirements are documented before any code is copied or vendored.

A compile failure, incompatible serialization shape, or excessive dependency footprint is a valid prototype result; it should lead to a written reuse decision, not forced compatibility patches.

## 9. Deliverables and decision gate

Deliver:
- Prototype source and tests in the isolated directory.
- Exact upstream commit and dependency tree.
- Before/after build measurements.
- Golden JSON examples.
- Risk and compatibility findings.
- Recommendation: adopt the crate, extract a narrower model boundary, or write project-owned equivalents.

No production integration should proceed until the project owner reviews the results and explicitly approves the next step.

## 10. Reference files

Eclipse OpenSOVD core, commit `27cdad1437cc70bb704779ce025a465c6d32ea6b`:

- `opensovd-models/src/error.rs`
- `opensovd-models/src/version.rs`
- `opensovd-server/src/routes/version.rs`
- `opensovd-server/src/server.rs`

The prototype must inspect the pinned source and package manifests directly before implementation. This proposal is not a statement that the dependency is already compatible with VeloSOVD.
