#!/usr/bin/env bash
set -euo pipefail
PID_FILE="/tmp/sovd-central-proxy.pid"
WRAPPER="/tmp/sovd-central-proxy-exec"
if sudo test -f "$PID_FILE"; then
  PID="$(sudo cat "$PID_FILE")"
  sudo kill "$PID" 2>/dev/null || true
  sudo rm -f "$PID_FILE"
  echo "SOVD proxy stopped."
else
  echo "No proxy PID file found."
fi
sudo rm -f "$WRAPPER"
