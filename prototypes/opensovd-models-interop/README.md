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

Commit the generated `Cargo.lock` when updating this prototype. The lockfile is specific to this standalone crate; the root VeloSOVD workspace lockfile is not used by this nested workspace.

## Interpretation limits

Passing tests establish only that these selected Rust types can be constructed and serialized/deserialized at the pinned source revision. They do not establish ISO 17978-3 conformance, compatibility with VeloSOVD HTTP status mapping, endpoint behavior, runtime adoption suitability, or production security.

Build duration, binary size, resolved dependency count, and full transitive license review must be measured from the generated lockfile on a Rust-enabled runner. No footprint claim is made here.
