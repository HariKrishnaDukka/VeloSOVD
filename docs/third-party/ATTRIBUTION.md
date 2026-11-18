# Third-party attribution register

**Status:** One isolated prototype dependency recorded; no third-party source code has been copied or vendored into production crates.

This register records third-party code, packages, and assets incorporated into VeloSOVD. A repository's declared license is not a substitute for reviewing file-level notices, applicable `NOTICE` files, and dependency licenses.

## Register

| Component | Upstream and immutable revision | Use in VeloSOVD | License / notices | Review status |
|---|---|---|---|---|
| `opensovd-models` 0.1.1 | [eclipse-opensovd/opensovd-core](https://github.com/eclipse-opensovd/opensovd-core), commit `27cdad1437cc70bb704779ce025a465c6d32ea6b` | Git dependency used only by `prototypes/opensovd-models-interop` to evaluate error and version-info serialization. No upstream source files copied; no production crate depends on it. | Upstream repository declares Apache-2.0. Preserve the upstream license and applicable notices if redistributing or copying source. A root-level `NOTICE` file was not found at the inspected revision; nested notice inventory and resolved transitive dependency licenses remain to be checked. | Direct package license identified. CI license and source checks are configured for the pinned dependency graph; the prototype audit reports RustSec RUSTSEC-2026-0543 because chrono 0.4.45 is unmaintained and has no fixed upgrade. Not approved for production adoption. |

## Required entry fields

For each incorporated component, record:

- Component / package name
- Upstream repository and URL
- Exact version, tag or immutable commit SHA
- Files or modules used
- Copyright holder and SPDX license identifier
- Applicable `LICENSE` and `NOTICE` files retained
- Local modifications and date
- Integration location in VeloSOVD
- Dependency-license review status

## Reuse requirements

Before incorporating third-party material:

1. Verify the license at the exact upstream revision.
2. Preserve copyright, license and applicable NOTICE notices.
3. Mark modified files where required.
4. Include the Apache License 2.0 text for Apache-2.0 material.
5. Review transitive dependency licenses separately.
6. Record the exact source revision and modifications.
