# Local End-to-End Verification Harness

## Purpose

`tools/run_local_checks.sh` is the privileged Linux integration harness for the Rust virtual-SDV/SOVD deployment. It complements the hermetic Cargo tests by starting the actual central and zonal processes in isolated network namespaces and exercising the HTTP, DoIP and ISO-TP paths.

Run it from any directory:

```bash
./tools/run_local_checks.sh
```

The harness builds the `sovd-server` and `virtual-ecu` binaries, creates the network declared by `tools/setup_sovd_network.sh`, starts FRONT, REAR and central services, and records each check as `[PASS]`, `[FAIL]` or `[SKIP]`. A skipped check is never counted as a pass. The final exit status is non-zero if any check failed.

## Preconditions

The host must be Linux and provide:

- Rust/Cargo and the repository's locked dependencies.
- `iproute2`, `iputils-ping`, `sudo`, and permission to create network namespaces.
- Kernel virtual CAN support and permission to load `vcan` and the Linux CAN ISO-TP protocol (`can-isotp`, either modular or built into the kernel).
- Python 3, `curl`, `timeout`, and `xxd`.
- `can-utils`, specifically `isotpsend` and `isotprecv`.

The script requests sudo authorization once. It does not read, write, or require application credentials, tokens, certificates, or other secrets.

Do not run it while another VeloSOVD deployment is using the three namespace names or the configured ports. It refuses to adopt an existing namespace or the setup script's ownership marker.

## Executed checks

1. Build the central and virtual ECU binaries.
2. Create `sdv-central-ns`, `sdv-front-ns` and `sdv-rear-ns`, their veth links, and `vcan-front` / `vcan-rear`; verify namespace connectivity and vCAN link state.
3. Start one FRONT virtual ECU process, one REAR virtual ECU process, and the central SOVD process. Probe both zonal HTTP endpoints, central health, and the FRONT DoIP TCP listener.
4. Read `GET /api/ecus/ECM/diagnostics/faults` and verify the simulator's startup-seeded `P0128` DTC is present. The current virtual ECU seeds this DTC in `make_ecu_from_topology`; there is no separate public DTC-injection endpoint, so the harness verifies that deterministic fixture rather than inventing a test-only production API.
5. Start the SOVD operation at `POST /sovd/v1/components/ECM/operations/clear-faults/executions`. Verify the returned execution ID appears in the SOVD execution collection, poll its individual execution resource until terminal state and require `completed`; then read the fault-list route and require an empty list. The same collection-plus-status checks require the negative execution to be present and `failed`.
6. Stop only the FRONT process launched by this invocation. Start another SOVD clear execution while the FRONT zonal endpoint is unavailable and require the execution resource to reach `failed`. Restart FRONT and verify HTTP recovery. This is a controlled upstream transport failure; it does not inject an artificial UDS negative response or modify central/ECU source code.
7. Run `tools/doip_independent_client.py`, a standalone Python standard-library client (not the project's DoIP implementation). It opens a TCP connection to FRONT, performs routing activation as tester `0x0E00`, sends `22 F1 90` to ECM logical address `0x1110` and verifies a `62 F1 90` positive response, then sends `14 FF FF FF` and requires the exact one-byte response `54`.
8. Use `isotpsend` and `isotprecv` on `vcan-front` to send an 11-byte UDS request. Its length forces ISO-TP multi-frame segmentation/reassembly. The currently supported UDS service set does not implement WriteDataByIdentifier (`0x2E`), so the expected application response is `7F 2E 11`; the transport check is the successful delivery/reassembly of the full request and receipt of that negative response.
9. Run Rust formatting, workspace Clippy and workspace tests.
10. Stop the service processes recorded by this run and tear down the network only when the setup ownership marker proves that this run created it. If setup fails before writing the marker, the setup script performs its own rollback and the harness verifies that none of the three namespace names remain.

## Output and evidence handling

Each command's stdout and stderr are printed and also retained in a unique directory under `/tmp/velosovd-local-checks.*`. The run log is `run.log`; separate FRONT, REAR and central service logs are retained there as well. These logs may contain simulator diagnostics but no secrets are supplied by the harness.

The script prints counts for PASS, FAIL and SKIP and exits non-zero for any failure. Cleanup runs from an EXIT/INT/TERM trap. A cleanup failure is itself a FAIL and forces a non-zero exit.

## Scope and limitations

This is a virtual Linux integration test. It does not establish physical CAN-bus operation, physical ECU interoperability, automotive timing compliance, ISO 14229/ISO 13400 conformance, production security, or functional safety. The independent DoIP client validates the implemented DoIP path using a separately written client, not a third-party diagnostic tester. The ISO-TP check uses Linux SocketCAN virtual CAN and can-utils.

The DTC fixture is simulator startup data, not a separate external seeding command. The negative case tests a zonal-service outage and confirms failed execution recording; it is not a simulated ECU NRC. The multi-frame ISO-TP request deliberately uses an unsupported UDS SID so it cannot alter ECU data.

No full privileged execution result is claimed by this document. The harness must be run on the target Linux host with the listed kernel facilities and utilities to produce runtime evidence.
