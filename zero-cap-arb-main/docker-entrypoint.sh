#!/bin/sh
# Zero-Cap Arbitrage container entrypoint.
#
# Starts the Rust backend and the Next.js frontend in the same container so the
# frontend's server-side proxy reaches the backend over loopback (no network
# hop, no extra TLS termination).
#
# Either process exiting takes the whole container down, so the orchestrator
# restarts the pair together rather than leaving a half-broken stack serving
# traffic. PM2 is deliberately not used here — inside a single container the
# init/supervision role is played by the platform's restart policy.

set -eu

BACKEND_PORT="${PORT:-3001}"
FRONTEND_PORT="${FRONTEND_PORT:-3000}"
export RUST_BACKEND_URL="${RUST_BACKEND_URL:-http://127.0.0.1:${BACKEND_PORT}}"

echo "[entrypoint] region=${DEPLOYMENT_REGION:-unpinned} mode=${ZCA_MODE:-simulation}"
echo "[entrypoint] backend=:${BACKEND_PORT} frontend=:${FRONTEND_PORT} -> ${RUST_BACKEND_URL}"
echo "[entrypoint] profit_transfer=${PROFIT_TRANSFER_MODE:-MANUAL} (min \${PROFIT_TRANSFER_MIN_USD:-25} USD)"

/usr/local/bin/zero-cap-arb-backend &
BACKEND_PID=$!

# Give the backend a moment to bind so the first frontend requests do not 502.
sleep 2

PORT="${FRONTEND_PORT}" HOSTNAME=0.0.0.0 NODE_ENV=production node server.js &
FRONTEND_PID=$!

# Trap signals so `docker stop` reaches both children and we can exit cleanly
# instead of being SIGKILLed after the grace period.
term() {
    echo "[entrypoint] shutting down"
    kill -TERM "$BACKEND_PID" "$FRONTEND_PID" 2>/dev/null || true
    wait "$BACKEND_PID" "$FRONTEND_PID" 2>/dev/null || true
    exit 0
}
trap term TERM INT

# `wait -n` returns when EITHER child exits, which is the trigger we want.
wait -n "$BACKEND_PID" "$FRONTEND_PID"
STATUS=$?

echo "[entrypoint] a child process exited (status ${STATUS}); stopping the other and exiting"
kill -TERM "$BACKEND_PID" "$FRONTEND_PID" 2>/dev/null || true
exit "$STATUS"
