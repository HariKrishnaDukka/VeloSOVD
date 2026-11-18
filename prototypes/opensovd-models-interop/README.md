# OpenSOVD models interoperability prototype

**Status:** Isolated evaluation only. This crate is not a production dependency and is not a member of the VeloSOVD Cargo workspace.

## Upstream pin

- Repository: https://github.com/eclipse-opensovd/opensovd-core
- Exact Git revision: `27cdad1437cc70bb704779ce025a465c6d32ea6b`
- Upstream workspace package version at that revision: `0.1.1`
- Crate: `opensovd-models`
- License declared by upstream: Apache-2.0
- Relevant definitions: `opensovd-models/src/error.rs`, `version.rs`, and `types.rs`

The Git dependency uses the immutable revision above. Do not replace it with a branch or tag reference.

## Scope

This prototype exercises only the upstream error and version-info model serialization contracts. It does not register routes, link the crate into `sovd-server`, change existing API endpoints, or copy upstream implementation source into VeloSOVD.

The tested shapes cover generic errors, optional error fields, data errors with JSON Pointer paths, the untagged error-details enum, version-info with omitted schema, and version-info with schema.

## Run locally

From this directory:

```sh
cargo generate-lockfile
cargo fmt -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo run
cargo tree -e normal
cargo audit
cargo deny check
```

The generated `Cargo.lock` is committed beside this manifest. Regenerate it only when intentionally updating the dependency graph; review the resulting diff and keep the upstream Git revision immutable.

## Interpretation limits

Passing tests establish only that these selected Rust types can be constructed and serialized/deserialized at the pinned source revision. They do not establish ISO 17978-3 conformance, compatibility with VeloSOVD HTTP status mapping, endpoint behavior, runtime adoption suitability, or production security.

## GitHub CI baseline (2026-11-18)

Measured on the GitHub-hosted `ubuntu-latest` x86_64 runner with Rust/Cargo 1.100.0:

- Prototype lockfile: 17 package entries (15 registry packages, the pinned upstream Git package, and this local prototype package).
- Additional registry packages compared with the VeloSOVD root lockfile: 3 — `autocfg 1.5.1`, `chrono 0.4.45`, and `num-traits 0.2.19`. The remaining registry package versions are already present in the root lockfile.
- Clean release build: 5 seconds as reported by the CI shell timer.
- Release executable: 509,584 bytes on the CI runner. This is a host-build measurement, not an embedded target footprint.
- Serialization tests: 6 passed, 0 failed.
- `cargo fmt` and Clippy: passed.
- `cargo audit`: reports RustSec advisory `RUSTSEC-2026-0543` for `chrono 0.4.45` as unmaintained; no fixed upgrade is available at the time of this run.
- License/source checks are run separately from the advisory check so the unmaintained warning remains visible without being silently ignored. The prototype is not approved for production adoption.

The dependency graph and measurements are runner-specific. Re-run them when the toolchain or lockfile changes.
