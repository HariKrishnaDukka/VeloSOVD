#!/usr/bin/env bash
#
# VeloSOVD local Linux verification harness.
#
# Creates the isolated SOVD network, verifies namespace/veth/vCAN connectivity,
# runs the Rust workspace checks, prints a pass/fail summary, and tears down
# only the network created by this invocation.
#
set -Eeuo pipefail

readonly SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly REPO_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
readonly NETWORK_SCRIPT="${REPO_ROOT}/tools/setup_sovd_network.sh"
readonly OWNER_MARKER="/run/sovd-demo-network/owned-by-setup_sovd_network"

readonly CENTRAL_NS="sdv-central-ns"
readonly FRONT_NS="sdv-front-ns"
readonly REAR_NS="sdv-rear-ns"

PASS_COUNT=0
FAIL_COUNT=0
NETWORK_CREATED=0

log() {
    printf '[local-checks] %s\n' "$*"
}

record_pass() {
    PASS_COUNT=$((PASS_COUNT + 1))
    printf '[PASS] %s\n' "$1"
}

record_fail() {
    FAIL_COUNT=$((FAIL_COUNT + 1))
    printf '[FAIL] %s\n' "$1" >&2
}

run_check() {
    local description="$1"
    shift

    log "Running: ${description}"
    if "$@"; then
        record_pass "$description"
    else
        local result=$?
        record_fail "${description} (exit ${result})"
    fi
}

check_ping() {
    local namespace="$1"
    local destination="$2"

    sudo ip netns exec "$namespace" ping -c 1 -W 2 "$destination"
}

check_vcan_up() {
    local namespace="$1"
    local interface="$2"

    sudo ip -n "$namespace" -o link show dev "$interface" |
        grep -Eq '<[^>]*UP[^>]*>'
}

finish() {
    local original_status=$?

    trap - EXIT INT TERM

    if [[ "$NETWORK_CREATED" -eq 1 ]]; then
        log "Tearing down the SOVD network created by this run"
        if sudo bash "$NETWORK_SCRIPT" down; then
            record_pass "network teardown"
            NETWORK_CREATED=0
        else
            record_fail "network teardown"
            original_status=1
        fi
    fi

    printf '\n========== LOCAL CHECK SUMMARY ==========\n'
    printf 'Passed: %d\n' "$PASS_COUNT"
    printf 'Failed: %d\n' "$FAIL_COUNT"
    printf 'Total:  %d\n' "$((PASS_COUNT + FAIL_COUNT))"

    if [[ "$FAIL_COUNT" -gt 0 ]]; then
        exit 1
    fi

    exit "$original_status"
}

trap finish EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

if [[ "$(uname -s)" != "Linux" ]]; then
    record_fail "Linux host requirement"
    exit 2
fi

for required_tool in cargo ip sudo ping modprobe; do
    if ! command -v "$required_tool" >/dev/null 2>&1; then
        record_fail "required command: $required_tool"
        exit 2
    fi
done
record_pass "Linux and required command preflight"

if ! sudo -v; then
    record_fail "sudo authorization"
    exit 2
fi
record_pass "sudo authorization"

if sudo test -e "$OWNER_MARKER"; then
    record_fail "existing SOVD network ownership marker (refusing to adopt it)"
    exit 2
fi

cd "$REPO_ROOT"

log "Creating isolated namespaces and virtual links"
if sudo bash "$NETWORK_SCRIPT" up; then
    NETWORK_CREATED=1
    record_pass "network setup"
else
    record_fail "network setup"
    exit 1
fi

# Verify both point-to-point links in both directions.
run_check "central-to-front namespace connectivity" \
    check_ping "$CENTRAL_NS" "10.77.10.2"
run_check "front-to-central namespace connectivity" \
    check_ping "$FRONT_NS" "10.77.10.1"
run_check "central-to-rear namespace connectivity" \
    check_ping "$CENTRAL_NS" "10.77.10.6"
run_check "rear-to-central namespace connectivity" \
    check_ping "$REAR_NS" "10.77.10.5"

# Confirm both virtual CAN interfaces exist and are administratively UP.
run_check "front vCAN interface is UP" \
    check_vcan_up "$FRONT_NS" "vcan-front"
run_check "rear vCAN interface is UP" \
    check_vcan_up "$REAR_NS" "vcan-rear"

# Run the same repository-wide Rust checks used by CI.
run_check "Rust formatting" cargo fmt --all -- --check
run_check "Rust Clippy" \
    cargo clippy --workspace --all-targets -- -D warnings
run_check "Rust workspace tests" cargo test --workspace

log "All checks completed; cleanup will run automatically"
