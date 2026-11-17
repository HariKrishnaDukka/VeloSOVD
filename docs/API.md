# Rust Central SOVD HTTP API Reference

This document describes the HTTP API exposed by the Rust central SOVD
server in the current virtual SDV demonstrator.

It is an implementation reference, not a reproduction of the ISO 17978
standard or a claim of conformance.

## 1. Development endpoint

Host-side development URL:

`http://127.0.0.1:18010`

The central server listens on port `8010` inside `sdv-central-ns`.
The host proxy exposes it on port `18010`.

The current development deployment uses HTTP. Do not expose this endpoint
to an untrusted network.

## 2. Endpoint catalogue

The following routes are part of the Rust server API. ECU-specific routes
use the ECU identifier configured in the vehicle topology.

| Method | Endpoint | Purpose |
|---|---|---|
| GET | `/api/health` | Vehicle and zonal health |
| GET | `/api/topology` | Vehicle topology |
| GET | `/api/ecus` | ECU discovery |
| GET | `/api/ecus/{id}` | ECU information |
| GET | `/api/ecus/{id}/data/vehicle-speed` | Vehicle speed data |
| GET | `/api/ecus/{id}/resources` | Discover ECU resources |
| GET | `/api/ecus/{id}/operations` | Discover ECU operations |
| GET | `/api/ecus/{id}/diagnostics/faults` | Read diagnostic faults |
| GET | `/api/ecus/{id}/operations/executions` | List operation executions |
| GET | `/api/ecus/{id}/operations/executions/{execution_id}` | Read an operation execution |
| POST | `/api/ecus/{id}/diagnostics/requests` | Submit a raw UDS request |
| POST | `/api/ecus/{id}/diagnostics/operations/clear-faults` | Direct clear-fault operation |
| POST | `/api/ecus/{id}/operations/clear-faults/executions` | Execute the clear-fault operation |

Confirm the route registration in `crates/sovd-server/src/main.rs` before
treating this table as a stable public API. Both clear-fault routes are currently registered in the Rust server.
The direct route returns the ECU operation result; the execution route
also records an entry in the central in-memory execution history.

## 3. Health and topology

### `GET /api/health`

Returns vehicle-level and zonal health information.

The central health response uses these fields:

- `vehicle_id`
- `vehicle_name`
- `overall`
- `zones`

Each zone entry contains:

- `id`
- `name`
- `online`
- `detail`

### `GET /api/topology`

Returns the configured vehicle, central node, zones and ECU topology.

## 4. ECU discovery

### `GET /api/ecus`

Returns the ECU list aggregated from zonal services.

### `GET /api/ecus/{id}`

Returns information for the selected ECU.

The configured demonstration topology contains:

| ECU | Zone |
|---|---|
| ECM | FRONT |
| LIGHTING | FRONT |
| DOOR | FRONT |
| BCM | REAR |
| HVAC | REAR |

## 5. Resource discovery

### `GET /api/ecus/{id}/resources`

Returns resource descriptors for the selected ECU.

A resource descriptor contains:

- `id`
- `path`
- `kind`
- `description`
- `methods`
- `simulated`

The `simulated` field identifies resources represented by the virtual
demonstrator. It must not be interpreted as proof of a physical ECU
implementation.

## 6. Operation discovery and execution

### `GET /api/ecus/{id}/operations`

Returns operation descriptors for the selected ECU.

An operation descriptor contains:

- `id`
- `name`
- `method`
- `execution_path`
- `description`
- `supports_execution`

### `POST /api/ecus/{id}/diagnostics/operations/clear-faults`

Invokes the clear-fault operation directly through the zonal ECU service.
This route returns the ECU operation result and does not create an entry
in the central operation-execution history.

### `POST /api/ecus/{id}/operations/clear-faults/executions`

Requests execution of the clear-fault operation for the selected ECU.

The operation execution representation contains:

- `execution_id`
- `ecu_id`
- `operation`
- `status`
- `success`
- `message`
- `created_at_unix_ms`

The current execution history is held in process memory. Its capacity is
limited to 128 entries. Entries are not persistent and are lost when the
server restarts.

### `GET /api/ecus/{id}/operations/executions`

Lists recorded operation executions for the selected ECU.

### `GET /api/ecus/{id}/operations/executions/{execution_id}`

Retrieves a recorded execution by its identifier.

Do not assume that this execution model provides asynchronous job
semantics, cancellation, durable history, or exactly-once execution.

## 7. Diagnostic faults

### `GET /api/ecus/{id}/diagnostics/faults`

Returns diagnostic fault information for the selected ECU.

An optional query parameter is supported:

`active=true`

or

`active=false`

The query parameter is represented internally as an optional Boolean.
Confirm the exact filtering behaviour and response schema in the current
handler implementation before building an external client against it.

## 8. Vehicle speed

### `GET /api/ecus/{id}/data/vehicle-speed`

Returns the vehicle-speed data exposed by the selected ECU.

The exact response representation should be taken from the current ECU
service implementation.

## 9. Raw UDS diagnostic request

### `POST /api/ecus/{id}/diagnostics/requests`

Content type:

`application/json`

Request body:

```json
{
  "payload_hex": "22 F1 90"
}
```

Example:

```bash
curl --max-time 5 -sS -i \
  -X POST http://127.0.0.1:18010/api/ecus/ECM/diagnostics/requests \
  -H "Content-Type: application/json" \
  -d '{"payload_hex":"22 F1 90"}'
```

The request payload is hexadecimal UDS data. `22 F1 90` requests the
VIN DID using ReadDataByIdentifier.

The response representation contains:

- `ecu_id`
- `request_hex`
- `response_hex`

`response_hex` is optional and may be absent when no response is returned.

An example successful response observed during development was:

```json
{
  "ecu_id": "ECM",
  "request_hex": "22F190",
  "response_hex": "62F19053445644454D4F30303030303030303031"
}
```

The positive response begins with `62 F1 90`.

## 10. Input validation and errors

The central raw UDS handler validates the hexadecimal payload.

The current validation implementation:

- Rejects an empty payload.
- Rejects an odd number of hexadecimal characters.
- Accepts ASCII whitespace between hexadecimal characters.
- Rejects non-hexadecimal characters.
- Limits the input string and compacted hexadecimal payload to 8192
  characters.

Relevant status conditions include:

| HTTP status | Meaning |
|---|---|
| 400 | Invalid diagnostic payload |
| 404 | ECU identifier not found |
| 413 | Diagnostic payload exceeds the configured limit |
| 500 | Internal or topology-related error |
| 502 | Failure communicating with a zonal service |

Exact status propagation and error response bodies are handler-dependent.
The current implementation must not be described as providing a single,
standardized SOVD error envelope unless that is verified separately.

## 11. Authentication and transport security

The development API currently uses HTTP. OAuth 2.0, OpenID Connect,
certificate-based authentication, authorization policy enforcement and
production TLS configuration are not established by this API reference.

Do not expose the development server as a production vehicle diagnostic
interface.

## 12. Deployment and verification context

The host-side API is reached through the local proxy at
`127.0.0.1:18010`. The central server runs inside `sdv-central-ns`.

The demonstration topology has two zones and five virtual ECUs. Historical
verification on 13 November 2026 reported successful central-to-zonal UDS
routing for all five configured ECUs.

See:

- [Architecture](ARCHITECTURE.md)
- [Development and Startup](DEVELOPMENT.md)
- [Testing and Verification](TESTING.md)
- [Known deviations](sovd_deviations.md)
