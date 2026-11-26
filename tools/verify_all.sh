#!/usr/bin/env bash
# One-command verification: fmt, clippy, tests, sovd-server gate, traceability.
#   tools/verify_all.sh
# Prints PASS or FAIL per step; for a FAIL it prints the last 40 lines of that step.
# Exit code 0 only if every step passes.
# Needs: Rust toolchain, Python 3, Node.js + npm (the Tauri clippy step builds the
# frontend once if sovd-studio/dist is missing) and the Tauri system libraries (docs/STUDIO.md).
set -u
export CARGO_TERM_COLOR=never
export EXPECT_SOVD_SERVER=86
cd "$(dirname "$0")/.." || exit 2
LOG=$(mktemp -d); trap 'rm -rf "$LOG"' EXIT
total=0; failed=0

step() {  # step "name" "command"
    local name=$1 cmd=$2 t=$SECONDS
    total=$((total + 1))
    if bash -c "$cmd" >"$LOG/$total.log" 2>&1; then
        printf '[PASS] %-42s %4ss\n' "$name" "$((SECONDS - t))"
    else
        failed=$((failed + 1))
        printf '[FAIL] %-42s %4ss   (last 40 lines)\n' "$name" "$((SECONDS - t))"
        tail -n 40 "$LOG/$total.log" | sed 's/^/       | /'
    fi
}

# Reads the cargo test output; needs "sovd-server: <EXPECT> passed, 0 failed".
gate_sovd_server() {
    awk -v want="$EXPECT_SOVD_SERVER" '
        /Running unittests/ && /sovd_server-/ { found = 1; next }
        found && /^test result:/ {
            print; done = 1
            ok = (index($0, "test result: ok. " want " passed; 0 failed;") == 1)
            exit
        }
        END {
            if (!found) { print "sovd-server unit-test run not found in cargo test output"; exit 1 }
            if (done && ok) exit 0
            exit 1
        }' "$1"
}
export -f gate_sovd_server

FRONT='[ -f sovd-studio/dist/index.html ] || (cd sovd-studio && npm ci && npm run build)'

step "fmt --check (root)"               'cargo fmt --all -- --check'
step "fmt --check (src-tauri)"          '(cd sovd-studio/src-tauri && cargo fmt -- --check)'
step "clippy -D warnings (root)"        'cargo clippy --workspace --all-targets --locked -- -D warnings'
step "clippy -D warnings (src-tauri)"   "$FRONT && (cd sovd-studio/src-tauri && cargo clippy --locked --all-targets -- -D warnings)"
step "cargo test --workspace --locked"  "set -o pipefail; cargo test --workspace --locked 2>&1 | tee '$LOG/tests.log'"
step "gate: sovd-server $EXPECT_SOVD_SERVER passed, 0 failed" "gate_sovd_server '$LOG/tests.log'"
step "traceability"                     'python3 tools/check_traceability.py'

echo
if [ "$failed" -eq 0 ]; then
    echo "RESULT: PASS ($total/$total steps)"; exit 0
fi
echo "RESULT: FAIL ($failed of $total steps failed)"; exit 1
