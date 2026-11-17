# System Architecture and Runtime

## 1. Scope

This document describes the current implementation of the virtual SOVD
demonstrator. It records the runtime boundaries, network topology,
diagnostic request paths, configuration inputs, and known limitations.

It does not claim production readiness or full compliance with the SOVD,
UDS, ISO-TP, or DoIP standards.

## 2. Workspace organization

| Crate | Responsibility |
|---|---|
| `diagnostic-common` | Shared serializable ECU and diagnostic data types |
| `vehicle-topology` | Vehicle topology deserialization and validation |
| `uds-protocol` | Transport-independent UDS service processing |
| `doip-gateway` | DoIP framing, routing activation, TCP connection handling, and diagnostic router |
| `virtual-ecu` | Zonal runtime, ECU registry, HTTP endpoints, and diagnostic transport integration |
| `sovd-server` | Central HTTP API and zone proxy |

The `virtual-ecu` binary contains the ISO-TP endpoint, DoIP integration,
and UDS dispatcher modules.

## 3. Runtime boundaries

There are three Linux network namespaces:

| Namespace | Role |
|---|---|
| `sdv-central-ns` | Central SOVD API |
| `sdv-front-ns` | FRONT zonal ECU runtime |
| `sdv-rear-ns` | REAR zonal ECU runtime |

The central namespace has a point-to-point link to each zonal namespace.

### 3.1 IP topology

| Link | Central address | Zone address | Subnet |
|---|---|---|---|
| Central–FRONT | `10.77.10.1/30` | `10.77.10.2/30` | `10.77.10.0/30` |
| Central–REAR | `10.77.10.5/30` | `10.77.10.6/30` | `10.77.10.4/30` |

The network setup creates:

- `vcan-front` in `sdv-front-ns`
- `vcan-rear` in `sdv-rear-ns`

No default route or general IP forwarding is configured by this topology.

### 3.2 ECU assignment

| ECU | Zone | DoIP logical address | CAN interface | Request ID | Response ID |
|---|---|---:|---|---:|---:|
| ECM | FRONT | `0x1110` | `vcan-front` | `0x620` | `0x628` |
| LIGHTING | FRONT | `0x1111` | `vcan-front` | `0x650` | `0x658` |
| DOOR | FRONT | `0x1112` | `vcan-front` | `0x660` | `0x668` |
| BCM | REAR | `0x1121` | `vcan-rear` | `0x630` | `0x638` |
| HVAC | REAR | `0x1122` | `vcan-rear` | `0x640` | `0x648` |

Zone DoIP logical addresses are `0x1101` for FRONT and `0x1102` for REAR.

## 4. Topology loading and validation

Both central and zonal binaries load:

`platform/config/vehicle-topology.toml`

The `SDV_TOPOLOGY` environment variable overrides the default path.

The topology loader validates, among other things:

- Non-empty vehicle and namespace identifiers
- Non-zero central and DoIP ports
- Valid zone IP addresses
- Unique zone IDs and namespace names
- Unique ECU IDs
- ECU zone references
- ECU CAN interface consistency with its zone
- Unique DoIP logical addresses
- 11-bit CAN identifier bounds
- CAN request/response identifier uniqueness per interface
- Different request and response CAN identifiers

The zonal runtime requires `SDV_ZONE` and selects only the ECUs assigned
to that zone. An empty selection is a startup error.

The central server uses the topology to map ECU IDs to zonal IP addresses.

`platform/config/ecus.toml` is a separate configuration file. It currently
contains only ECM, BCM, and HVAC HTTP base URLs and is not the active
five-ECU runtime inventory described above.

## 5. HTTP request architecture

### 5.1 Central API

The central SOVD server binds using the topology's central bind address
and port. Current configuration: `0.0.0.0:8010`.

Routes:

| Method | Path |
|---|---|
| GET | `/api/ecus` |
| GET | `/api/ecus/{id}` |
| GET | `/api/ecus/{id}/data/vehicle-speed` |
| GET | `/api/ecus/{id}/diagnostics/faults` |
| POST | `/api/ecus/{id}/diagnostics/operations/clear-faults` |

The ECU-specific central routes resolve the ECU's zone and proxy to:

`http://{zonal_ip}:9001/ecus/{ecu_id}`

The ECU list route queries each configured zone's `/ecus` endpoint and
combines the results. The central HTTP client has a three-second timeout.
Upstream connection, HTTP status, or response decoding failures are mapped
to `502 Bad Gateway`.

### 5.2 Zonal API

Each zonal process binds its HTTP API to `0.0.0.0:9001` by default.
`SDV_HTTP_BIND` overrides this address.

Routes:

| Method | Path |
|---|---|
| GET | `/ecus` |
| GET | `/ecus/{id}` |
| GET | `/ecus/{id}/data/vehicle-speed` |
| GET | `/ecus/{id}/diagnostics/faults` |
| POST | `/ecus/{id}/diagnostics/operations/clear-faults` |

The zonal API exposes only the ECUs assigned to its selected zone.

## 6. UDS processing architecture

The UDS engine is transport-independent. It accepts UDS payloads and
produces a response payload or a suppressed response.

Current implemented services:

| SID | Service | Implemented behavior |
|---:|---|---|
| `0x10` | DiagnosticSessionControl | Diagnostic session switching |
| `0x22` | ReadDataByIdentifier | Reads registered DIDs |
| `0x3E` | TesterPresent | Tester presence handling |
| `0x19` | ReadDTCInformation | Subfunction `0x02` |
| `0x14` | ClearDiagnosticInformation | Group `0xFFFFFF` |

The default S3 timeout is five seconds. Expiration is checked when a
subsequent request is processed; there is no background S3 timer.

The current DID setup registers:

- `0xF190`: VIN `SDVDEMO0000000001`, default and extended sessions
- `0xF187`: ECU-specific software version, extended session

Each ECU owns an independent UDS server/session and DTC store.

## 7. UDS dispatcher and concurrency

Each ECU has one UDS dispatcher actor. The actor owns the mutable UDS server
and processes requests serially.

The dispatcher uses a bounded Tokio channel. The configured default queue
capacity is 16 requests per ECU.

Synchronous transport threads submit requests through a blocking adapter.
The adapter uses a bounded response channel and a two-second response wait.
Queue-full and actor-unavailable conditions are reported instead of
allowing unbounded request accumulation.

The dispatcher is shared by the ECU's ISO-TP and DoIP diagnostic paths.
This gives both transports access to the same UDS session and diagnostic
state for that ECU.

A caller timing out does not currently cancel work already accepted by the
actor. This should be considered when extending the dispatcher or adding
long-running diagnostic services.

## 8. ISO-TP path

The ISO-TP implementation uses Linux SocketCAN ISO-TP sockets.

The kernel handles ISO-TP segmentation, reassembly, and flow control.
The application receives complete UDS payloads, submits them to the
ECU-specific dispatcher, and writes the resulting UDS response.

The implementation uses one OS thread per ECU endpoint. The current
maximum UDS buffer is 4096 bytes.

This path depends on Linux SocketCAN ISO-TP support and the configured
CAN interface and identifiers.

## 9. DoIP path

The zonal runtime binds a TCP DoIP listener to the zone's configured zonal
IP address and port. The current port is `13400`; `SDV_DOIP_BIND` can
override the bind address.

The DoIP router maps ECU logical addresses to the same dispatcher handles
used by ISO-TP.

The current implementation supports:

- DoIP version/header processing
- Routing activation
- Diagnostic messages
- ECU logical-address routing
- A bounded connection count

The implementation is intentionally a subset. Generic NACK, diagnostic
ACK/NACK, alive check, vehicle identification, entity status, and complete
production session/security handling are not implemented.

Do not describe this implementation as complete ISO 13400 conformance.

## 10. DTC state and fault operations

Each ECU has a DTC store shared by its HTTP-visible state and UDS server.
The store is protected by a mutex and shared through an `Arc`.

The HTTP fault-read route converts the stored DTC entries into the common
diagnostic fault response.

The REST clear-faults route directly clears the shared in-memory DTC store.
UDS service `0x14` separately clears that same store through the UDS engine.

Important distinction: the REST operation does not currently dispatch
a UDS `0x14` request over ISO-TP or DoIP. Both paths share state, but the
REST handler is a direct in-memory operation.

Initial simulated faults:

- ECM: `P0128`, coolant thermostat temperature below threshold
- BCM: `B1000`, body control configuration fault

The ECM sample speed is `65.0 km/h`; the other configured ECUs currently
use `0.0 km/h`.

These values are simulation data, not sensor measurements. DTC persistence
across process restarts is not implemented.

## 11. Build and launch

Build:

```bash
source "$HOME/.cargo/env"
cargo build --workspace
```

Create the network if required:

```bash
sudo tools/setup_sovd_network.sh up
```

Start FRONT:

```bash
sudo ip netns exec sdv-front-ns \
  env SDV_ZONE=FRONT \
  "$PWD/target/debug/virtual-ecu"
```

Start REAR:

```bash
sudo ip netns exec sdv-rear-ns \
  env SDV_ZONE=REAR \
  "$PWD/target/debug/virtual-ecu"
```

Start central:

```bash
sudo ip netns exec sdv-central-ns \
  "$PWD/target/debug/sovd-server"
```

Run these processes in separate terminals. Build the binaries before launching
them. Do not tear down the namespaces while the processes are running.

## 12. Verification

Run the workspace checks:

```bash
cargo fmt --check
cargo check --workspace
cargo test --workspace
git diff --check
```

Query the central API:

```bash
sudo ip netns exec sdv-central-ns \
  curl http://127.0.0.1:8010/api/ecus
```

Read ECM faults:

```bash
sudo ip netns exec sdv-central-ns \
  curl http://127.0.0.1:8010/api/ecus/ECM/diagnostics/faults
```

Clear ECM faults:

```bash
sudo ip netns exec sdv-central-ns \
  curl -X POST \
  http://127.0.0.1:8010/api/ecus/ECM/diagnostics/operations/clear-faults
```

A successful clear changes the in-memory state. Restarting the zonal process
reinitializes the simulated ECU state and its initial sample faults.

## 13. Known limitations and engineering follow-ups

- The platform is a Linux virtual demonstrator, not a physical vehicle network.
- `vcan` does not model electrical behavior, arbitration timing, bus errors,
  termination, or physical-layer faults.
- Network configuration values are duplicated between the topology TOML
  and `tools/setup_sovd_network.sh`.
- `platform/config/ecus.toml` is not synchronized with the complete ECU
  inventory in `vehicle-topology.toml`.
- Simulated speed and DTC values are not backed by physical signals.
- DTC persistence is not implemented.
- REST clear-faults directly accesses the shared store instead of invoking
  UDS service `0x14` through a diagnostic transport.
- DoIP currently implements only a subset of the protocol.
- UDS session state is per ECU server, not per tester connection.
- Diagnostic security access and production authentication are not implemented.
- Dispatcher work accepted before a caller timeout is not cancelled.

Potential future engineering work includes a single topology source of truth
for network setup, persistent DTC storage, per-tester diagnostic sessions,
and broader DoIP protocol coverage.
