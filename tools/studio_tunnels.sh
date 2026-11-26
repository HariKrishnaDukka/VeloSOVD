#!/usr/bin/env bash
# M2: TCP tunnels from the host into sdv-central-ns for VeloSOVD Studio.
#   127.0.0.1:18010 -> 10.77.10.1:8010  (central SOVD server)
#   127.0.0.1:18020 -> 10.77.10.1:8020  (auth-service)
# Raw TCP forwarding only: TLS (and mTLS) stays end-to-end between Studio
# and the services. Listens on 127.0.0.1 only.
# Usage: tools/studio_tunnels.sh up|down|status
set -euo pipefail
NS=sdv-central-ns
TUNNELS=("18010:10.77.10.1:8010" "18020:10.77.10.1:8020")
RUN=/run/velosovd-studio-tunnels

need_socat() { command -v socat >/dev/null || { echo "ERROR: install socat (sudo apt install socat)"; exit 1; }; }

up() {
    need_socat
    sudo ip netns list | grep -q "^$NS" || { echo "ERROR: $NS missing (sudo tools/setup_sovd_network.sh up)"; exit 1; }
    sudo mkdir -p "$RUN"
    for t in "${TUNNELS[@]}"; do
        IFS=: read -r port host rport <<<"$t"
        if sudo test -s "$RUN/$port.pid" && sudo kill -0 "$(sudo cat "$RUN/$port.pid")" 2>/dev/null; then
            echo "tunnel 127.0.0.1:$port already up"; continue
        fi
        if ss -ltn | grep -q "127.0.0.1:$port "; then
            echo "ERROR: 127.0.0.1:$port is used by another process"; exit 1
        fi
        # The forwarder is run as "sh <file>", not executed directly: /run is
        # mounted noexec on Ubuntu, so execvp() of a file there fails with
        # "Permission denied" and every connection is closed at once.
        sudo tee "$RUN/$port.exec" >/dev/null <<EXEC
#!/bin/sh
exec ip netns exec $NS socat STDIO TCP:$host:$rport
EXEC
        sudo chmod 644 "$RUN/$port.exec"
        sudo sh -c "nohup socat TCP-LISTEN:$port,bind=127.0.0.1,reuseaddr,fork EXEC:'sh $RUN/$port.exec' >$RUN/$port.log 2>&1 & echo \$! > $RUN/$port.pid"
        echo "tunnel 127.0.0.1:$port -> $NS $host:$rport"
    done
}

down() {
    for t in "${TUNNELS[@]}"; do
        port=${t%%:*}
        if sudo test -s "$RUN/$port.pid"; then
            sudo kill "$(sudo cat "$RUN/$port.pid")" 2>/dev/null || true
            sudo rm -f "$RUN/$port.pid" "$RUN/$port.exec"
            echo "tunnel 127.0.0.1:$port stopped"
        fi
    done
}

status() {
    for t in "${TUNNELS[@]}"; do
        port=${t%%:*}
        if sudo test -s "$RUN/$port.pid" && sudo kill -0 "$(sudo cat "$RUN/$port.pid")" 2>/dev/null; then
            echo "127.0.0.1:$port up"; else echo "127.0.0.1:$port down"; fi
    done
}

case "${1:-}" in
    up) up ;; down) down ;; status) status ;;
    *) echo "usage: $0 up|down|status"; exit 2 ;;
esac
