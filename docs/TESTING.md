# Testing and Verification

## 1. Rust Workspace

The following checks passed during the 13 November 2026 development session:

```bash
cargo fmt --all
cargo check --workspace
cargo test --workspace
cargo build -p sovd-server
cargo build -p virtual-ecu
```

Recorded test count: 83 tests passed.

Reported crate test counts:

| Crate | Tests passed |
|---|---:|
| doip-gateway | 49 |
| uds-protocol | 21 |
| virtual-ecu | 11 |
| vehicle-topology | 2 |
| Other suites and doc tests | 0 |

## 2. Frontend Build

Command:

```bash
cd sovd-studio
npm run build
```

The production build completed successfully.

A Vite warning reported a JavaScript bundle larger than 500 kB. The recorded minified JavaScript bundle was approximately 695.47 kB. Code splitting and lazy loading remain optimization tasks.

## 3. Zonal Health

The central health endpoint returned:

- Overall vehicle state: online.
- FRONT zone: online, HTTP 200.
- REAR zone: online, HTTP 200.

## 4. Direct Zonal UDS Tests

The direct front ECM request:

```bash
sudo ip netns exec sdv-front-ns \
  curl --max-time 5 -sS -i \
  -X POST http://127.0.0.1:9001/ecus/ECM/diagnostics/requests \
  -H "Content-Type: application/json" \
  -d '{"payload_hex":"22 F1 90"}'
```

The direct rear BCM request:

```bash
sudo ip netns exec sdv-rear-ns \
  curl --max-time 5 -sS -i \
  -X POST http://127.0.0.1:9001/ecus/BCM/diagnostics/requests \
  -H "Content-Type: application/json" \
  -d '{"payload_hex":"22 F1 90"}'
```

Both returned HTTP 200 and a positive UDS response beginning with `62F190`.

## 5. Central-to-Zonal UDS Routing

The following requests were sent through the host proxy:

```text
POST /api/ecus/ECM/diagnostics/requests
POST /api/ecus/LIGHTING/diagnostics/requests
POST /api/ecus/DOOR/diagnostics/requests
POST /api/ecus/BCM/diagnostics/requests
POST /api/ecus/HVAC/diagnostics/requests
```

Request payload:

```json
{
  "payload_hex": "22 F1 90"
}
```

All five requests returned HTTP 200.

## 6. Diagnostic Response Interpretation

The positive response starts with:

`62 F1 90`

- `0x62`: positive response for UDS service `0x22`.
- `0xF190`: requested VIN data identifier.
- Remaining bytes: returned VIN data.

The configured demonstration VIN is:

`SDVDEMO0000000001`

The current implementation returns this same demonstration value from all five virtual ECUs.

The same VIN is registered in the virtual ECU UDS server and verified by the DoIP-to-UDS VIN integration test.

## 7. Known Gaps

The recorded checks do not yet establish:

- GUI end-to-end validation through browser interaction.
- Multi-client load and dispatcher queue saturation behavior.
- ECU reset and recovery behavior.
- Full DoIP and ISO-TP interoperability with external diagnostic equipment.
- Security penetration testing or production authentication.
- Automotive standards conformance or functional safety certification.

These should be treated as future validation activities rather than completed tests.

## 8. Automated HTTP Payload-Forwarding Integration Test

The `sovd-server` integration-style test starts the actual central Axum router and a local mock zonal ECU HTTP server on operating-system-assigned ports. It does not require network namespaces or running ECU services.

Run the focused test:

```bash
cargo test -p sovd-server payload_forwarding_integration_tests
```

The test verifies:

- Input `22 f1 90` is forwarded to the mock ECU as `22f190`.
- The mock ECU receives exactly one valid request.
- Invalid hexadecimal input (`22G190`) returns HTTP 400 and is not forwarded.
- An unknown ECU returns HTTP 404 and is not forwarded.

This validates central-to-zonal HTTP payload normalization and forwarding against a mock HTTP ECU. It does not establish DoIP, ISO-TP, or physical ECU interoperability.

## 9. Automated DoIP-to-UDS VIN Integration Test

The `virtual-ecu` integration test starts the production DoIP listener on a loopback TCP port and connects a DoIP test client. It uses the production UDS dispatcher and UDS server rather than the `EchoHandler` used by the lower-level DoIP transport test.

Run the focused test:

```bash
cargo test -p virtual-ecu doip_reads_vin_through_production_uds_dispatcher -- --nocapture
```

The test verifies:

1. A TCP connection is established with the DoIP listener.
2. Routing activation succeeds for tester logical address `0x0E00`.
3. The diagnostic request `22 F1 90` is sent to ECU logical address `0x1110`.
4. The response is a DoIP diagnostic message (`0x8001`).
5. The diagnostic response addresses are reversed correctly:
   - Source: ECU `0x1110`.
   - Target: tester `0x0E00`.
6. The UDS positive response matches the test's expected bytes:

   `62 F1 90 SDVDEMO0000000001`

7. The listener and UDS dispatcher shut down cleanly.

This verifies that the loopback DoIP listener, routing activation, diagnostic routing, production UDS dispatcher, and VIN DID response work together in the virtual ECU implementation.

**Limitations:** This is a software loopback integration test. It does not establish interoperability with a physical ECU or external DoIP tester, ISO-TP communication, vehicle-network timing behavior, or automotive standards conformance.