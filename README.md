# Virtual Software Defined Vehicle (SOVD) Demonstrator

A configurable, distributed virtual Software Defined Vehicle platform
implemented in Rust, with a React and TypeScript diagnostic and monitoring
interface.

The project demonstrates a central vehicle-level HTTP server communicating
with front and rear zonal controllers, each managing virtual ECUs. It is an
open-source reference implementation under development, not a certified
production vehicle platform.

## 1. Architecture

```text
                     SOVD Studio
                  React + TypeScript
                     localhost:5173
                          |
                        HTTP API
                          |
                  Host Proxy :18010
                          |
                  Central SOVD Server
               sdv-central-ns :8010
                          |
              +-----------+-----------+
              |                       |
        Front Zonal              Rear Zonal
        Controller               Controller
        10.77.10.2               10.77.10.6
        :9001                    :9001
              |                       |
        +-----+------+           +----+-----+
        |     |      |           |          |
       ECM LIGHTING DOOR         BCM        HVAC
```

The central server loads vehicle topology and resolves each ECU to its
assigned zone. It forwards diagnostic requests to the corresponding zonal
service.

## 2. Technology stack

| Layer | Technology |
|---|---|
| Backend | Rust |
| HTTP server | Axum / Tokio |
| Serialization | Serde |
| HTTP client | Reqwest |
| Diagnostics | UDS |
| CAN transport | ISO-TP |
| Ethernet diagnostics | DoIP |
| Frontend | React + TypeScript |
| Build tooling | Cargo, npm, Vite |
| Virtual networking | Linux network namespaces |

A desktop wrapper should only be considered part of the supported stack
when its build and runtime integration are verified in the active
workspace.

## 3. Workspace structure

```text
VeloSOVD/
├── Cargo.toml
├── Cargo.lock
├── crates/
│   ├── diagnostic-common/
│   ├── virtual-ecu/
│   ├── uds-protocol/
│   ├── vehicle-topology/
│   ├── sovd-server/
│   └── doip-gateway/
├── platform/
│   └── config/
├── sovd-studio/
├── tools/
└── docs/
```

## 4. Demonstration topology

The topology is configured in:

`platform/config/vehicle-topology.toml`

The documented demonstration topology contains:

| Zone | ECU | Function |
|---|---|---|
| FRONT | ECM | Engine Control Module |
| FRONT | LIGHTING | Lighting Control Module |
| FRONT | DOOR | Door Control Module |
| REAR | BCM | Body Control Module |
| REAR | HVAC | Heating, Ventilation and Air Conditioning |

## 5. Running the platform

See [Development and Startup Guide](docs/DEVELOPMENT.md) for the
two-terminal workflow.

The normal workflow is:

- Terminal 1: configure the virtual network and start zonal ECU processes,
  the central SOVD server and host proxy.
- Terminal 2: start the React frontend using Vite.
- Browser: open `http://127.0.0.1:5173`.

Do not create duplicate network namespaces or start duplicate processes
when the platform is already running.

## 6. API documentation

See [Rust API Reference](docs/API.md).

Important central endpoints include:

| Method | Endpoint | Purpose |
|---|---|---|
| GET | `/api/health` | Vehicle and zone health |
| GET | `/api/topology` | Vehicle topology |
| GET | `/api/ecus` | ECU discovery |
| GET | `/api/ecus/{id}` | ECU information |
| GET | `/api/ecus/{id}/resources` | Resource discovery |
| GET | `/api/ecus/{id}/operations` | Operation discovery |
| GET | `/api/ecus/{id}/diagnostics/faults` | Diagnostic faults |
| GET | `/api/ecus/{id}/operations/executions` | Operation execution history |
| GET | `/api/ecus/{id}/operations/executions/{execution_id}` | Operation execution details |
| POST | `/api/ecus/{id}/diagnostics/operations/clear-faults` | Direct clear-fault operation |
| POST | `/api/ecus/{id}/operations/clear-faults/executions` | Clear-fault execution |
| POST | `/api/ecus/{id}/diagnostics/requests` | Raw UDS request |

Consult the API reference and current route registration for the complete
endpoint list.

## 7. Verified diagnostic example

Read the VIN DID using UDS service `0x22` and DID `0xF190`.

Request:

```json
{
  "payload_hex": "22 F1 90"
}
```

Example response:

```json
{
  "ecu_id": "ECM",
  "request_hex": "22F190",
  "response_hex": "62F19053445644454D4F30303030303030303031"
}
```

The positive response begins with `62 F1 90`.

The demonstration VIN bytes decode to:

`SDVDEMO00000000001`

## 8. Verification status

Historical verification on 13 November 2026 reported:

- Rust workspace formatting passed.
- Rust workspace compilation passed.
- Rust workspace tests: 83 passed.
- Frontend production build passed.
- Central health and zonal health endpoints were exercised.
- Front and rear zonal UDS requests were exercised.
- Central-to-zonal UDS routing was exercised for all five configured ECUs.

These results describe that verification run, not a guarantee that every
later source change has been tested.

See [Testing and Verification](docs/TESTING.md).

## 9. Known limitations

- This is a virtual SDV demonstrator, not a production vehicle platform.
- The configured VIN is demonstration data.
- Linux network namespaces and virtual CAN interfaces simulate the
  distributed network.
- TLS and production authentication are not established by the current
  development deployment.
- Operation execution history is in-memory and bounded.
- Full ASAM SOVD OpenAPI and ISO 17978 conformance have not been established.
- Full UDS, ISO-TP and DoIP conformance have not been established.
- The frontend build previously reported a large JavaScript bundle warning.
- Passing the current tests does not establish automotive safety or
  cybersecurity certification.

See [Known Deviations](docs/sovd_deviations.md) for the engineering
tracking register.

## 10. Documentation

- [Architecture](docs/ARCHITECTURE.md)
- [Development and Startup](docs/DEVELOPMENT.md)
- [API Reference](docs/API.md)
- [Testing and Verification](docs/TESTING.md)
- [Project requirements](docs/requirements/project-requirements.md)
- [Standards engineering baseline](docs/engineering/STANDARDS_COMPLIANCE_MATRIX.md)
- [Known deviations](docs/sovd_deviations.md)

## 11. License

This project is licensed under the Apache License, Version 2.0.
See the [LICENSE](LICENSE) file for the complete license text.

Third-party dependencies remain subject to their respective licenses.
