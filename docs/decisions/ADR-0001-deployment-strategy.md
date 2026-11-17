# ADR-0001: Deployment Strategy

## Status
Accepted

## Context
The platform needs fast Linux development and future embedded ECU integration.

## Options considered
1. Linux PC only
2. Linux PC with embedded ECUs
3. Embedded Linux only
4. Linux-first followed by hybrid deployment

## Decision
Option 4: Linux-first followed by hybrid deployment.

## Technical justification
- Faster development and debugging.
- Virtual ECUs enable repeatable testing.
- Hardware-specific transport remains isolated.
- Supports later CAN and DoIP integration.

## Consequences
- Virtual ECU behavior must represent real ECU behavior.
- Hardware timing and transport require separate validation.
- Embedded resource limits must influence dependency choices.

## Current implementation baseline

The accepted Linux-first decision has been implemented as a virtual
distributed vehicle prototype with:

- One central SOVD demonstrator server.
- Two zonal virtual ECU runtimes: FRONT and REAR.
- Five configured virtual ECUs: ECM, LIGHTING, DOOR, BCM and HVAC.
- Linux network namespaces and separate virtual CAN interfaces for the
  two zones.
- HTTP communication between the central server and zonal runtimes.
- SocketCAN ISO-TP and a DoIP diagnostic path to the virtual ECUs.
- A shared transport-independent UDS processing implementation.

The runtime topology is configured in
platform/config/vehicle-topology.toml.

This implementation validates the Linux virtual deployment approach.
It does not demonstrate deployment on an automotive gateway, operation
with physical ECUs, physical CAN electrical behavior or compliance with
a complete SOVD, UDS, ISO-TP or DoIP standard profile.

## Follow-up validation

Before hybrid or embedded deployment:

- Validate physical CAN and transceiver behavior on a hardware bench.
- Measure gateway and ECU timing under representative bus load.
- Measure CPU, RAM and Flash requirements on the selected target.
- Define diagnostic authentication, authorization and network trust
  boundaries.
- Validate protocol behavior against the selected normative standard
  editions and profiles.
