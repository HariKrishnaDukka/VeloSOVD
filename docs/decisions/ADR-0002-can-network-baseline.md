# ADR-0002: CAN Network Baseline

## Status
Accepted for bench prototype; subject to engineering validation.

## Context
The BCM reference ECU requires communication with a zonal gateway.
The initial platform uses CAN for ECU communication and ISO-TP for
diagnostic transport.

## Decision
Use Classical CAN 2.0 at 500 kbit/s for the initial BCM demonstrator.

CAN FD is excluded from the initial physical BCM bench baseline.

## Current virtual demonstrator implementation

The Linux virtual vehicle prototype uses two separate SocketCAN virtual
interfaces:

- FRONT zone: vcan-front.
- REAR zone: vcan-rear.

The current topology assigns 11-bit CAN identifiers to each configured
virtual ECU. For example, the BCM uses request identifier 0x630 and
response identifier 0x638 on the REAR virtual CAN interface.

Linux vcan provides a software CAN networking interface. It does not
model or validate a physical bus bitrate, transceiver, termination,
electrical signal integrity or real-time bus behavior.

The configured virtual CAN identifiers and topology are implementation
facts for the current demonstrator. They do not, by themselves, approve
the physical BCM bench wiring or replace the hardware communication
matrix and its review.

## Initial assumptions
- One CAN network connects the BCM and gateway.
- ISO-TP provides diagnostic message transport.
- CAN identifiers, addressing mode and message definitions remain TBD.
- Transceiver, termination and physical wiring shall be finalized
  during hardware design.

## Rationale
- Sufficient for the current BCM control and diagnostic scope.
- Simpler initial hardware and software validation.
- Compatible with the planned CAN-based diagnostic transport.
- Avoids introducing CAN FD complexity without a demonstrated need.

## Consequences
- CAN bandwidth and bus load must be measured.
- Message identifiers and priorities require a communication matrix.
- ISO-TP timing and flow-control behavior require verification.
- Physical-layer signal integrity must be validated on hardware.
- CAN FD may be introduced later through a reviewed architecture change.

## Verification
- Verify operation at 500 kbit/s using a CAN analyzer.
- Verify communication between BCM and gateway.
- Measure bus load under expected traffic.
- Test error handling, bus-off recovery and missing messages.
- Verify ISO-TP segmentation and flow control.

## Open decisions
- 11-bit or 29-bit CAN identifiers.
- CAN message ID allocation and priorities.
- ISO-TP addressing mode.
- Network topology and termination.
- CAN transceiver and MCU CAN peripheral.
- Communication timeout and bus-off recovery strategy.
