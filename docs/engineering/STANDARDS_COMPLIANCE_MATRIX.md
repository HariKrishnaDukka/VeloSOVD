# Standards Compliance Matrix

**Document ID:** ENG-STD-001

**Version:** 0.2

**Baseline date:** 2026-11-17

**Status:** Draft — engineering review required

**Scope:** SOVD Rust platform, five-ECU virtual vehicle and BCM reference ECU

## 1. Purpose

This document identifies standards relevant to the development of:

- The SOVD diagnostic platform and server APIs.
- The five-ECU virtual vehicle demonstrator.
- Zonal gateways and diagnostic transports.
- The BCM reference ECU and its future embedded implementation.
- Future bare-metal/HAL and AUTOSAR Classic ECU implementations.

The current demonstrator contains ECM, LIGHTING, DOOR, BCM and HVAC virtual ECUs. BCM is the primary reference ECU for detailed embedded implementation and engineering analysis. The other four ECUs remain within the vehicle-level diagnostic platform scope.

This matrix is an engineering planning baseline. It does not establish conformity, certification or compliance.

The exact contractual, regulatory and product scope must be agreed before any formal compliance claim.

## 2. Applicability classifications

- Direct: directly relevant to a protocol or product capability in scope.
- Conditional: applicability depends on safety, security, deployment or contractual analysis.
- Future: applicable when the corresponding capability is introduced.
- Reference: useful engineering guidance, but not itself an ISO requirement for this project.

An applicability classification does not imply that the implementation has been assessed against the standard.

## 3. Primary SOVD standards

| Standard | Edition | Scope in this project | Required engineering evidence |
|---|---|---|---|
| ISO 17978-1 | 2026, Edition 1 | SOVD principles, terminology and rules | Architecture and terminology review |
| ISO 17978-2 | 2026, Edition 1 | SOVD use cases | Use-case-to-requirement traceability |
| ISO 17978-3 | 2026, Edition 1 | SOVD API | API specification mapping and conformance tests |

The official ISO 17978-3 OpenAPI package has been obtained and structurally inspected. Its recorded metadata identifies OpenAPI 3.1.0 and ASAM SOVD V1.1.0.

The package inspection does not replace review of the complete normative ISO document. Detailed API requirement mapping and conformance assessment remain pending.

## 4. Supporting diagnostic and communication standards

The following standards are relevant to the current or planned diagnostic architecture. Their exact applicable editions and parts must be confirmed against authoritative sources before they are used as acceptance criteria.

| Standard | Edition / status | Scope in this project | Required engineering evidence |
|---|---|---|---|
| ISO 14229-1 | Edition and applicable amendments pending verification | UDS application-layer diagnostic services | Service implementation, session rules and negative-response tests |
| ISO 14229-2 | Edition pending verification | UDS session-layer services | Session and timing behavior tests |
| ISO 14229-3 | Edition pending verification | UDS on CAN | CAN diagnostic profile tests |
| ISO 15765-2 | Edition pending verification | Diagnostic transport over CAN (ISO-TP) | Segmentation, flow-control, timeout and error tests |
| ISO 11898-1 | Edition pending verification | CAN data-link layer | CAN controller and network validation |
| ISO 11898-2 | Edition pending verification | High-speed CAN physical layer | Transceiver, termination and physical-bus validation |
| ISO 13400-2 | Edition pending verification | DoIP transport and network layer | Routing activation, connection handling and diagnostic-message tests |

The Rust demonstrator contains UDS, DoIP and SocketCAN-related components. The existence of these components does not establish full protocol conformance.

## 5. Conditional standards

| Standard | Edition / status | Applicability condition | Required action |
|---|---|---|---|
| ISO 26262 series | 2018 editions; revision status to be monitored | Safety-related vehicle functions or production-intended E/E systems | Perform item definition and hazard analysis; determine applicability and ASIL scope |
| ISO/SAE 21434 | 2021; revision status to be monitored | Automotive E/E systems and interfaces with cybersecurity risks | Perform TARA and define cybersecurity requirements and lifecycle evidence |
| ISO 20077 series | Applicable editions to be confirmed | Extended Vehicle methodology is used by the SOVD architecture | Review relevant parts and document architecture impact |

Functional safety shall not be assumed applicable at a particular ASIL without a hazard analysis. A demonstrator is not automatically a production safety-qualified ECU.

Cybersecurity shall be considered from architecture inception, including diagnostic access, authentication, authorization, network exposure, credential handling, logging and update paths.

## 6. Future-scope standards

| Standard | Edition / status | Trigger |
|---|---|---|
| ISO 24089 | 2023; Amendment 1:2024 | ECU/software update engineering is introduced |
| Applicable secure boot and cryptographic specifications | To be selected | Secure boot, firmware authenticity and key management are designed |
| Applicable vehicle cybersecurity regulations | To be determined by target market | Product is integrated into a regulated vehicle program |

ISO 24089 shall be revisited before implementing software update workflows. It is not a substitute for cybersecurity risk analysis.

## 7. Related industry frameworks (not ISO standards)

| Framework | Relevance |
|---|---|
| AUTOSAR Classic Platform | ECU software architecture and standardized interfaces |
| Automotive SPICE | Software and systems engineering process assessment |
| IATF 16949 | Automotive quality management system for organizations in its scope |

These frameworks must not be described as ISO standards.

## 8. Project-level engineering controls

The following controls apply throughout development:

1. Every system requirement shall have a unique identifier.
2. Requirements shall be verifiable and have measurable acceptance criteria.
3. Requirements shall trace to design elements and test cases.
4. Protocol implementations shall be checked against the applicable standard edition and selected profile.
5. Security assumptions and trust boundaries shall be documented.
6. Safety-related assumptions shall be recorded and reviewed.
7. Source changes shall be version-controlled and reviewed.
8. Test results, failures and corrective actions shall be retained.
9. Deviations and non-applicable clauses shall have documented rationale.
10. Standard editions and applicability shall be reviewed at project gates.

## 9. Compliance status

| Area | Current status |
|---|---|
| Standards identification | Initial baseline established |
| ISO 17978 catalogue review | Completed for Parts 1, 2 and 3 |
| ISO 17978-3 OpenAPI package inspection | Completed |
| SOVD operation execution demonstrator (GAP-09) | Synchronous POST execution and execution retrieval implemented and functionally tested; normative API mapping and conformance assessment pending |
| Full normative ISO review | Pending |
| Clause-level analysis | Not started |
| Requirements mapping | Not started |
| Architecture assessment | In progress |
| Implementation assessment | Not started |
| Supporting protocol edition verification | Pending |
| Verification evidence | Not started |
| Independent assessment | Not started |
| Compliance claim | Not permitted |

## 10. Official source register

The following ISO catalogue pages are recorded in `docs/standards/README.md`:

- ISO 17978-1:2026
  https://www.iso.org/standard/85133.html
- ISO 17978-2:2026
  https://www.iso.org/standard/86586.html
- ISO 17978-3:2026
  https://www.iso.org/standard/86587.html

The supporting protocol standards must be checked against their official catalogue records and the applicable purchased normative documents before their editions are finalized.

The source-access history and package inspection evidence are maintained in:

- `docs/standards/README.md`
- `docs/standards/source-access-log.csv`

## 11. Review and approval

This document remains Draft until reviewed.

| Role | Responsibility | Status |
|---|---|---|
| System architect | Confirm product and system scope | Pending |
| ECU architect | Confirm BCM and other ECU standards scope | Pending |
| Cybersecurity reviewer | Review cybersecurity applicability | Pending |
| Functional safety reviewer | Review safety applicability | Pending |
| Verification lead | Review evidence and test strategy | Pending |

No standards compliance or certification claim is authorized by this document.