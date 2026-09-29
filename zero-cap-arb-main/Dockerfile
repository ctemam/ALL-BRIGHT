# syntax=docker/dockerfile:1
#
# Zero-Cap Arbitrage — multi-stage production image.
#
# Intended target region: Frankfurt (Hetzner/OVH eu-central). Latency-sensitive
# components (backend) and the frontend both run in this single image so they
# share a loopback network namespace and the frontend proxy is not crossing a
# network hop.
#
# The region itself is a property of WHERE this image is deployed, not of the
# image. DEPLOYMENT_REGION=frankfurt below is a telemetry/advisory label that
# the backend records in its startup log and uses to prefer EU-hosted RPC
# endpoints. See DEPLOYMENT.md.

# ── Stage 1: build the Rust backend ────────────────────────────────────────
# Must match the channel pinned in rust-toolchain.toml (currently 1.97.0), which
# the resolved dependency graph requires (ruint 1.20, icu_*, time, serde_with).
FROM rust:1.97-slim-bookworm AS backend-builder
WORKDIR /build

# Install the pinned toolchain; the base tag is only a bootstrap and may not
# carry the exact channel.
COPY rust-toolchain.toml ./
RUN rustup toolchain install 1.97.0 --profile minimal --component rustfmt,clippy

# Cache dependency compilation separately from source compilation.
COPY backend/Cargo.toml backend/Cargo.lock* ./backend/
RUN mkdir -p backend/src backend/src/bin \
    && echo 'fn main() {}' > backend/src/main.rs \
    && cd backend \
    && cargo build --release --bin zero-cap-arb-backend \
    && rm -rf src

COPY backend/src ./backend/src
# Touch so cargo re-compiles the real sources rather than reusing the stub.
RUN cd backend && touch src/main.rs && cargo build --release --bin zero-cap-arb-backend

# ── Stage 2: build the Next.js frontend ────────────────────────────────────
FROM node:22-bookworm-slim AS frontend-builder
WORKDIR /build

COPY frontend/package.json frontend/package-lock.json* ./frontend/
RUN cd frontend && npm ci --no-audit --no-fund

COPY frontend/ ./
# NEXT_PUBLIC_* values are inlined at build time, so they must be supplied here
# rather than at runtime. The browser talks to the backend through the same
# public origin, which the frontend server proxies to RUST_BACKEND_URL.
ENV NEXT_PUBLIC_API_BASE=/api
RUN cd frontend && npm run build

# ── Stage 3: runtime ───────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

# curl is required by the HEALTHCHECK below; ca-certificates for outbound HTTPS
# to RPC providers and the Velora API.
RUN apt-get update \
    && apt-get install -y --no-install-recommends curl ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Run unprivileged. uid 10001 avoids colliding with host-mounted volumes.
RUN useradd --create-home --uid 10001 --shell /usr/sbin/nologin zca

COPY --from=backend-builder /build/backend/target/release/zero-cap-arb-backend /usr/local/bin/zero-cap-arb-backend

# Next.js is configured with output: 'standalone', which emits a self-contained
# server with only the modules actually imported. The public/ and .next/static
# directories are NOT bundled by standalone and must be copied explicitly.
WORKDIR /app
COPY --from=frontend-builder --chown=zca:zca /build/frontend/.next/standalone ./
COPY --from=frontend-builder --chown=zca:zca /build/frontend/.next/static ./.next/static
COPY --from=frontend-builder --chown=zca:zca /build/frontend/public ./public

USER zca

# Backend 3001, frontend 3000. These mirror ports.config.json simulation range;
# production runs the same image with ZCA_MODE=production (4001/4000).
EXPOSE 3001 3000

# Deployment region label. Advisory only — see header comment.
ENV DEPLOYMENT_REGION=frankfurt \
    ZCA_MODE=simulation \
    RUST_LOG=info \
    PORT=3001 \
    FRONTEND_PORT=3000 \
    RUST_BACKEND_URL=http://127.0.0.1:3001 \
    NODE_ENV=production

# Fail-safe default: withdrawals are manual until an operator opts in.
ENV PROFIT_TRANSFER_MODE=MANUAL \
    PROFIT_TRANSFER_MIN_USD=25

EXPOSE 3001
HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 \
    CMD curl -fsS "http://127.0.0.1:${PORT}/api/health" || exit 1

# Single entrypoint starts the backend and the frontend together so the
# container is a complete, self-supervised unit.
CMD ["/usr/local/bin/entrypoint.sh"]
