# Rust SOVD Implementation: Deviations and Known Limitations

## 1. Purpose

This document records known implementation limitations, incomplete capabilities, verification gaps and areas requiring standards review.

It is an engineering tracking document. It does not reproduce ISO 17978 or the ASAM SOVD specification.

The project is a virtual Software-Defined Vehicle (SDV) demonstrator and reference implementation under development. No claim of certification or full standards conformance is made.

The primary SOVD API baseline is ISO 17978-3:2026 Edition 1. The official ISO-hosted OpenAPI package has been obtained and structurally inspected. Full normative review and clause-level API conformance assessment remain pending.

ASAM SOVD OpenAPI material is retained as a technical comparison reference. It must not be assumed to be normatively equivalent to the ISO specification without verification.

## 2. Current implementation limitations
### 2.1 Fault retrieval implementation record — GAP-04

**Project requirement:** FR-05

**Capability:** Retrieve an individual diagnostic fault using its ECU identifier
and fault code.

**Implementation status:** IMPLEMENTED — demonstrator-level functional tests passed; retained verification evidence is pending.

**Implementation locations:**
- `crates/sovd-server/src/routes/faults.rs` — central `fault_by_code` handler; route in `routes/mod.rs`.
- `crates/virtual-ecu/src/main.rs` — zonal `read_fault` handler and route.
- `crates/diagnostic-common/src/lib.rs` — shared `DiagnosticFault` model.

**Verification evidence:**
- Central integration test:
  `retrieves_individual_fault_from_zonal_server`.
- Virtual ECU test:
  `http_reads_individual_fault_from_shared_dtc_store`.
- Workspace test suite: 105 tests passed.
- `cargo fmt --all -- --check` passed.
- `cargo clippy --workspace --all-targets -- -D warnings` passed.

**Verified behaviour:**
- An existing fault can be retrieved using its ECU identifier and fault code.
- An unknown fault code returns HTTP 404.
- An unknown ECU returns HTTP 404.
- The virtual ECU HTTP fault retrieval reads the shared DTC store.

**Limitations:**
- The central integration test uses a mock zonal HTTP service.
- The virtual ECU test directly invokes the handler; it does not exercise the
  complete central-to-zonal HTTP path using both real routers.
- The returned `DiagnosticFault` representation has not been demonstrated to
  match the normative ISO 17978-3 fault schema.
- Applicable normative clauses, response schemas, parameter requirements and
  error semantics have not yet been fully assessed.

**Standards assessment:** Not established. This implementation record is not
a declaration of ISO 17978-3 conformance.


### 2.2 Fault deletion implementation record — GAP-05

**Project requirement:** FR-05

**Capability:** Delete an individual diagnostic fault or the complete fault collection for an ECU.

**Implementation status:** IMPLEMENTED — demonstrator-level functional tests passed; workspace tests, formatting, and Clippy checks passed on 2026-11-16. Normative ISO 17978-3 conformance remains unverified.

**Implementation locations:**

- `crates/sovd-server/src/routes/faults.rs` — central DELETE handlers; route registration in `routes/mod.rs`.
- `crates/virtual-ecu/src/main.rs` — zonal DELETE handlers and route registration.
- `crates/diagnostic-common/src/lib.rs` — shared diagnostic fault model and DTC-related types.

**Implemented API routes:**

- `DELETE /api/ecus/{id}/diagnostics/faults` — deletes all faults for an ECU.
- `DELETE /api/ecus/{id}/diagnostics/faults/{fault_code}` — deletes one specified fault.
- Existing POST clear-fault compatibility endpoint is retained.

**Verification evidence:**

- Central integration test: `retrieves_individual_fault_from_zonal_server`.
- Virtual ECU test: `http_delete_individual_fault_is_visible_through_uds`.
- Virtual ECU test: `http_delete_fault_collection_clears_shared_dtc_store`.
- Virtual ECU test: `http_delete_fault_returns_not_found_for_unknown_resources`.
- Virtual ECU test: `http_delete_individual_fault_preserves_other_faults`.
- Full workspace test suite: 105 tests passed, 0 failed.
- `cargo fmt --all -- --check` passed.
- `cargo clippy --workspace --all-targets -- -D warnings` passed.

**Verified behaviour:**

- Individual fault deletion removes only the requested fault.
- Other faults remain available after individual deletion.
- Collection deletion clears all faults for the specified ECU.
- Successful deletion returns HTTP 204 No Content.
- Unknown ECU or fault resources return HTTP 404 Not Found.
- HTTP deletion modifies the shared DTC store, and the deletion is visible through UDS diagnostic reads.
- The existing POST clear-fault compatibility endpoint remains available.

**Limitations:**

- Central integration testing uses a mock zonal HTTP service.
- Virtual ECU tests exercise the zonal handlers and shared DTC store.
- A complete end-to-end test using both real central and zonal HTTP routers has not yet been established.
- The DELETE route semantics, response schemas, parameter requirements and error behaviour have not yet been fully assessed against the normative ISO 17978-3 requirements.

**Standards assessment:** Not established. This implementation record is not a declaration of ISO 17978-3 conformance.

### 2.3 Fault status and filtering implementation record — GAP-06

**Project requirement:** FR-05

**Capability:** Preserve the raw UDS DTC status byte in the shared diagnostic fault model and support filtering the ECU fault collection by active state.

**Implementation status:** IMPLEMENTED — demonstrator-level functional tests passed; workspace tests, formatting and Clippy checks passed on 2026-11-17. Normative ISO 17978-3 conformance remains unverified.

**Implementation locations:**

- `crates/diagnostic-common/src/lib.rs` — `DiagnosticFault` includes the raw `status: u8` field.
- `crates/virtual-ecu/src/main.rs` — `read_faults` and `read_fault` preserve the UDS DTC status byte when constructing `DiagnosticFault`.
- `crates/sovd-server/src/routes/faults.rs` — central fault-list handler applies the `active` query filter.

**Implemented behaviour:**

- The raw UDS DTC status byte is preserved in `DiagnosticFault`.
- The existing `active` property is derived from bit 0 of the UDS DTC status byte.
- `GET /api/ecus/{id}/diagnostics/faults` returns the fault collection.
- `?active=true` returns only active faults.
- `?active=false` returns only inactive faults.
- Omitting the `active` parameter returns the complete fault collection.

**Verification evidence:**

- Central integration test: `retrieves_individual_fault_from_zonal_server`.
- The central integration test covers an unfiltered fault list, `active=true` and `active=false`.
- Full workspace test suite: 105 tests passed, 0 failed.
- `cargo fmt --all` completed successfully.
- `cargo clippy --workspace --all-targets -- -D warnings` passed.

**Limitations:**

- The central fault-filter integration test uses a mock zonal HTTP service.
- The raw UDS status byte is not equivalent to the SOVD `status[key]` representation.
- SOVD fault `severity` and `scope` mapping have not been implemented.
- The `active` query parameter is a demonstrator-specific filter; its implementation does not establish normative SOVD query-parameter conformance.
- A complete end-to-end test using both real central and zonal HTTP routers has not yet been established.

**Standards assessment:** Not established. The implementation preserves UDS diagnostic status information and provides demonstrator-level filtering. It is not a declaration of ISO 17978-3 conformance.

### 2.4 Individual operation description implementation record — GAP-07

**Project requirement:** FR-06

**Capability:** Retrieve the description of an individual operation supported by an ECU using its ECU identifier and operation identifier.

**Implementation status:** IMPLEMENTED — demonstrator-level functional test passed; workspace tests, formatting and Clippy checks passed on 2026-11-17. Normative ISO 17978-3 conformance remains unverified.

**Implementation locations:**

- `crates/sovd-server/src/routes/vehicle.rs` — `ecu_operation` handler retrieves an individual operation description.
- `crates/sovd-server/src/routes/mod.rs` — central router registers the individual operation description route.

**Implemented behaviour:**

- `GET /api/ecus/{id}/operation-descriptions/{operation_id}` retrieves an individual operation description.
- The currently supported operation is `clear-faults`.
- The response describes the operation name, HTTP method, execution path and execution support.
- An unknown operation returns HTTP 404.
- An unknown ECU returns HTTP 404.

**Verification evidence:**

- Central integration test: `retrieves_individual_operation_description`.
- The test verifies successful retrieval of `clear-faults`.
- The test verifies HTTP 404 for an unknown operation and an unknown ECU.
- Workspace tests, formatting and Clippy checks passed on 2026-11-17.

**Limitations:**

- The operation catalogue is currently hardcoded in the central server.
- The custom `OperationDescriptor` response is not demonstrated to match the normative SOVD `OperationDescription` schema.
- The custom `operation-descriptions` route has not been demonstrated to match the normative ISO 17978-3 operation-discovery path.
- Proximity-proof requirements, asynchronous-execution metadata, translation identifiers and other applicable normative fields have not been fully assessed.
- The test verifies the central handler using the configured topology; it does not establish full central-to-zonal operation-discovery behaviour.

**Standards assessment:** Not established. This implementation provides demonstrator-level individual operation retrieval. It is not a declaration of ISO 17978-3 conformance.


### 2.5 SOVD operation execution implementation record — GAP-09

**Project requirement:** FR-06

**Capability:** Start an operation execution, retrieve the execution collection,
and retrieve an individual execution status through separate SOVD-facing routes
while retaining the existing custom frontend API.

**Implementation status:** IMPLEMENTED — demonstrator-level asynchronous
execution. Workspace tests, formatting and Clippy checks passed on 2026-11-17.
Normative ISO 17978-3 conformance remains unverified.

**Implementation locations:**

- `crates/sovd-server/src/models.rs` — SOVD execution request/response models.
- `crates/sovd-server/src/routes/executions.rs` — execution handlers;
  route registration in `routes/mod.rs`.
- `crates/sovd-server/src/execution/` — generic execution manager, lifecycle
  model, executor interface and scheduling.
- Existing custom `/api/...` execution routes remain available separately.

**Implemented SOVD-facing routes:**

- `POST /sovd/v1/components/{id}/operations/{operation_id}/executions`
- `GET /sovd/v1/components/{id}/operations/{operation_id}/executions`
- `GET /sovd/v1/components/{id}/operations/{operation_id}/executions/{execution_id}`
- `DELETE /sovd/v1/components/{id}/operations/{operation_id}/executions/{execution_id}`
- `PUT /sovd/v1/components/{id}/operations/{operation_id}/executions/{execution_id}`

The `/sovd/v1` prefix is a local implementation choice. Mapping virtual ECUs
to the SOVD `components` entity collection is provisional and has not been
established as a normative mapping.

**Implemented behaviour:**

- The currently registered operation is `clear-faults`.
- POST starts an asynchronous execution and returns HTTP 202, an execution
  identifier, status `running`, and a Location header.
- GET on the execution collection returns an `items` array of execution IDs.
- GET on an individual execution retrieves its status from the generic manager.
- The SOVD-facing status is mapped separately from internal execution states.
- The existing custom `/api/...` execution routes remain available separately.
- Execution history is bounded to 128 in-memory records.

**Verification evidence:**

- Integration test: `sovd_post_executes_operation_and_records_execution`.
- The test exercises the SOVD execution flow using a mock zonal HTTP service,
  then retrieves the execution collection and polls the individual status.
- Integration test: `operation_execution_routes_filter_by_operation_and_ecu`.
- Full workspace test suite: 136 tests passed, 0 failed.
- `cargo fmt --all -- --check` passed.
- `cargo clippy --workspace --all-targets -- -D warnings` passed.
- Verification completed on 2026-11-17.

**Limitations and outstanding work:**

- Only `clear-faults` is currently registered.
- Operation parameters are rejected with HTTP 400.
- Proximity-proof requests are rejected with HTTP 501.
- The registered executor advertises `execute` and `status`, but not `stop`,
  `freeze` or `reset`. Stop requests are therefore rejected as unsupported.
- The PUT `status` capability handler currently returns HTTP 204.
- Execution history is in-memory, bounded to 128 records and lost on restart.
- The integration test uses a mock zonal HTTP service; it does not exercise
  both real central and zonal HTTP routers together.
- Error response structures and status-code mappings require further validation.
- The route prefix, entity mapping, schemas, status codes, headers and error
  semantics have not been fully assessed against normative ISO 17978-3
  requirements.

**Standards assessment:** Not established. This implementation provides
demonstrator-level asynchronous execution and retrieval. It is not a
declaration of ISO 17978-3 conformance.


| Area | Current implementation / limitation | Required follow-up |
|---|---|---|
| SOVD API | The Rust server exposes custom `/api/...` routes and separate demonstrator SOVD-facing operation execution routes. GAP-04 through GAP-09 have implementation records; full ISO 17978-3 API conformance has not been established. | Map applicable ISO 17978-3 requirements to routes, methods, schemas, status codes, error responses and tests. |
| OpenAPI reference | The official ISO 17978-3 OpenAPI package has been structurally inspected. Full normative interpretation and clause-level mapping are incomplete. | Review the normative specification and validate implementation behaviour against applicable API requirements. |
| ASAM comparison | ASAM SOVD OpenAPI material is available as a technical reference. Normative equivalence with the selected ISO edition has not been independently established. | Record differences and use ASAM material only with clearly identified reference status. |
| Authentication | Production OAuth 2.0 / OpenID Connect enforcement has not been established. | Define and implement the applicable authentication model and test its enforcement. |
| Transport security | The development endpoint uses HTTP. Production TLS deployment and certificate lifecycle management are not established. | Configure TLS for deployable environments and define certificate provisioning, renewal and revocation. |
| Authorization | Production-ready diagnostic access-control policy has not been demonstrated. | Define roles, scopes, ECU permissions, operation permissions and enforcement tests. |
| Vehicle modes and sessions | Required vehicle-mode and diagnostic-session enforcement for all operations has not been established. | Define vehicle states and enforce operation preconditions. |
| Operation history | Execution records are held in process memory with a capacity of 128 entries. They are lost on restart. | Decide persistence, retention, ownership and audit requirements. |
| Operation lifecycle | The demonstrator SOVD-facing API starts asynchronous executions and returns HTTP 202 with an execution ID and Location. The generic manager implements bounded scheduling, lifecycle transitions, timeout handling, executor supervision and cooperative shutdown. The registered `clear-faults` executor advertises only execute and status; stop, freeze and reset are not supported by that executor. History remains in-memory. | Validate the HTTP contract and lifecycle against normative ISO 17978-3 requirements; implement durable history, recovery and operation-specific remote cancellation only where the ECU can confirm the outcome. |
| Startup error handling | Startup error handling requires a current source-code audit. Remaining `panic!`, `expect()` and `unwrap()` calls must be assessed individually. | Audit startup and runtime paths; replace inappropriate panic-based handling with explicit error propagation and controlled termination. |
| Error model | Handler-specific HTTP status behaviour exists. A complete standardized SOVD error model has not been demonstrated. | Validate error schemas, response bodies and status-code mapping against the selected API requirements. |
| UDS | Virtual ECU diagnostic request processing is implemented, including selected services. Full ISO 14229 conformance has not been established. | Validate service behaviour, session rules, security access, timing and negative responses against the applicable edition and profile. |
| DoIP | A DoIP listener, routing activation handling and diagnostic-message dispatch path are implemented. Full ISO 13400 conformance and interoperability have not been established. | Validate protocol framing, routing activation, connection lifecycle, discovery requirements and diagnostic-message behaviour. |
| HTTP diagnostic forwarding | Central-to-zonal HTTP forwarding has integration-test coverage using a mock zonal HTTP service. This does not prove UDS-over-DoIP or ISO-TP transport. | Test the complete production routing path and retain separate evidence for each transport boundary. |
| ISO-TP | Virtual CAN and ISO-TP-related components exist in the demonstrator. Full ISO 15765-2 conformance and end-to-end transport validation have not been established. | Validate segmentation, flow control, addressing, timing and error handling against the applicable edition. |
| Physical ECUs | Current vehicle topology uses virtual ECU services. | Validate with representative hardware, real ECU interfaces and physical vehicle networks. |
| Safety | No ISO 26262 safety case has been established. | Determine intended use, safety responsibilities and applicability before vehicle integration. |
| Cybersecurity | No complete ISO/SAE 21434 cybersecurity case has been established. | Perform threat analysis, risk assessment and verification for the intended deployment. |
| CRA | Cyber Resilience Act applicability and maintainer obligations have not been legally assessed. | Obtain qualified legal review for the intended product, distribution and support model. |

## 3. Runtime and implementation constraints

### 3.1 In-memory execution history

The central server stores operation execution records in a bounded in-memory collection. The configured capacity is 128 entries.

Consequences:

- Records are lost when the process restarts.
- Older records are discarded when the capacity is exceeded.
- This is not a durable audit log.
- It does not provide crash recovery.
- It does not provide persistent operation ownership or lifecycle recovery.

The implementation must not be presented as a durable operation-history or audit solution.

### 3.2 Diagnostic dispatch

The architecture uses bounded diagnostic dispatch queues and isolates blocking diagnostic work from asynchronous request handling.

The UDS dispatcher uses a dedicated operating-system thread and a bounded request queue. DoIP diagnostic handling dispatches work through a blocking execution boundary.

The following behaviours require additional stress and shutdown testing:

- Queue saturation and overload response.
- Concurrent diagnostic requests.
- Request timeout and cancellation.
- Worker failure and recovery.
- Shutdown while a diagnostic request is active.
- Connection closure during diagnostic processing.
- Resource limits under repeated connection attempts.

A successful unit test does not establish correct behaviour under all concurrent runtime conditions.

### 3.3 Demonstration data

Virtual ECU responses and configured vehicle data are demonstration data.

A successful response from a virtual ECU does not establish interoperability with a production ECU.

Diagnostic identifiers, software versions, vehicle identifiers and fault data must be treated as simulated values unless verified against a real ECU.

### 3.4 Network and transport boundaries

The demonstrator has separate central and zonal services, Linux network namespaces and virtual CAN interfaces.

The following evidence must remain distinct:

- Network namespace connectivity.
- Central-to-zonal HTTP communication.
- UDS request processing by a virtual ECU.
- DoIP routing activation and diagnostic-message handling.
- ISO-TP segmentation and reassembly.
- End-to-end diagnostic communication over the intended transport.

Success at one boundary must not be used as proof of success at another boundary.

## 4. Code quality work still required

The project roadmap requires:

- No unjustified `unsafe` Rust.
- No inappropriate panic-based error handling in core request paths.
- Fallible startup and configuration handling.
- Strict Clippy and rustfmt checks.
- `cargo-audit` and `cargo-deny`.
- Unit, integration, API conformance and parser fuzz tests.
- Continuous integration that runs required checks on every change.
- Review of concurrency, shutdown and resource-bound behaviour.

Startup code must be audited before recording specific panic-related defects. Any remaining `panic!`, `expect()` or unjustified `unwrap()` should be assessed in context and replaced with explicit error propagation where appropriate.

## 5. Security work still required

Before production-oriented deployment:

- TLS must be enabled and correctly configured.
- Authentication and authorization must be enforced.
- Diagnostic requests must be access-controlled.
- ECU and operation permissions must be enforced.
- Input validation and resource limits must be reviewed.
- Rate limiting and abuse protection must be added.
- Security-relevant actions must be auditable.
- Credential and key-management procedures must be defined.
- `SECURITY.md` and a vulnerability-reporting process must be provided.
- Demonstration credentials and demonstration seed/key logic must never be presented as production security.

Security controls must be assessed across central services, zonal services, gateways and ECU diagnostic interfaces.

### 5.1 Progress (2026-11-26)

| Section 5 item | Status | Requirement |
|---|---|---|
| TLS enabled and correctly configured | Implemented earlier (TLS 1.3, mTLS both hops) | — |
| Authentication and authorization enforced | Implemented (certificate roles) | PRJ-SEC-02, PRJ-SEC-03 |
| Diagnostic requests access-controlled | Implemented | PRJ-SEC-01, PRJ-SEC-03 |
| ECU and operation permissions | Partial: per role, not per ECU | DEV-SEC-03 |
| Input validation and resource limits reviewed | Implemented | PRJ-SEC-04..06 |
| Rate limiting and abuse protection | Partial: global concurrency only | DEV-SEC-04 |
| Security-relevant actions auditable | Implemented | PRJ-SEC-07..10 |
| Credential and key-management procedures | Open | — |

## 6. Conformance and traceability policy

Each normative requirement should have:

1. A stable project requirement identifier.
2. A source reference to the applicable standard, edition and clause.
3. An implementation location or an explicit not-implemented status.
4. One or more verification tests.
5. A result and evidence reference.
6. A recorded deviation when the requirement is not implemented or verified.

Use the following project status definitions consistently:

- `NOT_IMPLEMENTED`: no implementation is available.
- `IMPLEMENTED`: implementation exists, but verification evidence is incomplete.
- `VERIFIED`: the defined verification activity passed and evidence is retained.
- `DEVIATION`: a documented difference from an applicable requirement exists.
- `UNVERIFIED`: applicability or implementation status has not yet been established.

Do not copy ISO standard text into this repository. Record requirement identifiers and project-authored summaries instead.

A feature that is outside the current demonstrator scope must not automatically be described as a standards deviation. First establish whether the requirement applies to the selected project scope.

## 7. Release maturity

Until API conformance, security, transport interoperability and real-ECU validation are demonstrated, describe the project as a virtual demonstrator, reference implementation or alpha project.

Do not claim that the project is certified or fully compliant with ISO 17978, ASAM SOVD, ISO 14229, ISO 13400, ISO 15765 or automotive safety and cybersecurity standards without supporting evidence.

## 8. Review status

This document is a working engineering register. Each entry must be reviewed against the current source code, project requirements, test results and selected normative standards.

Changes to implementation status must include evidence references wherever verification is claimed.


## 9. GAP-10 - Generic operation execution lifecycle

**Status:** IMPLEMENTED - demonstrator-level execution framework.

**Verification:** Unit and workspace tests passed on 2026-11-17.

### 9.1 Architecture

The central SOVD server now contains a generic execution framework under:

`crates/sovd-server/src/execution/`

The framework is divided into:

- `model.rs`: execution states, operation definitions, capability declarations and execution records.
- `executor.rs`: asynchronous executor interface, execution context, timeout information and cooperative stop signalling.
- `manager.rs`: operation and executor registration, execution scheduling, bounded history, concurrency control, state transitions and shutdown handling.
- `mod.rs`: execution module declarations.

The manager separates operation lifecycle management from operation-specific execution logic. Executors are registered against operation definitions, and their declared capabilities must match the operation definition.

### 9.2 Lifecycle and state handling

The internal execution lifecycle includes:

- `Created`
- `Running`
- `StopRequested`
- `Frozen`
- `Completed`
- `Failed`
- `Stopped`

Transitions are checked by the execution model. Invalid transitions and transitions out of terminal states are rejected.

A stop request is not equivalent to a confirmed stop. The manager records `StopRequested` before signalling the executor. The execution becomes `Stopped` only when the executor acknowledges the stop by returning the corresponding execution result.

The generic model includes freeze and resume transitions, but these transitions must not be interpreted as evidence that a registered ECU operation supports freeze or resume.

The SOVD-facing execution status is mapped separately from the internal lifecycle. Internal states are not automatically additional values in the SOVD `ExecutionStatus` enumeration.

### 9.3 Scheduling and resource bounds

The execution manager provides:

- Bounded execution-record capacity.
- Eviction of terminal records when history capacity is reached.
- Protection against eviction of active executions.
- A configurable concurrent-execution limit.
- Rejection when the concurrency limit is exhausted.
- Rejection of zero-duration timeouts.
- Asynchronous executor task supervision.
- Conversion of executor failures and task panics into failed execution records.
- Cleanup of completed task handles and stop-signal registrations.

The current central server configuration uses a history capacity of 128 records and a maximum of 8 concurrent executions.

Execution history remains in process memory. Records are lost on restart, and this mechanism is not a durable audit log or crash-recovery facility.

### 9.4 Timeout and cancellation semantics

A configured timeout bounds the local executor future. When the timeout expires, the local executor task is aborted and the execution is recorded as failed.

Aborting a local future does not prove that a remote ECU operation has stopped. A remote HTTP request, gateway action or ECU operation may have progressed independently of the local task.

The framework therefore distinguishes a requested stop from an acknowledged stop. Executors must implement cooperative cancellation and provide a reliable acknowledgement before the manager records `Stopped`.

The current `clear-faults` executor does not advertise stop capability. Its remote diagnostic action is not considered safely cancellable merely because the central task or HTTP request can be interrupted.

### 9.5 Shutdown behaviour

The manager provides a bounded graceful-shutdown method:

- New execution requests are rejected after shutdown begins.
- Stop signals are sent only to operations advertising stop capability.
- The manager waits for execution tasks up to the supplied grace period.
- Execution IDs still active after the grace period are returned to the caller.
- Unresolved task handles are retained rather than silently discarded.
- Expiry of the grace period does not cause an execution to be reported as stopped.

The shutdown method is implemented and unit-tested. Integration with the server process termination lifecycle and operating-system shutdown signals has not been established by this work.

### 9.6 Current HTTP API capability mapping

The current registered `clear-faults` operation declares:

| Capability | Advertised |
|---|---|
| execute | Yes |
| status | Yes |
| stop | No |
| freeze | No |
| reset | No |

The SOVD-facing POST execution handler starts the asynchronous operation and returns HTTP 202 with an execution identifier and a Location header.

The execution collection and individual execution status handlers retrieve records from the generic execution manager.

The DELETE execution handler and PUT `stop` capability handler delegate to the manager. Since `clear-faults` does not advertise stop, the manager rejects the request as an unsupported capability. The HTTP error mapping returns HTTP 501.

The PUT `status` capability currently returns HTTP 204. Execute, freeze and reset through the capability-control handler are not implemented and return HTTP 501.

The current implementation rejects operation parameters with HTTP 400 and proximity-proof requests with HTTP 501. These restrictions must remain explicit in the API documentation.

### 9.7 Verification evidence

The following verification completed on 2026-11-17:

- Execution model tests cover normal lifecycle transitions, stop transitions, freeze/resume model transitions, capability exposure and invalid transitions.
- Manager tests cover operation registration, executor registration, capability mismatch, duplicate registration, bounded history, active-record retention and ECU/operation filtering.
- Scheduler tests cover successful execution, executor failure, timeout, concurrency exhaustion, stop acknowledgement and zero-timeout rejection.
- Panic supervision test verifies that an executor panic becomes a failed execution and that task and stop-signal registrations are cleaned up.
- Shutdown tests verify rejection of new work, cooperative stop acknowledgement and reporting of a non-cooperative execution after grace-period expiry.
- `cargo fmt --all -- --check` passed.
- `cargo test --workspace` passed: 136 tests, 0 failures.
- `cargo clippy --workspace --all-targets -- -D warnings` passed.

### 9.8 Limitations and standards assessment

The implementation is a generic demonstrator execution framework, not a production operation orchestration service.

Outstanding work includes:

- Durable execution history and crash recovery.
- Persistent execution ownership and audit records.
- Server-process graceful-shutdown integration.
- Operation-specific cancellation with remote ECU acknowledgement.
- Real central-to-zonal execution lifecycle integration tests.
- Validation of all HTTP methods, request and response schemas, status codes, headers and error mappings against the normative ISO 17978-3 requirements.
- Verification of execution-status mapping and capability semantics against the selected normative API definition.
- Security, authorization and operation precondition enforcement.

**Standards assessment:** Not established. The implementation and tests demonstrate project-level execution behaviour only. They do not establish ISO 17978-3 conformance or certification.

## 2.6 UDS ClearDiagnosticInformation simulator behaviour

**Capability:** Clear all diagnostic trouble codes through UDS service
`0x14`, using the all-DTC group value `0xFFFFFF`.

**Implementation status:** Implemented as simulator behaviour. This is not
evidence of complete ISO 14229 conformance or physical ECU interoperability.

**Defined simulator responses:**

- A valid all-DTC clear request with payload `14 FF FF FF` clears the
  simulated DTC store and returns the one-byte positive response `54`.
  This also succeeds when the DTC store is already empty.
- A request with an invalid length returns negative response
  `7F 14 13` (incorrect message length or invalid format).
- A request with an unsupported DTC group returns negative response
  `7F 14 31` (request out of range).

The NRC values `0x13` and `0x31` are explicitly recorded as current
simulator behaviour. Their implementation does not establish that every
physical ECU or every applicable diagnostic profile must respond identically.

**Dispatch architecture:**

- The zonal clear-fault POST and fault-collection DELETE submit
  `14 FF FF FF` through the per-ECU UDS dispatcher.
- The zonal POST preserves the raw UDS response bytes in `response_hex`.
- The central clear-fault operation accepts success only for exactly one
  response byte, `54`.
- A negative, empty, suppressed, truncated or unexpected response is a
  failure and must not be recorded as a completed operation.

**Verification required:**

- Clear a seeded DTC and verify the shared HTTP/UDS DTC store is empty.
- Clear an already empty DTC store and verify `54`.
- Verify unsupported-group NRC `0x31` and invalid-length NRC `0x13`.
- Verify negative, missing and malformed responses are mapped to failed
  operation executions.
- Verify deterministic dispatcher timeout and queue-full handling at the
  configured per-ECU capacity of 16.

**Standards assessment:** Not established. The behaviour above is a
project-defined simulator subset and must not be represented as full
ISO 14229 or ISO 17978-3 conformance.


## 10. Security hardening R1–R10: known limitations

Requirements and evidence: `requirements.csv` rows `PRJ-SEC-*` and `PRJ-SW-*`,
`docs/evidence/2026-11-26-security-hardening-r1-r10.md`.

| ID | Requirement | Limitation | Risk | Planned resolution |
|---|---|---|---|---|
| DEV-SEC-01 | PRJ-SEC-01 | Raw UDS allow-list is compiled in and identical for every ECU and role | Cannot tighten per ECU without a rebuild | Move allow-list to configuration, per ECU and role |
| DEV-SEC-02 | PRJ-SEC-02 | Identity is the certificate DNS SAN; no OAuth2/OIDC bearer tokens as modelled in ISO 17978-3 | Workshop and cloud clients need certificates | Decide token model (open decision, project-requirements §6) |
| DEV-SEC-03 | PRJ-SEC-03 | Required role is derived from method and path string patterns; no per-ECU or per-operation permissions | A new route that does not match a pattern falls to the highest role (fail-safe, but coarse) | Explicit permission per route in the router |
| DEV-SEC-04 | PRJ-SEC-04 | Concurrency limit is global; no per-client quota | One client can consume all permits | Per-principal token bucket |
| DEV-SEC-05 | PRJ-SEC-05 | Deadline cancels the central handler only | Remote ECU operation may continue | Map to execution stop where the executor supports it |
| DEV-SEC-07 | PRJ-SEC-07 | Audit write failure is logged and the request continues (fail-open) | Unaudited write actions possible when disk is full | Fail-closed for non-GET requests |
| DEV-SEC-08 | PRJ-SEC-08, PRJ-SEC-09 | Hash chain proves integrity only; root on the HPC can regenerate the whole chain; segments are not signed | Undetected rewrite by a privileged attacker | Sign segments with an HSM-held key or stream to the vehicle SOC |
| DEV-SEC-10 | PRJ-SEC-10 | No server-stop record after a crash or SIGKILL | Missing stop marker | Treat a start without preceding stop as an unclean-shutdown indicator in log review |
| DEV-SW-01 | PRJ-SW-01 | Legacy `/api` execution call waits at most 8 s and then returns the current state | Long operations appear `running` on `/api` | Frontend migrates to `/sovd/v1` (ADR-0002) |
| DEV-SW-02 | PRJ-SW-02 | `error_code` values are project-defined | Clients cannot rely on ISO 17978-3 error codes | Map to the ISO 17978-3 error model |
| DEV-SW-03 | PRJ-SW-03 | Central requires all zones to use the same `zonal_http_port` | Mixed ports rejected at startup | Per-zone URL resolution |
| DEV-FE-01 | — | Host proxy on 18010 is a TCP forwarder; the browser frontend presents no client certificate to the mTLS-only central server | Frontend cannot reach central in the mTLS deployment | Frontend gateway with its own client certificate and viewer role (R13) |

These are recorded limitations of the demonstrator implementation. They are
not claims about what ISO 17978, ISO/SAE 21434 or UNECE R155 require.
