# System Architecture

## 1. Overview

The platform models a distributed Software Defined Vehicle using a central server, zonal controllers and virtual electronic control units.

The central server provides vehicle-level discovery, topology, health monitoring and diagnostic request routing. Zonal services expose the ECUs assigned to their respective zones.

## 2. Logical Components

### Central SOVD Server

Crate: `crates/sovd-server`

Responsibilities:

- Load vehicle topology.
- Expose vehicle-level HTTP APIs.
- Report zonal health.
- Discover ECUs across zones.
- Resolve an ECU identifier to its configured zone.
- Forward diagnostic requests to the corresponding zonal service.

The central server listens on port `8010` inside `sdv-central-ns`.

### Virtual ECU / Zonal Service

Crate: `crates/virtual-ecu`

Responsibilities:

- Load the selected zone configuration.
- Register the ECUs belonging to that zone.
- Expose zonal HTTP APIs on port `9001`.
- Create and manage UDS diagnostic dispatchers.
- Support diagnostic communication over ISO-TP and DoIP.

The configured zones are FRONT and REAR.

### UDS Protocol

Crate: `crates/uds-protocol`

Provides UDS diagnostic service processing.

The HTTP diagnostic interface accepts hexadecimal request payloads, converts them into bytes and dispatches them to the corresponding ECU diagnostic server.

### Vehicle Topology

Crate: `crates/vehicle-topology`

The topology configuration defines the vehicle, central node, zones, ECU assignments and communication parameters.

Configuration file:

`platform/config/vehicle-topology.toml`

### SOVD Studio

Directory: `sovd-studio/`

The frontend is built using React and TypeScript.

It communicates with the central API through the Vite development proxy. It does not communicate directly with individual zonal ECU processes.

## 3. Network Architecture

| Network component | Address |
|---|---|
| Central-front subnet | `10.77.10.0/30` |
| Central interface | `10.77.10.1` |
| Front interface | `10.77.10.2` |
| Central-rear subnet | `10.77.10.4/30` |
| Central rear-side interface | `10.77.10.5` |
| Rear interface | `10.77.10.6` |

Network namespaces:

- `sdv-central-ns`
- `sdv-front-ns`
- `sdv-rear-ns`

Virtual CAN interfaces:

- `vcan-front`
- `vcan-rear`

The host proxy listens on `127.0.0.1:18010` and forwards requests to the central server at port `8010` inside its network namespace.

## 4. Diagnostic Request Flow

For example, an ECM VIN read:

1. The frontend sends a POST request to `/api/ecus/ECM/diagnostics/requests`.
2. The host proxy forwards the request into the central namespace.
3. The central server resolves ECM using the topology configuration.
4. The central server identifies ECM as a FRONT zone ECU.
5. The request is forwarded to the FRONT zonal HTTP endpoint.
6. The virtual ECU service selects the ECM diagnostic dispatcher.
7. The UDS server processes service `0x22`, DID `0xF190`.
8. The response is returned through the zonal service and central server to the frontend.

## 5. UDS Dispatcher Design

The diagnostic dispatcher uses a bounded Tokio message queue and a dedicated worker thread for each ECU dispatcher.

The current implementation uses:

- Default queue capacity: 16 requests.
- Diagnostic response timeout: 2 seconds.
- Non-blocking queue submission using `try_send`.
- A synchronous response channel for the blocking diagnostic interface.
- `spawn_blocking` when invoked from the asynchronous HTTP handler.

This design prevents blocking UDS processing from directly occupying an asynchronous Tokio executor worker.

ISO-TP and DoIP use the same ECU diagnostic dispatcher instances, avoiding separate independent UDS state for those transports.

## 6. Engineering Considerations

- Queue saturation must be handled as a diagnostic overload condition.
- Dispatcher shutdown must account for outstanding sender handles and worker lifecycle.
- Diagnostic requests should have appropriate access control in a real vehicle environment.
- Network namespace isolation is a demonstration mechanism, not a replacement for production security.
- ECU response timing and transport timeouts must be validated under load.
