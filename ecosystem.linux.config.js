// PM2 process definitions — Linux build of the allbright fleet.
// Mirrors ecosystem.config.js; only the binary dir/naming differ.
// Each process loads `ENV` itself via allbright-core's config loader.

const BIN = "/tmp/abt/release";
const CWD = "/home/ubuntu/repos/ALL-BRIGHT";

const app = (name, extra) => ({
    name: `allbright-${name}`,
    script: `${BIN}/allbright-${name}`,
    interpreter: "none",
    cwd: CWD,
    autorestart: true,
    env: { RUST_LOG: "info,allbright=info" },
    ...extra,
});

module.exports = {
    apps: [
        app("ingest", { max_memory_restart: "512M" }),
        // CacheDB-per-chain is the memory-heavy process.
        app("simulation", { max_memory_restart: "2G" }),
        app("solver", { max_memory_restart: "512M" }),
        app("executor", { max_memory_restart: "512M" }),
        app("research", { max_memory_restart: "512M" }),
        app("observatory", { max_memory_restart: "256M" }),
        app("challenger", { max_memory_restart: "256M" }),
        app("sentinel", { max_memory_restart: "256M" }),
        {
            // RPC discovery/qualification worker (AGENTS §36/§37) —
            // sole writer of rpc_registry.json; sweeps every 30 min.
            // ~365 concurrent TLS probes spike RSS transiently.
            name: "allbright-rpc-scout",
            script: `${BIN}/allbright-rpc-scout`,
            interpreter: "none",
            cwd: CWD,
            autorestart: true,
            max_memory_restart: "4G",
            env: {
                RUST_LOG: "info,allbright=info",
                RPC_SCOUT_INTERVAL_SECS: "1800",
            },
        },
        app("dashboard", { max_memory_restart: "256M" }),
    ],
};
