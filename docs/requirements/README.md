# Requirements Register

This register maintains traceable engineering requirements for the SOVD Rust Stack.

The primary SOVD API reference is ISO 17978-3:2026, Edition 1. The official ISO-hosted OpenAPI package has been obtained and its structure inspected. The package identifies OpenAPI 3.1.0 and ASAM SOVD V1.1.0.

Obtaining and inspecting the OpenAPI package does not mean that the complete normative ISO 17978-3 document has been reviewed or that the implementation conforms to it. Clause-level analysis and API conformance verification remain pending.

## Requirement status definitions

- NOT_IMPLEMENTED: Requirement is understood but not implemented.
- IMPLEMENTED: Code exists, but verification is incomplete.
- VERIFIED: Requirement has passed its defined verification.
- DEVIATION: Implementation intentionally differs; rationale is documented.
- UNVERIFIED: Source, interpretation or implementation status is not yet sufficiently established.

## Required fields

| Field | Description |
|---|---|
| Requirement ID | Stable identifier, e.g. SOVD-API-001 |
| Source ID | ID from `docs/standards/README.md` |
| Edition | Exact edition or version |
| Clause | Clause or section reference, where established |
| Requirement summary | Project-authored summary; do not reproduce copyrighted text |
| Rust module | Responsible crate or module |
| Verification | Test or evidence required |
| Status | One of the defined statuses |
| Evidence | Test result, report or artifact reference |
| Notes | Assumptions, limitations or deviations |

## Current source review status

| Source | Current status |
|---|---|
| ISO 17978-3:2026 | Official catalogue entry checked; complete normative text not reviewed |
| ISO 17978-3 OpenAPI package | Obtained and structurally inspected; detailed API requirement mapping pending |
| ASAM SOVD 1.0.0 | Reference identified; separate comparison and review pending |
| Supporting UDS, DoIP, ISO-TP and CAN standards | Applicable editions and parts require confirmation |

## Requirements development approach

Requirements shall be added progressively after the relevant source material and implementation scope have been reviewed.

For each requirement:

1. Assign a stable project requirement ID.
2. Record the source ID and exact edition or version.
3. Record a clause or section reference when available.
4. Write a concise summary in our own words.
5. Identify the responsible Rust crate or module.
6. Define objective verification criteria.
7. Link the relevant test and retained evidence.
8. Record implementation limitations or deviations.

Do not infer normative requirements from an implementation example or an informative reference package without confirming the applicable source.

## Current state

The project-level functional and non-functional requirements are documented in `project-requirements.md`.

The ISO 17978-3 OpenAPI package has been obtained and inspected, but detailed endpoint, schema, method, response and error-model mapping has not yet been completed.

No clause-level SOVD API requirements are currently claimed as verified. The next activity is to establish the API requirement baseline and map it to the existing Rust implementation and its tests.

No standards-conformance claim is authorized by this register.