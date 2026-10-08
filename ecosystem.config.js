// PM2 process definitions — one app per Rust binary.
// Each process loads `ENV` itself via allbright-core's config loader;
// env blocks below carry only per-process overrides.

// C: is full — .cargo/config.toml redirects cargo's target dir to D:.
const BIN = "D:/allbright-target/release";

module.exports = {
    apps: [
        {
            name: "allbright-ingest",
            script: `${BIN}/allbright-ingest.exe`,
            interpreter: "none",
            autorestart: true,
            max_memory_restart: "512M",
            wait_ready: false,
            env: { RUST_LOG: "info,allbright=info" },
        },
        {
            name: "allbright-simulation",
            script: `${BIN}/allbright-simulation.exe`,
            interpreter: "none",
            autorestart: true,
            // CacheDB-per-chain is the memory-heavy process.
            max_memory_restart: "2G",
            env: { RUST_LOG: "info,allbright=info" },
        },
        {
            name: "allbright-solver",
            script: `${BIN}/allbright-solver.exe`,
            interpreter: "none",
            autorestart: true,
            max_memory_restart: "512M",
            env: { RUST_LOG: "info,allbright=info" },
        },
        {
            name: "allbright-executor",
            script: `${BIN}/allbright-executor.exe`,
            interpreter: "none",
            autorestart: true,
            max_memory_restart: "512M",
            env: { RUST_LOG: "info,allbright=info" },
        },
        {
            name: "allbright-research",
            script: `${BIN}/allbright-research.exe`,
            interpreter: "none",
            autorestart: true,
            max_memory_restart: "512M",
            env: { RUST_LOG: "info,allbright=info" },
        },
        {
            name: "allbright-observatory",
            script: `${BIN}/allbright-observatory.exe`,
            interpreter: "none",
            autorestart: true,
            max_memory_restart: "256M",
            env: { RUST_LOG: "info,allbright=info" },
        },
        {
            name: "allbright-challenger",
            script: `${BIN}/allbright-challenger.exe`,
            interpreter: "none",
            autorestart: true,
            max_memory_restart: "256M",
            env: { RUST_LOG: "info,allbright=info" },
        },
        {
            name: "allbright-sentinel",
            script: `${BIN}/allbright-sentinel.exe`,
            interpreter: "none",
            autorestart: true,
            max_memory_restart: "128M",
            env: { RUST_LOG: "info,allbright=info" },
        },
        {
            // RPC discovery/qualification worker (AGENTS §36/§37) —
            // sole writer of rpc_registry.json; sweeps every 30 min.
            name: "allbright-rpc-scout",
            script: `${BIN}/allbright-rpc-scout.exe`,
            interpreter: "none",
            autorestart: true,
            max_memory_restart: "256M",
            env: {
                RUST_LOG: "info,allbright=info",
                RPC_SCOUT_INTERVAL_SECS: "1800",
            },
        },
        {
            name: "allbright-dashboard",
            script: `${BIN}/allbright-dashboard.exe`,
            interpreter: "none",
            autorestart: true,
            max_memory_restart: "256M",
            env: { RUST_LOG: "info,allbright=info" },
        },
    ],
};
