#!/usr/bin/env bash
#
# SOVD demo network lifecycle
#
# Creates only the resources declared by platform/config/vehicle-topology.toml:
#   sdv-central-ns <-> sdv-front-ns : 10.77.10.0/30
#   sdv-central-ns <-> sdv-rear-ns  : 10.77.10.4/30
#   sdv-front-ns: vcan-front
#   sdv-rear-ns : vcan-rear
#
# It does not touch similarly named resources belonging to other projects.
#
set -Eeuo pipefail

readonly CENTRAL_NS="sdv-central-ns"
readonly FRONT_NS="sdv-front-ns"
readonly REAR_NS="sdv-rear-ns"

# Linux interface names are limited to 15 characters.
readonly CENTRAL_FRONT_IF="sdv-cf-host"
readonly FRONT_CENTRAL_IF="sdv-cf-zcu"
readonly CENTRAL_REAR_IF="sdv-cr-host"
readonly REAR_CENTRAL_IF="sdv-cr-zcu"

readonly FRONT_CAN_IF="vcan-front"
readonly REAR_CAN_IF="vcan-rear"

readonly STATE_DIR="/run/sovd-demo-network"
readonly OWNER_MARKER="${STATE_DIR}/owned-by-setup_sovd_network"

created_namespaces=()
created_host_links=()
created_can_links=()
success=0

log() { printf '[sovd-network] %s\n' "$*"; }
die() { printf '[sovd-network] ERROR: %s\n' "$*" >&2; exit 1; }

require_root() {
    [[ "${EUID}" -eq 0 ]] || die "Run with sudo: sudo $0 {up|status|down}"
    command -v ip >/dev/null 2>&1 || die "'ip' command not found (install iproute2)"
}

namespace_exists() {
    ip netns list | awk '{print $1}' | grep -Fxq "$1"
}

link_exists_host() {
    ip link show dev "$1" >/dev/null 2>&1
}

link_exists_ns() {
    ip -n "$1" link show dev "$2" >/dev/null 2>&1
}

rollback_created_resources() {
    local item
    log "Setup failed; rolling back resources created by this invocation"
    for item in "${created_can_links[@]}"; do
        local ns="${item%%:*}" iface="${item#*:}"
        ip -n "$ns" link delete "$iface" 2>/dev/null || true
    done
    for item in "${created_host_links[@]}"; do
        ip link delete "$item" 2>/dev/null || true
    done
    for item in "${created_namespaces[@]}"; do
        ip netns delete "$item" 2>/dev/null || true
    done
}

on_exit() {
    local rc=$?
    if [[ "$success" -ne 1 ]]; then
        rollback_created_resources
    fi
    exit "$rc"
}

verify_no_conflicts() {
    local ns iface
    for ns in "$CENTRAL_NS" "$FRONT_NS" "$REAR_NS"; do
        if namespace_exists "$ns"; then
            die "Namespace '$ns' already exists. Refusing to adopt or modify it."
        fi
    done
    for iface in "$CENTRAL_FRONT_IF" "$FRONT_CENTRAL_IF" \
                 "$CENTRAL_REAR_IF" "$REAR_CENTRAL_IF"; do
        if link_exists_host "$iface"; then
            die "Host interface '$iface' already exists. Refusing to modify it."
        fi
    done
}

create_namespace() {
    local ns="$1"
    ip netns add "$ns"
    created_namespaces+=("$ns")
    ip -n "$ns" link set lo up
}

create_veth_pair() {
    local central_if="$1" central_ns="$2"
    local zcu_ns="$3" zcu_if="$4"
    local central_addr="$5" zcu_addr="$6"

    ip link add "$central_if" type veth peer name "$zcu_if"
    ip link set "$central_if" netns "$central_ns"
    ip link set "$zcu_if" netns "$zcu_ns"

    ip -n "$central_ns" addr add "$central_addr" dev "$central_if"
    ip -n "$central_ns" link set "$central_if" up

    ip -n "$zcu_ns" addr add "$zcu_addr" dev "$zcu_if"
    ip -n "$zcu_ns" link set "$zcu_if" up
}

create_vcan() {
    local ns="$1" iface="$2"
    ip -n "$ns" link add "$iface" type vcan
    created_can_links+=("${ns}:${iface}")
    ip -n "$ns" link set "$iface" up
}

do_up() {
    verify_no_conflicts
    trap on_exit EXIT

    modprobe vcan || die "Could not load vcan kernel module"

    create_namespace "$CENTRAL_NS"
    create_namespace "$FRONT_NS"
    create_namespace "$REAR_NS"

    # Central <-> Front: 10.77.10.0/30
    create_veth_pair "$CENTRAL_FRONT_IF" "$CENTRAL_NS" \
        "$FRONT_NS" "$FRONT_CENTRAL_IF" \
        "10.77.10.1/30" "10.77.10.2/30"

    # Central <-> Rear: 10.77.10.4/30
    create_veth_pair "$CENTRAL_REAR_IF" "$CENTRAL_NS" \
        "$REAR_NS" "$REAR_CENTRAL_IF" \
        "10.77.10.5/30" "10.77.10.6/30"

    create_vcan "$FRONT_NS" "$FRONT_CAN_IF"
    create_vcan "$REAR_NS" "$REAR_CAN_IF"

    install -d -m 0755 "$STATE_DIR"
    printf 'created_utc=%s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" > "$OWNER_MARKER"
    chmod 0644 "$OWNER_MARKER"

    success=1
    trap - EXIT
    log "SOVD network created successfully."
    do_status
}

do_status() {
    local ns
    echo "========== SOVD NETWORK STATUS =========="
    for ns in "$CENTRAL_NS" "$FRONT_NS" "$REAR_NS"; do
        if namespace_exists "$ns"; then
            echo "--- namespace: $ns ---"
            ip -n "$ns" -br link
            ip -n "$ns" -br addr
            ip -n "$ns" route
        else
            echo "--- namespace: $ns (missing) ---"
        fi
    done
    if [[ -f "$OWNER_MARKER" ]]; then
        echo "Ownership marker: present ($OWNER_MARKER)"
    else
        echo "Ownership marker: absent; cleanup is disabled"
    fi
}

do_down() {
    [[ -f "$OWNER_MARKER" ]] || die \
        "Ownership marker not found. Refusing cleanup because this script cannot prove ownership."

    # Remove only the three namespaces and exact links/interfaces owned by this
    # SOVD setup. Never flush host networking or touch other namespace names.
    local ns
    for ns in "$FRONT_NS" "$REAR_NS" "$CENTRAL_NS"; do
        if namespace_exists "$ns"; then
            ip netns delete "$ns"
            log "Deleted owned namespace $ns"
        fi
    done
    rm -f "$OWNER_MARKER"
    rmdir "$STATE_DIR" 2>/dev/null || true
    log "SOVD network removed."
}

main() {
    require_root
    case "${1:-}" in
        up)     do_up ;;
        status) do_status ;;
        down)   do_down ;;
        *)
            echo "Usage: sudo $0 {up|status|down}" >&2
            exit 2
            ;;
    esac
}

main "$@"
