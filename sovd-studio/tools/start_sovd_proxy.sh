#!/usr/bin/env bash
set -euo pipefail
PORT=18010
PID_FILE="/tmp/sovd-central-proxy.pid"
LOG_FILE="/tmp/sovd-central-proxy.log"
WRAPPER="/tmp/sovd-central-proxy-exec"

if ! sudo ip netns list | grep -q '^sdv-central-ns'; then
  echo "ERROR: sdv-central-ns does not exist."
  exit 1
fi
if sudo ss -ltn | grep -q ":${PORT} "; then
  echo "Port ${PORT} is already listening; inspect it before proceeding."
  exit 1
fi

sudo tee "$WRAPPER" >/dev/null <<'EOF'
#!/bin/sh
exec ip netns exec sdv-central-ns socat STDIO TCP:127.0.0.1:8010
EOF
sudo chmod 755 "$WRAPPER"
sudo sh -c "nohup socat TCP-LISTEN:${PORT},bind=127.0.0.1,reuseaddr,fork EXEC:${WRAPPER} >${LOG_FILE} 2>&1 & echo \$! >${PID_FILE}"

for _ in $(seq 1 20); do
  if curl --connect-timeout 1 --max-time 2 -fsS "http://127.0.0.1:${PORT}/api/ecus" >/dev/null 2>&1; then
    echo "SOVD proxy ready at http://127.0.0.1:${PORT}"
    exit 0
  fi
  sleep 0.25
done
echo "Proxy failed to start:"
sudo cat "$LOG_FILE" || true
exit 1
