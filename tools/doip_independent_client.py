#!/usr/bin/env python3
"""Independent DoIP smoke client using only Python's standard library."""

import socket
import struct
import sys

VERSION = 0x02
INVERSE_VERSION = 0xFD
ROUTING_ACTIVATION_REQUEST = 0x0005
ROUTING_ACTIVATION_RESPONSE = 0x0006
DIAGNOSTIC_MESSAGE = 0x8001
DIAGNOSTIC_MESSAGE_ACK = 0x8002


def frame(payload_type: int, payload: bytes) -> bytes:
    return struct.pack("!BBHI", VERSION, INVERSE_VERSION, payload_type, len(payload)) + payload


def receive_frame(sock: socket.socket) -> tuple[int, bytes]:
    header = recv_exact(sock, 8)
    version, inverse, payload_type, length = struct.unpack("!BBHI", header)
    if version != VERSION or inverse != INVERSE_VERSION:
        raise RuntimeError(f"invalid DoIP header version pair {version:02X}/{inverse:02X}")
    return payload_type, recv_exact(sock, length)


def recv_exact(sock: socket.socket, length: int) -> bytes:
    data = bytearray()
    while len(data) < length:
        chunk = sock.recv(length - len(data))
        if not chunk:
            raise RuntimeError("peer closed the DoIP connection before the frame was complete")
        data.extend(chunk)
    return bytes(data)


def send_uds(sock: socket.socket, tester: int, ecu: int, request: bytes) -> bytes:
    payload = struct.pack("!HH", tester, ecu) + request
    sock.sendall(frame(DIAGNOSTIC_MESSAGE, payload))
    payload_type, response = receive_frame(sock)
    if payload_type == DIAGNOSTIC_MESSAGE_ACK:
        raise RuntimeError(f"DoIP diagnostic negative ACK: {response.hex().upper()}")
    if payload_type != DIAGNOSTIC_MESSAGE or len(response) < 5:
        raise RuntimeError(f"unexpected DoIP response type=0x{payload_type:04X} payload={response.hex().upper()}")
    source, target = struct.unpack("!HH", response[:4])
    if source != ecu or target != tester:
        raise RuntimeError(f"unexpected DoIP address pair source=0x{source:04X} target=0x{target:04X}")
    return response[4:]


def main() -> int:
    if len(sys.argv) != 5:
        print("usage: doip_independent_client.py HOST PORT TESTER_LOGICAL_ADDRESS ECU_LOGICAL_ADDRESS", file=sys.stderr)
        return 2

    host = sys.argv[1]
    port = int(sys.argv[2])
    tester = int(sys.argv[3], 0)
    ecu = int(sys.argv[4], 0)

    print(f"Independent DoIP client: {host}:{port}, tester=0x{tester:04X}, ECU=0x{ecu:04X}")
    with socket.create_connection((host, port), timeout=5) as sock:
        sock.settimeout(5)
        activation = struct.pack("!HB4s", tester, 0x00, b"\x00" * 4)
        sock.sendall(frame(ROUTING_ACTIVATION_REQUEST, activation))
        payload_type, response = receive_frame(sock)
        print(f"Routing activation response: type=0x{payload_type:04X} payload={response.hex().upper()}")
        if payload_type != ROUTING_ACTIVATION_RESPONSE or len(response) != 9:
            raise RuntimeError("routing activation response has unexpected type or length")
        response_tester, entity, response_code = struct.unpack("!HHB", response[:5])
        if response_tester != tester or response_code != 0x10:
            raise RuntimeError(
                f"routing activation rejected: tester=0x{response_tester:04X}, "
                f"entity=0x{entity:04X}, code=0x{response_code:02X}"
            )
        print(f"PASS routing activation: entity=0x{entity:04X}, response_code=0x10")

        vin_response = send_uds(sock, tester, ecu, bytes.fromhex("22 F1 90"))
        print(f"UDS 22 F1 90 response: {vin_response.hex().upper()}")
        if not vin_response.startswith(bytes.fromhex("62 F1 90")):
            raise RuntimeError("ReadDataByIdentifier did not return positive response 62 F1 90")
        print("PASS UDS ReadDataByIdentifier 22 F1 90")

        clear_response = send_uds(sock, tester, ecu, bytes.fromhex("14 FF FF FF"))
        print(f"UDS 14 FF FF FF response: {clear_response.hex().upper()}")
        if clear_response != b"\x54":
            raise RuntimeError(f"expected exactly 54, received {clear_response.hex().upper()}")
        print("PASS UDS ClearDiagnosticInformation 14 FF FF FF -> 54")

    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as exc:
        print(f"FAIL independent DoIP client: {exc}", file=sys.stderr)
        raise SystemExit(1)
