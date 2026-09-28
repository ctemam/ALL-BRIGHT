# Multi-stage build for optimized production image
# Pin the same toolchain as rust-toolchain.toml (1.97) so local, CI and image builds agree.
FROM rust:1.97-slim as builder

# Install system dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    libpq-dev \
    curl \
    build-essential \
    && rm -rf /var/lib/apt/lists/*

# Toolchain versions for the on-chain programs.
#
# These MUST stay in sync with the pinned crate versions in Cargo.toml (anchor-lang 1.2,
# solana-program 4.0). Building the programs with a different toolchain than the crates
# expect fails; the previous values were hard-coded to Solana v1.17.0 with
# "avm install latest", which could not build these crates and made the image
# non-reproducible.
ARG SOLANA_VERSION=v4.0.0
ARG ANCHOR_VERSION=1.2.0

# Install Solana CLI
RUN sh -c "$(curl -sSfL https://release.solana.com/${SOLANA_VERSION}/install)"
ENV PATH="/root/.local/share/solana/install/active_release/bin:$PATH"

# Install Anchor CLI (pinned instead of "latest")
RUN cargo install --git https://github.com/coral-xyz/anchor avm --locked --force
RUN avm install ${ANCHOR_VERSION}
RUN avm use ${ANCHOR_VERSION}

# Create app directory
WORKDIR /app

# Copy workspace configuration
COPY Cargo.toml Cargo.lock ./
COPY engine/Cargo.toml ./engine/

# Copy source code
COPY engine/src ./engine/src/
COPY programs ./programs/
# NOTE: a `COPY shared ./shared/` line used to sit here, but no `shared/` directory exists
# in this repository, so the image could never be built.

# Build dependencies first (for better caching)
# The package/binary is named `arbitrage-engine` (see engine/Cargo.toml); it was previously
# built as `defi-arbitrage-engine`, which does not exist.
RUN cargo build --release --bin arbitrage-engine

# Build Anchor programs
RUN anchor build

# Production stage
FROM debian:bookworm-slim as runtime

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    libpq5 \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN groupadd -r arbitrage && useradd -r -g arbitrage arbitrage

# Create app directory
WORKDIR /app

# Copy binary from builder stage
COPY --from=builder /app/target/release/arbitrage-engine /app/
COPY --from=builder /app/target/deploy /app/programs/

# Copy configuration files
COPY config.toml /app/
COPY migrations /app/migrations/

# Create directories for logs and data
RUN mkdir -p /app/logs /app/data && \
    chown -R arbitrage:arbitrage /app

# Switch to non-root user
USER arbitrage

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:8080/health || exit 1

# Expose ports
EXPOSE 8080 8081 9091

# Set environment variables
ENV RUST_LOG=info,arbitrage_engine=debug
ENV RUST_BACKTRACE=1

# Run the application
CMD ["./arbitrage-engine", "run"]