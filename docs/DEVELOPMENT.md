# Development and Startup Guide

## 1. Prerequisites

The development environment requires:

- Linux with network namespace support.
- Rust toolchain and Cargo.
- Node.js and npm.
- Python 3.
- `iproute2`.
- `curl`.
- `socat`.
- Linux virtual CAN support.

Check the toolchain:

```bash
rustc --version
cargo --version
node --version
npm --version
```

Load the Cargo environment if necessary:

```bash
source "$HOME/.cargo/env"
```

## 2. Build and Test

From the repository root, set the path once:

```bash
REPO_ROOT="$(pwd)"
cargo fmt --all
cargo check --workspace
cargo test --workspace
cargo build --workspace
```

Build the frontend:

```bash
cd "$REPO_ROOT/sovd-studio"
npm ci
npm run build
```

## 3. Terminal 1 - Backend and Virtual Network

Open Terminal 1 in the VeloSOVD repository root directory.

Initialize and verify the repository path:

```bash
REPO_ROOT="$(pwd)"
test -f "$REPO_ROOT/Cargo.toml" || {
  echo "ERROR: Open this terminal in the VeloSOVD repository root."
  exit 1
}
```

Build the workspace before starting the services:

```bash
cd "$REPO_ROOT"
source "$HOME/.cargo/env"
cargo build --workspace
```

## 4. Terminal 2 - Frontend

Open Terminal 2 in the same VeloSOVD repository root directory.

Initialize and verify the repository path:

```bash
REPO_ROOT="$(pwd)"
test -f "$REPO_ROOT/Cargo.toml" || {
  echo "ERROR: Open this terminal in the VeloSOVD repository root."
  exit 1
}
```

Start the frontend:

```bash
cd "$REPO_ROOT/sovd-studio"
npm ci
npm run dev
```

## 5. Important Operational Notes

- Start the network before starting zonal and central services.
- Start zonal services before relying on central health checks.
- Start the central server before testing the frontend.
- Do not start duplicate processes on ports `8010`, `9001` or `18010`.
- Do not stop or modify unrelated network namespaces or systemd services.
- The commands above operate on this repository and its SOVD network namespaces.
- If the executable changes after a build, restart the relevant running process so it loads the newly built binary.

## 6. Troubleshooting

### Central health reports a zone offline

Check the relevant zonal log and confirm that port `9001` is listening inside that zone.

### HTTP 404 on a diagnostic request

Check that the currently running virtual ECU executable was built from the latest source and includes the diagnostic HTTP route.

### Host API is unreachable

Check the host proxy process and log:

```bash
cat /tmp/sovd-central-proxy.log
```

### Frontend cannot reach the API

Check the Vite proxy configuration and confirm that the host proxy responds on port `18010`.
