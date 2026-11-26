# Project Requirements

## 1. Purpose

Develop an independent SOVD platform that provides a standardized
diagnostic interface for virtual and physical automotive ECUs.

## 2. Deployment

Initial deployment:
- Linux PC
- Virtual diagnostic targets
- Local development and testing

Future deployment:
- Linux-based automotive gateway
- MCU-based ECUs
- Embedded diagnostic communication

## 3. Functional requirements

FR-01: The platform shall expose diagnostic functionality through SOVD APIs.

FR-02: The platform shall represent diagnostic targets as identifiable entities.

FR-03: The platform shall support diagnostic data access.

FR-04: The platform shall support diagnostic operations.

FR-05: The platform shall provide diagnostic fault information for identifiable
ECUs, including fault-list retrieval and retrieval of an individual fault by
its fault code.

FR-06: The platform shall support virtual ECUs.

FR-07: The platform shall provide an abstraction for diagnostic transports.

FR-08: The platform shall support integration with physical ECUs.

## 4. Non-functional requirements

NFR-01: Modular architecture.

NFR-02: Separation of API, diagnostic logic and transport layers.

NFR-03: Configurable diagnostic targets.

NFR-04: Testable interfaces and components.

NFR-05: Resource-aware embedded deployment.

NFR-06: Explicit error handling and logging.

## 4a. Security and robustness requirements

Added 2026-11-26 for the central SOVD server hardening (R1–R10). Register
rows, verification and evidence: `requirements.csv` (`PRJ-SEC-*`, `PRJ-SW-*`).

SEC-01: Raw UDS passthrough shall forward only allow-listed read-only services.

SEC-02: Every request shall be mapped to a role from the mTLS client certificate; unmapped clients shall be refused.

SEC-03: Write-type diagnostic actions shall require a role that permits them.

SEC-04: Concurrent in-flight requests shall be limited.

SEC-05: Request processing shall be bounded by a deadline.

SEC-06: Request body size shall be limited.

SEC-07: Every request, including denied requests, shall be audited without recording payloads or secrets.

SEC-08: Audit records shall be tamper-evident.

SEC-09: Audit log rotation shall preserve tamper evidence across files, and incomplete chains shall fail verification by default.

SEC-10: Server start and stop shall be audited.

SEC-11: User passwords shall be stored only as salted, slow hashes.

SEC-12: Login shall resist user enumeration and brute force.

SEC-13: User tokens shall be signed, time-limited and strictly verified.

SEC-14: Tool devices shall require a logged-in user; the effective role shall not exceed the device or the user role.

SEC-15: Audit records shall name the user behind each request.

SW-01: Both execution APIs shall share one execution store.

SW-02: Every error response shall carry a machine-readable JSON error body.

SW-03: Zonal service ports shall come from the vehicle topology configuration.

SW-04: Startup shall fail before serving requests if the execution manager cannot be created.

SW-05: The server shall shut down gracefully on SIGINT or SIGTERM.

## 5. Initial constraints

- Linux is the first development platform.
- Embedded hardware integration is a later milestone.
- The active Rust implementation is developed independently and does not reuse the archived C++ demonstrator implementation. The archived C++ project is retained for historical reference and engineering comparison.
- Upstream Eclipse OpenSOVD repositories are references only.

## 6. Open decisions

- SOVD API version and formal conformance scope.
- Authentication and authorization model (mTLS certificate roles implemented as interim model, SEC-02/SEC-03; OAuth2/OIDC per ISO 17978-3 not decided).
- Target embedded hardware and production deployment architecture.
- Physical CAN transceiver, wiring, termination and timing validation.
- Production diagnostic transport profiles and interoperability testing.

The Linux demonstrator architecture and its current transport implementation
are recorded in Section 7. These implementation choices do not close the
remaining product, hardware or standards-conformance decisions.

## 7. Current demonstrator baseline

The following describes the implemented Linux prototype, not a production
vehicle architecture or a declaration of standards compliance.

### 7.1 Software architecture

- The implementation is a Rust workspace using Cargo.
- The workspace is divided into diagnostic-common, virtual-ecu,
  uds-protocol, vehicle-topology, doip-gateway and sovd-server crates.
- A central HTTP server exposes vehicle-level ECU discovery and diagnostic
  endpoints.
- Two zonal virtual ECU runtimes represent FRONT and REAR zones.
- Runtime topology is loaded from
  platform/config/vehicle-topology.toml and validated at startup.

### 7.2 Virtual vehicle configuration

- Vehicle identifier: SDV-DEMO-001.
- Zones: FRONT and REAR.
- Virtual ECUs: ECM, LIGHTING and DOOR in FRONT; BCM and HVAC in REAR.
- Central API listener: TCP port 8010.
- Zonal HTTP listeners: TCP port 9001 in each zone namespace.
- DoIP listeners: TCP port 13400 on each zonal IP address.

### 7.3 Diagnostic transports and services

- Linux SocketCAN ISO-TP provides the virtual ECU diagnostic path over
  separate vcan-front and vcan-rear interfaces.
- The DoIP gateway implements the demonstrator's routing activation and
  diagnostic-message paths.
- The UDS engine currently supports selected services: DiagnosticSessionControl
  (0x10), ReadDataByIdentifier (0x22), TesterPresent (0x3E),
  ReadDTCInformation (0x19) and ClearDiagnosticInformation (0x14).
- UDS requests are processed by a bounded per-ECU dispatcher.
- The central HTTP API proxies ECU discovery, metadata, vehicle-speed data,
  diagnostic fault-list retrieval, individual fault retrieval and clear-fault
  operations to the zonal runtimes.
- Individual fault retrieval is exposed through the demonstrator route
  `/api/ecus/{id}/diagnostics/faults/{fault_code}`.

### 7.4 Validation boundary

- The prototype has been built and tested on Linux.
- Virtual network communication and end-to-end diagnostic requests have
  been exercised in the configured network namespaces.
- The vcan interfaces do not validate physical CAN signaling, bitrate,
  transceiver behavior, termination or electrical robustness.
- The implemented DoIP and UDS subsets are not evidence of complete
  ISO protocol conformance.
- The HTTP API is a demonstrator interface; formal ISO 17978 conformance
  has not been established.
- Physical ECU integration, embedded resource measurements, security
  validation and production qualification remain future work.
