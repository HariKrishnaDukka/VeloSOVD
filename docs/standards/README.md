# Standards Source Register

**Project:** SOVD Rust Stack  
**Baseline review date:** 2026-11-16

This register identifies the standards and technical references used to guide implementation. Listing a document does not mean its complete normative contents have been obtained or reviewed.

## Primary SOVD standards

| ID | Document | Edition / version | Official status | Project use | Access / review status |
|---|---|---|---|---|---|
| STD-SOVD-001 | ISO 17978-1 | 2026, Edition 1 | Published | General information, definitions, rules and basic principles | Catalogue verified; full document not reviewed |
| STD-SOVD-002 | ISO 17978-2 | 2026, Edition 1 | Published | SOVD use cases | Catalogue verified; full document not reviewed |
| STD-SOVD-003 | ISO 17978-3 | 2026, Edition 1 | Published | SOVD API contract | Catalogue verified; official OpenAPI package obtained and structurally inspected; full normative document not reviewed |
| STD-SOVD-004 | ASAM SOVD | 1.0.0, 2022-06-30 | Published | Comparison reference and historical API implementation reference | Official ASAM reference identified; separate specification review pending |
| STD-SOVD-005 | ASAM SOVD 1.2.0 project | Development project | Ongoing; planned release March 2028 | Monitor future changes | Development project identified; not a released specification |

## ISO official catalogue links

- ISO 17978-1:2026:  
  https://www.iso.org/standard/85133.html
- ISO 17978-2:2026:  
  https://www.iso.org/standard/86586.html
- ISO 17978-3:2026:  
  https://www.iso.org/standard/86587.html

### ISO 17978-3 OpenAPI package

The official ISO Standards Maintenance Portal provides a machine-readable OpenAPI definition associated with ISO 17978-3:

https://standards.iso.org/iso/17978/-3/ed-1/en

The obtained package was inspected and has the following recorded characteristics:

- OpenAPI version: 3.1.0
- API specification version: ASAM SOVD V1.1.0
- Main definition: `sovd-api.yaml`
- Separate authentication definition: `authentication-api.yaml`
- Additional domain definitions include discovery, data, faults, operations, configurations, modes, logs and software updates.

The package is a machine-readable API reference. Its inspection does not establish that the complete normative ISO standard has been reviewed or that the Rust implementation conforms to it.

Recorded package SHA-256:

`4a3fb8bf99acbeb1d751a0dd349612164d5d957d44d7bb5cd7919d035e454212`

## ASAM official reference

- ASAM SOVD:  
  https://www.asam.net/standards/detail/sovd/

ASAM SOVD 1.0.0 is recorded as the published reference version. The 1.2.0 development project is separate from the published 1.0.0 specification and must not be treated as a released version.

## Supporting protocol standards — acquisition and edition review pending

| ID | Standard family | Intended use | Status |
|---|---|---|---|
| STD-UDS-001 | ISO 14229 | UDS diagnostic services and behavior | Applicable parts and editions to be confirmed |
| STD-DOIP-001 | ISO 13400 | Diagnostics over Internet Protocol | Applicable parts and editions to be confirmed |
| STD-ISOTP-001 | ISO 15765-2 | Diagnostic communication over CAN | Edition to be confirmed |
| STD-CAN-001 | ISO 11898 | CAN communication | Applicable parts to be confirmed |

Do not use unverified edition numbers as implementation acceptance criteria.

## Software and security references — review pending

- OpenAPI Specification
- HTTP semantics (RFC 9110)
- JSON (RFC 8259)
- OAuth 2.0 (RFC 6749)
- OAuth 2.0 Bearer Token Usage (RFC 6750)
- OAuth 2.0 Token Exchange (RFC 8693)
- OpenID Connect Core
- TLS 1.2 / TLS 1.3 specifications

The applicable versions and profiles must be selected during API and security architecture review.

## Evidence rules

1. Record the exact edition and publication status.
2. Record the official source URL and date checked.
3. Distinguish catalogue verification, package acquisition, structural inspection and full-document review.
4. Do not copy copyrighted ISO text into this repository.
5. Record requirements in project-authored wording and retain clause references where established.
6. Do not claim conformance based only on a document being listed or an OpenAPI package being inspected.
7. Record package hashes for retained machine-readable source artifacts.
8. Update this register when a newer edition or amendment is adopted.

## Current review status

| Activity | Status |
|---|---|
| ISO 17978 catalogue review | Completed for Parts 1, 2 and 3 |
| ISO 17978-3 OpenAPI package acquisition | Completed |
| OpenAPI package structural inspection | Completed |
| Clause-level SOVD requirement mapping | Pending |
| Full normative ISO 17978 review | Pending |
| Supporting UDS / DoIP / ISO-TP / CAN edition review | Pending |
| Rust API conformance assessment | Pending |

This register is an engineering source record, not a declaration of standards compliance.