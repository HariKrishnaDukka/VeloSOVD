#!/usr/bin/env bash
# Full local Linux end-to-end verification for the virtual SOVD stack.
set -Eeuo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "$0")" && pwd)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/.." && pwd)"
NETWORK_SCRIPT="$SCRIPT_DIR/setup_sovd_network.sh"
TOPOLOGY="$REPO_ROOT/platform/config/vehicle-topology.toml"
VIRTUAL_ECU_BIN="$REPO_ROOT/target/debug/virtual-ecu"
CENTRAL_BIN="$REPO_ROOT/target/debug/sovd-server"
OWNER_MARKER="/run/sovd-demo-network/owned-by-setup_sovd_network"
CENTRAL_NS="sdv-central-ns"
FRONT_NS="sdv-front-ns"
REAR_NS="sdv-rear-ns"
CENTRAL_URL="https://10.77.10.1:8010"
FRONT_IP="10.77.10.2"
REAR_IP="10.77.10.6"

# Development PKI identities. Private keys remain outside Git.
PKI_ROOT="$REPO_ROOT/pki/dev"
SERVER_CA_CERT="$PKI_ROOT/server-ca/ca.crt"
CLIENT_CA_CERT="$PKI_ROOT/client-ca/ca.crt"
HARNESS_CLIENT_CERT="$PKI_ROOT/harness/harness-client.crt"
HARNESS_CLIENT_KEY="$PKI_ROOT/harness/harness-client.key"
UNTRUSTED_CLIENT_CERT="$PKI_ROOT/untrusted/untrusted-client.crt"
UNTRUSTED_CLIENT_KEY="$PKI_ROOT/untrusted/untrusted-client.key"
EXPIRED_CLIENT_CERT="$PKI_ROOT/harness/expired-client.crt"
EXPIRED_CLIENT_KEY="$PKI_ROOT/harness/expired-client.key"

CENTRAL_SERVER_CERT="$PKI_ROOT/central/central-server.crt"
CENTRAL_SERVER_KEY="$PKI_ROOT/central/central-server.key"
CENTRAL_CLIENT_CERT="$PKI_ROOT/central/central-client.crt"
CENTRAL_CLIENT_KEY="$PKI_ROOT/central/central-client.key"
FRONT_SERVER_CERT="$PKI_ROOT/zones/front/front-server.crt"
FRONT_SERVER_KEY="$PKI_ROOT/zones/front/front-server.key"
REAR_SERVER_CERT="$PKI_ROOT/zones/rear/rear-server.crt"
REAR_SERVER_KEY="$PKI_ROOT/zones/rear/rear-server.key"
RUN_DIR="$(mktemp -d /tmp/velosovd-local-checks.XXXXXX)"
RUN_LOG="$RUN_DIR/run.log"
FRONT_PID_FILE="$RUN_DIR/front.pid"
REAR_PID_FILE="$RUN_DIR/rear.pid"
CENTRAL_PID_FILE="$RUN_DIR/central.pid"

PASS_COUNT=0
FAIL_COUNT=0
SKIP_COUNT=0
NETWORK_ATTEMPTED=0
FRONT_STARTED=0
REAR_STARTED=0
CENTRAL_STARTED=0

log() { printf '[local-checks] %s\n' "$*" | tee -a "$RUN_LOG"; }
record_pass() { PASS_COUNT=$((PASS_COUNT + 1)); printf '[PASS] %s\n' "$1" | tee -a "$RUN_LOG"; }
record_fail() { FAIL_COUNT=$((FAIL_COUNT + 1)); printf '[FAIL] %s\n' "$1" | tee -a "$RUN_LOG" >&2; }
record_skip() { SKIP_COUNT=$((SKIP_COUNT + 1)); printf '[SKIP] %s\n' "$1" | tee -a "$RUN_LOG"; }

run_check() {
    description="$1"
    shift
    output="$RUN_DIR/check-output"
    log "Running: $description"
    if "$@" >"$output" 2>&1; then
        cat "$output" | tee -a "$RUN_LOG"
        record_pass "$description"
        return 0
    else
        rc=$?
        cat "$output" | tee -a "$RUN_LOG"
        record_fail "$description (exit $rc)"
        return 1
    fi
}

need_command() {
    command -v "$1" >/dev/null 2>&1 || {
        printf 'Missing required command: %s\n' "$1" >&2
        return 1
    }
}

check_ping() { sudo ip netns exec "$1" ping -c 1 -W 2 "$2"; }
check_vcan_up() {
    sudo ip -n "$1" -o link show dev "$2" | grep -Eq '<[^>]*UP[^>]*>'
}

start_service() {
    name="$1"
    namespace="$2"
    logfile="$3"
    pidfile="$4"
    shift 4
    if [[ -e "$pidfile" ]]; then
        printf '%s has a stale PID file: %s\n' "$name" "$pidfile" >&2
        return 1
    fi
    log "Starting $name in $namespace; log=$logfile"
    sudo sh -c 'pidfile=$1; ns=$2; shift 2; printf "%s\n" "$$" > "$pidfile"; exec ip netns exec "$ns" env "$@"' \
        sh "$pidfile" "$namespace" "$@" >"$logfile" 2>&1 &
    launcher_pid=$!
    for attempt in $(seq 1 50); do
        if sudo test -s "$pidfile"; then
            service_pid="$(sudo cat "$pidfile")"
            if sudo kill -0 "$service_pid" 2>/dev/null; then
                case "$name" in
                    FRONT) FRONT_STARTED=1 ;;
                    REAR) REAR_STARTED=1 ;;
                    CENTRAL) CENTRAL_STARTED=1 ;;
                esac
                printf '%s service PID=%s (launcher PID=%s)\n' "$name" "$service_pid" "$launcher_pid"
                return 0
            fi
        fi
        if ! kill -0 "$launcher_pid" 2>/dev/null; then break; fi
        sleep 0.1
    done
    printf '%s did not remain running. Recent service log:\n' "$name" >&2
    tail -n 60 "$logfile" 2>/dev/null || true
    return 1
}

stop_front_for_negative() {
    if ! sudo test -s "$FRONT_PID_FILE"; then
        printf 'FRONT PID file is missing; cannot establish a controlled outage\n' >&2
        return 1
    fi
    service_pid="$(sudo cat "$FRONT_PID_FILE")"
    if ! sudo kill -0 "$service_pid" 2>/dev/null; then
        printf 'FRONT process was already stopped; negative case is not controlled\n' >&2
        return 1
    fi
    stop_service FRONT "$FRONT_PID_FILE"
}

stop_service() {
    name="$1"
    pidfile="$2"
    if ! sudo test -s "$pidfile"; then
        printf '%s: no owned PID file; no process to stop\n' "$name"
        return 0
    fi
    service_pid="$(sudo cat "$pidfile")"
    if sudo kill -0 "$service_pid" 2>/dev/null; then
        sudo kill -TERM "$service_pid" || return 1
        stopped=0
        for attempt in $(seq 1 30); do
            if ! sudo kill -0 "$service_pid" 2>/dev/null; then
                stopped=1
                break
            fi
            process_state="$(sudo ps -o stat= -p "$service_pid" 2>/dev/null | awk '{print $1}')"
            if [[ "$process_state" == Z* ]]; then
                stopped=1
                break
            fi
            sleep 0.1
        done
        if [[ "$stopped" -eq 0 ]]; then
            sudo kill -KILL "$service_pid" || return 1
            sleep 0.2
            process_state="$(sudo ps -o stat= -p "$service_pid" 2>/dev/null | awk '{print $1}')"
            if sudo kill -0 "$service_pid" 2>/dev/null && [[ "$process_state" != Z* ]]; then
                printf '%s process %s is still alive after SIGKILL\n' "$name" "$service_pid" >&2
                return 1
            fi
        fi
    fi
    sudo rm -f "$pidfile" || return 1
    printf '%s service stopped (PID %s)\n' "$name" "$service_pid"
    case "$name" in
        FRONT) FRONT_STARTED=0 ;;
        REAR) REAR_STARTED=0 ;;
        CENTRAL) CENTRAL_STARTED=0 ;;
    esac
}

# All SOVD HTTPS requests authenticate the server and present a
# dedicated harness client certificate.
curl_mtls() {
    namespace="$1"
    shift
    sudo ip netns exec "$namespace" curl \
        --cacert "$SERVER_CA_CERT" \
        --cert "$HARNESS_CLIENT_CERT" \
        --key "$HARNESS_CLIENT_KEY" \
        "$@"
}

wait_https() {
    namespace="$1"
    url="$2"
    for attempt in $(seq 1 60); do
        if curl_mtls "$namespace" -fsS --max-time 2 "$url" >/dev/null; then
            curl_mtls "$namespace" -fsS --max-time 2 "$url"
            return 0
        fi
        sleep 0.25
    done
    printf 'HTTPS/mTLS endpoint did not become ready: %s\n' "$url" >&2
    return 1
}

http_json() {
    curl_mtls "$CENTRAL_NS" -fsS --max-time 8 "$1"
}

# A TLS rejection test must fail at the transport/handshake layer.
# Do not use curl --fail here: an HTTP error response would otherwise
# be mistaken for successful TLS rejection.
expect_tls_rejection() {
    local description="$1"
    shift

    local test_name
    local output_file
    local curl_rc=0

    test_name="${description//[^[:alnum:]_-]/_}"
    output_file="$RUN_DIR/tls-rejection-${test_name}.log"

    if sudo ip netns exec "$CENTRAL_NS" curl \
        --verbose --no-progress-meter --output /dev/null --max-time 5 \
        --cacert "$SERVER_CA_CERT" "$@" 2>"$output_file"; then
        printf 'FAIL: TLS request unexpectedly succeeded: %s\n' "$description" >&2
        return 1
    else
        curl_rc=$?
    fi

    case "$curl_rc" in
        35|56|60)
            ;;
        *)
            printf 'FAIL: Unexpected curl failure for %s (exit %s)\n' \
                "$description" "$curl_rc" >&2
            cat "$output_file" >&2
            return 1
            ;;
    esac

    # Require evidence of a TLS alert or certificate validation failure.
    # A generic connection reset or receive error is insufficient.
    if grep -Eiq \
        'TLS alert|alert.*(certificate|required|unknown ca|bad certificate|protocol version)|certificate.*(expired|verify|problem)|SSL certificate problem' \
        "$output_file"; then
        printf 'PASS: TLS rejection confirmed: %s (curl exit %s)\n' \
            "$description" "$curl_rc"
    else
        printf 'FAIL: No explicit TLS/certificate rejection evidence for %s (curl exit %s)\n' \
            "$description" "$curl_rc" >&2
        cat "$output_file" >&2
        return 1
    fi
}
check_missing_client_certificate() {
    expect_tls_rejection "missing client certificate" \
        "$CENTRAL_URL/api/health"
}

check_untrusted_client_certificate() {
    expect_tls_rejection "untrusted client CA" \
        --http1.1 \
        --cert "$UNTRUSTED_CLIENT_CERT" \
        --key "$UNTRUSTED_CLIENT_KEY" \
        "$CENTRAL_URL/api/health"
}

check_expired_client_certificate() {
    expect_tls_rejection "expired client certificate" \
        --cert "$EXPIRED_CLIENT_CERT" \
        --key "$EXPIRED_CLIENT_KEY" \
        "$CENTRAL_URL/api/health"
}

check_wrong_server_identity() {
    expect_tls_rejection "server IP does not match certificate SAN" \
        --cert "$HARNESS_CLIENT_CERT" \
        --key "$HARNESS_CLIENT_KEY" \
        "https://10.77.10.5:8010/api/health"
}

check_tls12_rejected() {
    expect_tls_rejection "TLS 1.2 is disabled" \
        --cert "$HARNESS_CLIENT_CERT" \
        --key "$HARNESS_CLIENT_KEY" \
        --tls-max 1.2 \
        "$CENTRAL_URL/api/health"
}

check_seeded_dtc() {
    body="$(http_json "$CENTRAL_URL/api/ecus/ECM/diagnostics/faults")"
    printf '%s\n' "$body"
    printf '%s' "$body" | python3 -c '
import json,sys
faults=json.load(sys.stdin)
assert any(x.get("code")=="P0128" for x in faults), "boot-seeded ECM DTC P0128 is absent"
print("Verified boot-seeded DTC P0128 through the fault-listing route")
'
}

check_faults_empty() {
    body="$(http_json "$CENTRAL_URL/api/ecus/ECM/diagnostics/faults")"
    printf '%s\n' "$body"
    printf '%s' "$body" | python3 -c '
import json,sys
faults=json.load(sys.stdin)
assert faults==[], "expected an empty ECM fault list, got "+repr(faults)
print("Verified ECM fault list is empty")
'
}

submit_sovd_execution() {
    expected="$1"
    response="$(curl_mtls "$CENTRAL_NS" -fsS --max-time 8 \
        -X POST "$CENTRAL_URL/sovd/v1/components/ECM/operations/clear-faults/executions" \
        -H 'Content-Type: application/json' -d '{}')"
    printf 'SOVD start response:\n%s\n' "$response"
    execution_id="$(printf '%s' "$response" | python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])')"
    printf 'Execution ID: %s\n' "$execution_id"
    history_body="$(http_json "$CENTRAL_URL/sovd/v1/components/ECM/operations/clear-faults/executions")"
    printf 'SOVD execution history:\n%s\n' "$history_body"
    printf '%s' "$history_body" | python3 -c '
import json,sys
expected=sys.argv[1]
items=json.load(sys.stdin).get("items",[])
assert expected in items, f"execution {expected} was not present in history {items!r}"
print(f"Verified execution {expected} is present in SOVD execution history")
' "$execution_id"
    status=""
    for attempt in $(seq 1 60); do
        status_body="$(http_json "$CENTRAL_URL/sovd/v1/components/ECM/operations/clear-faults/executions/$execution_id")"
        printf 'Execution status response:\n%s\n' "$status_body"
        status="$(printf '%s' "$status_body" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("status",""))')"
        if [[ "$status" == "completed" || "$status" == "failed" ]]; then break; fi
        sleep 0.25
    done
    if [[ "$status" != "$expected" ]]; then
        printf 'Expected execution status=%s, received=%s\n' "$expected" "$status" >&2
        return 1
    fi
    printf 'Verified SOVD execution %s reached status=%s\n' "$execution_id" "$status"
}

check_tcp_port() {
    sudo ip netns exec "$CENTRAL_NS" python3 - "$1" "$2" <<'PY'
import socket,sys
host,port=sys.argv[1],int(sys.argv[2])
with socket.create_connection((host,port),timeout=2):
    print(f"TCP connection established to {host}:{port}")
PY
}

check_isotp_multiframe() {
    rx_file="$RUN_DIR/isotp-response.bin"
    rx_log="$RUN_DIR/isotprecv.log"
    request_hex="2EF1874142434445464748"
    sudo ip netns exec "$FRONT_NS" timeout 5 \
        isotprecv -s 0x620 -d 0x628 vcan-front >"$rx_file" 2>"$rx_log" &
    receiver_pid=$!
    sleep 0.4
    printf '2E F1 87 41 42 43 44 45 46 47 48\n' |
        sudo ip netns exec "$FRONT_NS" timeout 5 isotpsend -s 0x620 -d 0x628 vcan-front
    receiver_rc=0
    wait "$receiver_pid" || receiver_rc=$?
    printf 'isotprecv exit=%s (0 means one response PDU was captured; 124 means receive timeout)\n' "$receiver_rc"
    printf 'ISO-TP request: %s (%s bytes)\n' "$request_hex" "$(expr length "$request_hex" / 2)"
    printf 'isotprecv response (hex): '
    cat "$rx_file"
    printf '\n'
    cat "$rx_log" || true
    [[ -s "$rx_file" ]] || { printf 'No ISO-TP response captured\n' >&2; return 1; }
    response_hex="$(tr -d '[:space:]' <"$rx_file" | tr '[:lower:]' '[:upper:]')"
    [[ "$response_hex" == "7F2E11" ]] || {
        printf 'Expected UDS response 7F2E11, got %s\n' "$response_hex" >&2
        return 1
    }
    printf 'Verified multi-frame request was reassembled; ECU returned %s\n' "$response_hex"
}

finish() {
    original_status=$?
    trap - EXIT INT TERM
    set +e
    printf '\n[local-checks] Cleanup started\n' | tee -a "$RUN_LOG"
    for service_log in "$RUN_DIR/front.log" "$RUN_DIR/rear.log" "$RUN_DIR/central.log"; do
        if [[ -f "$service_log" ]]; then
            printf '\n========== FINAL SERVICE OUTPUT: %s ==========\n' "$service_log" | tee -a "$RUN_LOG"
            tail -n 60 "$service_log" | tee -a "$RUN_LOG"
        fi
    done

    if [[ -s "$FRONT_PID_FILE" ]]; then
        if stop_service FRONT "$FRONT_PID_FILE" >"$RUN_DIR/cleanup-front" 2>&1; then
            cat "$RUN_DIR/cleanup-front" | tee -a "$RUN_LOG"
            record_pass "stop FRONT service"
        else
            cat "$RUN_DIR/cleanup-front" | tee -a "$RUN_LOG"
            record_fail "stop FRONT service"
        fi
    fi
    if [[ -s "$REAR_PID_FILE" ]]; then
        if stop_service REAR "$REAR_PID_FILE" >"$RUN_DIR/cleanup-rear" 2>&1; then
            cat "$RUN_DIR/cleanup-rear" | tee -a "$RUN_LOG"
            record_pass "stop REAR service"
        else
            cat "$RUN_DIR/cleanup-rear" | tee -a "$RUN_LOG"
            record_fail "stop REAR service"
        fi
    fi
    if [[ -s "$CENTRAL_PID_FILE" ]]; then
        if stop_service CENTRAL "$CENTRAL_PID_FILE" >"$RUN_DIR/cleanup-central" 2>&1; then
            cat "$RUN_DIR/cleanup-central" | tee -a "$RUN_LOG"
            record_pass "stop central service"
        else
            cat "$RUN_DIR/cleanup-central" | tee -a "$RUN_LOG"
            record_fail "stop central service"
        fi
    fi

    if [[ "$NETWORK_ATTEMPTED" -eq 1 ]]; then
        if sudo test -e "$OWNER_MARKER"; then
            if sudo bash "$NETWORK_SCRIPT" down >"$RUN_DIR/network-down" 2>&1; then
                cat "$RUN_DIR/network-down" | tee -a "$RUN_LOG"
                record_pass "tear down owned namespaces, veth links and vCAN interfaces"
            else
                cat "$RUN_DIR/network-down" | tee -a "$RUN_LOG"
                record_fail "tear down owned namespaces, veth links and vCAN interfaces"
            fi
        else
            leftovers=0
            for ns in "$CENTRAL_NS" "$FRONT_NS" "$REAR_NS"; do
                if sudo ip netns list | awk '{print $1}' | grep -Fxq "$ns"; then
                    printf 'Namespace remains without ownership marker: %s\n' "$ns" | tee -a "$RUN_LOG"
                    leftovers=1
                fi
            done
            if [[ "$leftovers" -eq 0 ]]; then
                record_pass "verify setup rollback left no owned namespaces"
            else
                record_fail "network cleanup: namespace remains without ownership marker"
            fi
        fi
    else
        record_skip "network teardown (setup was not attempted)"
    fi

    printf '\n========== LOCAL END-TO-END SUMMARY ==========\n' | tee -a "$RUN_LOG"
    printf 'Passed:  %s\nFailed:  %s\nSkipped: %s\nTotal:   %s\nLog:     %s\n' \
        "$PASS_COUNT" "$FAIL_COUNT" "$SKIP_COUNT" \
        "$((PASS_COUNT + FAIL_COUNT + SKIP_COUNT))" "$RUN_LOG" | tee -a "$RUN_LOG"
    if [[ "$FAIL_COUNT" -gt 0 || "$original_status" -ne 0 ]]; then exit 1; fi
    exit 0
}

trap finish EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
: >"$RUN_LOG"
log "Repository: $REPO_ROOT"
log "Run artifacts: $RUN_DIR"

if [[ "$(uname -s)" != "Linux" ]]; then
    record_fail "Linux host requirement"
    record_skip "all build, namespace, service, SOVD, DoIP, ISO-TP and Rust checks (Linux is required)"
    exit 2
fi
missing=0
for tool in cargo ip sudo ping modprobe python3 curl timeout xxd isotpsend isotprecv ps openssl; do
    if ! need_command "$tool"; then missing=1; fi
done
if [[ "$missing" -ne 0 ]]; then
    record_fail "required Linux, Rust, Python, curl and can-utils commands"
    record_skip "Bash/Python syntax checks and all runtime integration checks (preflight failed)"
    record_skip "Rust formatting, Clippy and workspace tests (preflight failed)"
    exit 2
fi
record_pass "Linux and required command preflight (including isotpsend/isotprecv and openssl)"

run_check "Generate and verify isolated development PKI" \
    bash "$SCRIPT_DIR/security/generate_dev_pki.sh" || exit 1

run_check "Bash syntax for local-check harness" bash -n "$SCRIPT_DIR/run_local_checks.sh" || exit 1
run_check "Python syntax for independent DoIP client" python3 -c 'import ast,pathlib,sys; ast.parse(pathlib.Path(sys.argv[1]).read_text()); print("Python AST parse passed")' "$SCRIPT_DIR/doip_independent_client.py" || exit 1
if ! sudo -v; then
    record_fail "sudo authorization"
    record_skip "namespace, service, SOVD, DoIP, ISO-TP and Rust checks (sudo authorization failed)"
    exit 2
fi
record_pass "sudo authorization"
if ! run_check "load Linux CAN ISO-TP kernel protocol (can-isotp)" sudo modprobe can-isotp; then
    record_skip "namespace, service, SOVD, DoIP and ISO-TP checks (CAN ISO-TP kernel support unavailable)"
    record_skip "Rust formatting, Clippy and workspace tests (preflight stopped safely)"
    exit 2
fi
if sudo test -e "$OWNER_MARKER"; then
    record_fail "existing network ownership marker (refusing to adopt it)"
    record_skip "network, service, SOVD, DoIP and ISO-TP checks (existing owned network)"
    record_skip "Rust formatting, Clippy and workspace tests (preflight stopped safely)"
    exit 2
fi
for ns in "$CENTRAL_NS" "$FRONT_NS" "$REAR_NS"; do
    if sudo ip netns list | awk '{print $1}' | grep -Fxq "$ns"; then
        record_fail "pre-existing namespace $ns (refusing to adopt it)"
        record_skip "network, service, SOVD, DoIP and ISO-TP checks (namespace collision)"
        record_skip "Rust formatting, Clippy and workspace tests (preflight stopped safely)"
        exit 2
    fi
done

cd "$REPO_ROOT"
if ! run_check "build central and virtual ECU services" cargo build --locked -p sovd-server -p virtual-ecu; then
    record_skip "namespace, service, SOVD, DoIP and ISO-TP checks (build prerequisite failed)"
    record_skip "Rust formatting, Clippy and workspace tests (build prerequisite failed)"
    exit 1
fi

NETWORK_ATTEMPTED=1
if sudo bash "$NETWORK_SCRIPT" up; then
    record_pass "create central, FRONT and REAR namespaces, veth links and vCAN interfaces"
else
    record_fail "create central, FRONT and REAR namespaces, veth links and vCAN interfaces"
    exit 1
fi

run_check "central-to-FRONT namespace connectivity" check_ping "$CENTRAL_NS" "$FRONT_IP" || true
run_check "FRONT-to-central namespace connectivity" check_ping "$FRONT_NS" 10.77.10.1 || true
run_check "central-to-REAR namespace connectivity" check_ping "$CENTRAL_NS" "$REAR_IP" || true
run_check "REAR-to-central namespace connectivity" check_ping "$REAR_NS" 10.77.10.5 || true
run_check "FRONT vcan-front interface is UP" check_vcan_up "$FRONT_NS" vcan-front || true
run_check "REAR vcan-rear interface is UP" check_vcan_up "$REAR_NS" vcan-rear || true

run_check "start FRONT virtual ECU service" start_service FRONT "$FRONT_NS" "$RUN_DIR/front.log" "$FRONT_PID_FILE" \
    "SDV_ZONE=FRONT" "SDV_TOPOLOGY=$TOPOLOGY" \
    "SDV_TLS_CLIENT_CA_CERT=$CLIENT_CA_CERT" \
    "SDV_TLS_CERT=$FRONT_SERVER_CERT" "SDV_TLS_KEY=$FRONT_SERVER_KEY" \
    "$VIRTUAL_ECU_BIN" || true

run_check "start REAR virtual ECU service" start_service REAR "$REAR_NS" "$RUN_DIR/rear.log" "$REAR_PID_FILE" \
    "SDV_ZONE=REAR" "SDV_TOPOLOGY=$TOPOLOGY" \
    "SDV_TLS_CLIENT_CA_CERT=$CLIENT_CA_CERT" \
    "SDV_TLS_CERT=$REAR_SERVER_CERT" "SDV_TLS_KEY=$REAR_SERVER_KEY" \
    "$VIRTUAL_ECU_BIN" || true

run_check "start central SOVD service" start_service CENTRAL "$CENTRAL_NS" "$RUN_DIR/central.log" "$CENTRAL_PID_FILE" \
    "SDV_TOPOLOGY=$TOPOLOGY" \
    "SDV_TLS_CLIENT_CA_CERT=$CLIENT_CA_CERT" \
    "SDV_TLS_SERVER_CERT=$CENTRAL_SERVER_CERT" \
    "SDV_TLS_SERVER_KEY=$CENTRAL_SERVER_KEY" \
    "SDV_TLS_SERVER_CA_CERT=$SERVER_CA_CERT" \
    "SDV_TLS_CLIENT_CERT=$CENTRAL_CLIENT_CERT" \
    "SDV_TLS_CLIENT_KEY=$CENTRAL_CLIENT_KEY" \
    "$CENTRAL_BIN" || true

for service_log in "$RUN_DIR/front.log" "$RUN_DIR/rear.log" "$RUN_DIR/central.log"; do
    if [[ -f "$service_log" ]]; then
        log "Actual service output: $service_log"
        tail -n 30 "$service_log" | tee -a "$RUN_LOG"
    fi
done

SERVICES_READY=1
if [[ "$FRONT_STARTED" -ne 1 || "$REAR_STARTED" -ne 1 || "$CENTRAL_STARTED" -ne 1 ]]; then SERVICES_READY=0; fi
if [[ "$SERVICES_READY" -eq 1 ]]; then
    run_check "FRONT zonal HTTPS/mTLS service ready" wait_https "$CENTRAL_NS" "https://$FRONT_IP:9001/ecus" || SERVICES_READY=0
    run_check "REAR zonal HTTPS/mTLS service ready" wait_https "$CENTRAL_NS" "https://$REAR_IP:9001/ecus" || SERVICES_READY=0
    run_check "central SOVD HTTPS/mTLS service ready" wait_https "$CENTRAL_NS" "$CENTRAL_URL/api/health" || SERVICES_READY=0
    run_check "FRONT DoIP TCP listener available" check_tcp_port "$FRONT_IP" 13400 || SERVICES_READY=0
else
    record_skip "service readiness checks (a service failed to start)"
fi

if [[ "$SERVICES_READY" -eq 1 ]]; then
    run_check "TLS rejects clients without certificates" check_missing_client_certificate || true
    run_check "TLS rejects clients signed by an untrusted CA" check_untrusted_client_certificate || true
    run_check "TLS rejects expired client certificates" check_expired_client_certificate || true
    run_check "TLS rejects incorrect server identity" check_wrong_server_identity || true
    run_check "TLS 1.2 negotiation is rejected" check_tls12_rejected || true

    run_check "boot-seeded ECM DTC appears in fault-listing route" check_seeded_dtc || true
    run_check "SOVD clear-faults execution reaches completed" submit_sovd_execution completed || true
    run_check "ECM DTC is gone after successful SOVD clear" check_faults_empty || true

    if run_check "stop FRONT service for controlled negative case" stop_front_for_negative; then
        run_check "failed SOVD clear is recorded with status=failed" submit_sovd_execution failed || true
    else
        record_skip "negative SOVD execution (FRONT service could not be stopped)"
    fi
    run_check "restart FRONT service after negative case" start_service FRONT "$FRONT_NS" "$RUN_DIR/front.log" "$FRONT_PID_FILE" \
        "SDV_ZONE=FRONT" "SDV_TOPOLOGY=$TOPOLOGY" \
        "SDV_TLS_CLIENT_CA_CERT=$CLIENT_CA_CERT" \
        "SDV_TLS_CERT=$FRONT_SERVER_CERT" "SDV_TLS_KEY=$FRONT_SERVER_KEY" \
        "$VIRTUAL_ECU_BIN" || SERVICES_READY=0
    if [[ "$SERVICES_READY" -eq 1 ]]; then
        run_check "FRONT HTTPS/mTLS service recovers" wait_https "$CENTRAL_NS" "https://$FRONT_IP:9001/ecus" || SERVICES_READY=0
    fi
else
    record_skip "DTC listing, SOVD success and negative-case checks (services not ready)"
    record_skip "FRONT restart and recovery checks (services not ready)"
fi

if [[ "$SERVICES_READY" -eq 1 ]]; then
    run_check "independent DoIP routing activation, 22 F1 90 and 14 FF FF FF -> 54" \
        sudo ip netns exec "$CENTRAL_NS" python3 "$SCRIPT_DIR/doip_independent_client.py" "$FRONT_IP" 13400 0x0E00 0x1110 || true
    run_check "multi-frame ISO-TP request using isotpsend/isotprecv on vcan-front" check_isotp_multiframe || true
else
    record_skip "independent DoIP client check (FRONT service unavailable)"
    record_skip "multi-frame ISO-TP check (FRONT service unavailable)"
fi

run_check "Rust formatting" cargo fmt --all -- --check || true
run_check "Rust Clippy" cargo clippy --locked --workspace --all-targets -- -D warnings || true
run_check "Rust workspace tests" cargo test --locked --workspace || true
log "All checks attempted; EXIT cleanup will stop owned services and remove the owned network"
