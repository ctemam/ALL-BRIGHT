//! Configuration contract — layered loading via `figment` (GAP-16):
//!
//! ```text
//!   defaults (in-code, applied during assembly)
//!     ← ENV file  (`ENV`, `.env`, or `env` in the working directory)
//!     ← process environment (highest precedence)
//! ```
//!
//! Secrets are held behind `SecretString` and are never logged, serialized,
//! or telemetried. Validation errors name the offending KEY — values of
//! `*_KEY` / `*_SECRET` / `PRIVATE_*` entries are validated for *presence*
//! only, never echoed.
//!
//! Provider notes (deliberate choices, do not "simplify"):
//! - Layers are `Serialized` maps of RAW `KEY=value` strings — the same
//!   parser (`dotenvy`) and the same byte-for-byte values the legacy
//!   loader read through `std::env::var`. `figment::providers::Env` is
//!   intentionally NOT used: it re-parses every value with TOML-ish
//!   semantics, which would coerce `0x…`-hex secrets/addresses and bare
//!   numerics into non-string `Value`s and change semantics.
//! - `dotenvy::from_path` is still applied for its process-env side
//!   effect: readers outside this struct (`executor_address`,
//!   `RPC_REGISTRY_PATH`, `JOURNAL_*`, `ERC3156_LENDER_*`, …) consult
//!   `std::env::var` directly and rely on the ENV file being injected.
//!   dotenvy never overrides existing env vars, which matches the
//!   file-under-env merge order below exactly.

use anyhow::Context as _;
use figment::{providers::Serialized, Figment};
use secrecy::{ExposeSecret, SecretString};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::events::Chain;

/// ENV file discovery order — identical to the legacy loader.
const ENV_FILE_CANDIDATES: &[&str] = &["ENV", ".env", "env"];

/// §36.9 — private free-tier providers: a single API key in ENV
/// expands into a private endpoint on every chain the provider
/// supports and is prepended to that chain's pool (private quota
/// always outranks shared public capacity). URL shapes are the
/// providers' documented formats; every expanded URL still passes
/// the scout qualification ladder before production admission —
/// a bad slug fails honestly to Quarantined, never enters silently.
/// Keyed URLs live in rpc_pools in memory only; nothing logs or
/// persists the key material.
/// (env_key, url_template, &[(chain_name, network_slug)])
pub const PRIVATE_PROVIDERS: &[(&str, &str, &[(&str, &str)])] = &[
    (
        "ALCHEMY_API_KEY",
        "https://{slug}.g.alchemy.com/v2/{key}",
        &[
            ("eth", "eth-mainnet"), ("bsc", "bnb-mainnet"),
            ("arb", "arb-mainnet"),
            ("op", "opt-mainnet"), ("poly", "polygon-mainnet"),
            ("avax", "avax-mainnet"), ("base", "base-mainnet"),
            ("linea", "linea-mainnet"), ("scroll", "scroll-mainnet"),
            ("blast", "blast-mainnet"),
        ],
    ),
    (
        "INFURA_API_KEY",
        "https://{slug}.infura.io/v3/{key}",
        &[
            ("eth", "mainnet"), ("bsc", "bsc-mainnet"),
            ("arb", "arbitrum-mainnet"),
            ("op", "optimism-mainnet"), ("poly", "polygon-mainnet"),
            ("avax", "avalanche-mainnet"), ("base", "base-mainnet"),
            ("linea", "linea-mainnet"), ("scroll", "scroll-mainnet"),
            ("blast", "blast-mainnet"),
        ],
    ),
    (
        "DRPC_API_KEY",
        "https://lb.drpc.org/ogrpc?network={slug}&dkey={key}",
        &[
            ("eth", "ethereum"), ("bsc", "bsc"), ("arb", "arbitrum"),
            ("op", "optimism"), ("poly", "polygon"), ("avax", "avalanche"),
            ("base", "base"), ("linea", "linea"), ("scroll", "scroll"),
            ("blast", "blast"),
        ],
    ),
    (
        "BLASTAPI_KEY",
        "https://{slug}.blastapi.io/{key}",
        &[
            ("eth", "eth-mainnet"), ("bsc", "bsc-mainnet"),
            ("arb", "arbitrum-one"), ("op", "optimism-mainnet"),
            ("poly", "polygon-mainnet"), ("avax", "avax-mainnet"),
            ("base", "base-mainnet"), ("linea", "linea-mainnet"),
            ("scroll", "scroll-mainnet"), ("blast", "blast-mainnet"),
        ],
    ),
    (
        "ANKR_API_KEY",
        "https://rpc.ankr.com/{slug}/{key}",
        &[
            ("eth", "eth"), ("bsc", "bsc"), ("arb", "arbitrum"),
            ("op", "optimism"), ("poly", "polygon"),
            ("avax", "avalanche"), ("base", "base"),
            ("linea", "linea"), ("scroll", "scroll"),
            ("blast", "blast"),
        ],
    ),
    (
        "CHAINSTACK_API_KEY",
        "https://{slug}.core.chainstack.com/{key}",
        &[
            ("eth", "ethereum-mainnet"), ("bsc", "bsc-mainnet"),
            ("arb", "arbitrum-one"), ("op", "optimism-mainnet"),
            ("poly", "polygon-mainnet"), ("avax", "avalanche-mainnet"),
            ("base", "base-mainnet"),
        ],
    ),
    (
        "BLOCKPI_API_KEY",
        "https://{slug}.blockpi.network/v1/rpc/{key}",
        &[
            ("eth", "ethereum"), ("bsc", "bsc"), ("arb", "arbitrum-one"),
            ("op", "optimism"), ("poly", "polygon"),
            ("avax", "avalanche"), ("base", "base"),
            ("linea", "linea"), ("scroll", "scroll"),
            ("blast", "blast"),
        ],
    ),
    (
        // Slugs per Nodit network docs; the qualification
        // ladder quarantines any that fail honestly.
        "NODIT_API_KEY",
        "https://{slug}.nodit.io/{key}",
        &[
            ("bsc", "bnb-mainnet"), ("arb", "arbitrum-one"),
            ("op", "optimism-mainnet"), ("poly", "polygon-mainnet"),
            ("base", "base-mainnet"),
        ],
    ),
    (
        "TENDERLY_API_KEY",
        "https://{slug}.gateway.tenderly.co/{key}",
        &[
            ("eth", "mainnet"), ("bsc", "bsc"), ("arb", "arbitrum"),
            ("op", "optimism"), ("poly", "polygon"),
            ("avax", "avalanche"), ("base", "base"),
        ],
    ),
    (
        "NOWNODES_API_KEY",
        "https://{slug}.nownodes.io/?API_key={key}",
        &[
            ("eth", "eth"), ("bsc", "bsc"), ("arb", "arbitrum"),
            ("op", "optimism"), ("poly", "matic"),
            ("avax", "avax"), ("base", "base"),
        ],
    ),
    (
        "MORALIS_API_KEY",
        "https://site1.moralis-nodes.com/{slug}/{key}",
        &[
            ("eth", "eth"), ("bsc", "bsc"), ("arb", "arbitrum"),
            ("op", "optimism"), ("poly", "polygon"),
            ("avax", "avalanche"), ("base", "base"),
        ],
    ),
    (
        "ONFINALITY_API_KEY",
        "https://{slug}.api.onfinality.io/rpc?apikey={key}",
        &[
            ("eth", "eth"), ("bsc", "bsc"), ("arb", "arbitrum-one"),
            ("op", "optimism"), ("poly", "polygon"),
            ("avax", "avalanche"), ("base", "base"),
        ],
    ),
    (
        "NODEREAL_API_KEY",
        "https://{slug}-mainnet.nodereal.io/v1/{key}",
        &[
            ("eth", "eth"), ("bsc", "bsc"), ("arb", "arbitrum-one"),
            ("op", "optimism"), ("poly", "polygon"),
            ("base", "base"),
        ],
    ),
    (
        "CHAINNODES_API_KEY",
        "https://{slug}-mainnet.core.chainnodes.org/{key}",
        &[
            ("eth", "ethereum"), ("bsc", "binance-smart-chain"),
            ("arb", "arbitrum-one"), ("op", "optimism"),
            ("poly", "polygon"), ("base", "base"),
        ],
    ),
    (
        "ALLTHATNODE_API_KEY",
        "https://{slug}-mainnet.allthatnode.com/{key}",
        &[
            ("eth", "ethereum"), ("bsc", "bsc"), ("arb", "arbitrum"),
            ("op", "optimism"), ("poly", "polygon"),
            ("avax", "avalanche"), ("base", "base"),
        ],
    ),
    (
        "DVELLIR_API_KEY",
        "https://api-{slug}-mainnet.n.dwellir.com/{key}",
        &[
            ("eth", "ethereum"), ("bsc", "bsc"), ("arb", "arbitrum"),
            ("op", "optimism"), ("poly", "polygon"),
            ("avax", "avalanche"), ("base", "base"),
        ],
    ),
];

/// §36.9 budget surface — provider roster for the dashboard:
/// (env_key, display name, advertised free-tier quota). Quotas are
/// provider-published figures, NOT measured capacity — measured truth
/// is the registry's `verified_rps` (§36.7).
pub const PROVIDER_CATALOG: &[(&str, &str, &str)] = &[
    ("ALCHEMY_API_KEY", "Alchemy", "300M CU/mo"),
    ("INFURA_API_KEY", "Infura", "~100k req/day"),
    ("DRPC_API_KEY", "dRPC", "free tier · trace incl."),
    ("BLASTAPI_KEY", "BlastAPI", "12M calls/mo"),
    ("ANKR_API_KEY", "Ankr", "30 req/s"),
    ("CHAINSTACK_API_KEY", "Chainstack", "3M RU/mo"),
    ("BLOCKPI_API_KEY", "BlockPI", "50M RU/mo"),
    ("NODIT_API_KEY", "Nodit", "free tier"),
    ("TENDERLY_API_KEY", "Tenderly", "free tier"),
    ("NOWNODES_API_KEY", "NOWNodes", "100k req/day"),
    ("MORALIS_API_KEY", "Moralis", "40k CU/day"),
    ("ONFINALITY_API_KEY", "OnFinality", "500k req/day"),
    ("NODEREAL_API_KEY", "NodeReal", "free tier"),
    ("CHAINNODES_API_KEY", "Chainnodes", "free tier"),
    ("ALLTHATNODE_API_KEY", "AllThatNode", "free tier"),
    ("DVELLIR_API_KEY", "Dwellir", "free tier"),
];

/// Keyed providers whose endpoints embed a per-endpoint
/// subdomain/token and can't be templated from a bare key — the
/// operator pastes full URLs into `<CHAIN>_RPC_URLS`; `configured` =
/// host substring present in a live pool.
/// (display name, host substring, advertised free-tier quota)
pub const PASTE_IN_PROVIDERS: &[(&str, &str, &str)] = &[
    ("QuickNode", "quiknode.pro", "10M credits/mo · 80+ chains"),
    ("GetBlock", "getblock.io", "50k req/day"),
    ("Zan", "zan.top", "free tier"),
    ("PublicNode", "publicnode.com", "keyless premium"),
    ("Tenderly (keyless)", "gateway.tenderly.co", "keyless public"),
    ("1RPC", "1rpc.io", "keyless"),
];

/// Keyless-premium endpoints — no signup, no API key — every URL
/// live-verified by `eth_chainId` probe (2026-10-11). Baked into the
/// pool BELOW `<CHAIN>_RPC_URLS` and keyed `*_API_KEY` expansions:
/// per-IP shared quota is the floor tier, never outranking configured
/// capacity. Each URL still passes the scout qualification ladder
/// before production admission — a dead endpoint fails honestly to
/// Quarantined, it is never trusted by presence alone.
/// (chain_name, &[endpoint])
pub const KEYLESS_PREMIUM_ENDPOINTS: &[(&str, &[&str])] = &[
    ("eth", &[
        "https://eth.drpc.org",
        "https://ethereum-rpc.blockreq.com/v1/rpc/public",
        "https://eth-mainnet.public.blastapi.io",
        "https://ethereum-rpc.publicnode.com",
        "https://eth-pokt.nodies.app",
        "https://uk.rpc.blxrbdn.com",
        "https://virginia.rpc.blxrbdn.com",
        "https://mainnet.gateway.tenderly.co",
    ]),
    ("bsc", &[
        "https://bsc-rpc.publicnode.com",
        "https://bsc-rpc.blockreq.com/v1/rpc/public",
        "https://bsc-mainnet.public.blastapi.io",
        "https://bsc-dataseed.bnbchain.org",
        "https://bsc-dataseed3.bnbchain.org",
    ]),
    ("arb", &[
        "https://arb1.arbitrum.io/rpc",
        "https://arbitrum-one.public.blastapi.io",
        "https://arbitrum-one-rpc.publicnode.com",
        "https://arb.drpc.org",
        "https://arbitrum.api.onfinality.io/public",
    ]),
    ("op", &[
        "https://optimism.drpc.org",
        "https://optimism-rpc.publicnode.com",
        "https://optimism-rpc.blockreq.com/v1/rpc/public",
        "https://optimism.gateway.tenderly.co",
        "https://optimism.api.onfinality.io/public",
    ]),
    ("poly", &[
        "https://polygon.drpc.org",
        "https://polygon-bor-rpc.publicnode.com",
        "https://polygon-bor-rpc.blockreq.com/v1/rpc/public",
        "https://polygon.gateway.tenderly.co",
    ]),
    ("avax", &[
        "https://avalanche.drpc.org",
        "https://api.avax.network/ext/bc/C/rpc",
        "https://avalanche-c-chain-rpc.publicnode.com",
        "https://avalanche-rpc.blockreq.com/v1/rpc/public",
        "https://avalanche.api.onfinality.io/public/ext/bc/C/rpc",
    ]),
    ("base", &[
        "https://base.drpc.org",
        "https://mainnet.base.org",
        "https://base-rpc.publicnode.com",
        "https://base-mainnet.public.blastapi.io",
        "https://base-rpc.blockreq.com/v1/rpc/public",
        "https://base.gateway.tenderly.co",
    ]),
    ("linea", &[
        "https://linea.drpc.org",
        "https://rpc.linea.build",
        "https://linea-rpc.publicnode.com",
        "https://linea-rpc.blockreq.com/v1/rpc/public",
        "https://linea.gateway.tenderly.co",
    ]),
    ("scroll", &[
        "https://scroll.drpc.org",
        "https://scroll-rpc.publicnode.com",
        "https://scroll-rpc.blockreq.com/v1/rpc/public",
        "https://rpc.scroll.io",
    ]),
    ("blast", &[
        "https://blast.drpc.org",
        "https://blast-rpc.publicnode.com",
        "https://rpc.blast.io",
    ]),
];

/// Keyless WSS defaults — used ONLY when `<CHAIN>_WSS_URL` is unset.
/// Standard `eth_subscribe` endpoints only: Flashblocks on Base is NOT
/// one (custom preconfirmation schema) — it needs a dedicated consumer,
/// not the generic newHeads watcher.
/// (chain_name, wss_url)
pub const KEYLESS_PREMIUM_WSS: &[(&str, &str)] = &[
    ("eth", "wss://ethereum-rpc.publicnode.com"),
    ("bsc", "wss://bsc-rpc.publicnode.com"),
    ("arb", "wss://arbitrum-one-rpc.publicnode.com"),
    ("op", "wss://optimism-rpc.publicnode.com"),
    ("poly", "wss://polygon-bor-rpc.publicnode.com"),
    ("avax", "wss://avalanche-c-chain-rpc.publicnode.com"),
    ("base", "wss://base-rpc.publicnode.com"),
    ("linea", "wss://linea-rpc.publicnode.com"),
    ("scroll", "wss://scroll-rpc.publicnode.com"),
    ("blast", "wss://blast-rpc.publicnode.com"),
];

#[derive(Debug, Clone)]
pub struct Config {
    pub rpc_urls: HashMap<Chain, String>,
    /// Optional per-chain WSS override (`<CHAIN>_WSS_URL`). Not every
    /// HTTP endpoint offers a websocket transport — capability is
    /// per-endpoint, not per-chain (RPC capability matrix, AGENTS §32).
    pub wss_urls: HashMap<Chain, String>,
    /// Ordered endpoint pool per chain (`<CHAIN>_RPC_URLS`,
    /// comma-separated; falls back to the single `<CHAIN>_RPC_URL`).
    /// Private provider templates (`<PROVIDER>_API_KEY`) are expanded
    /// and prepended — private keyed capacity always outranks public
    /// (§36.9).
    pub rpc_pools: HashMap<Chain, Vec<String>>,
    /// Private-capable providers whose API key is NOT configured
    /// (§36.9) — surfaced on the infra page as "awaiting keys".
    pub pending_providers: Vec<String>,
    /// Active production chains (`ACTIVE_CHAINS`, comma-separated names;
    /// default = all). Collectors, solver scope and sentinel watch lists
    /// restrict to this set; scout/discovery remain full-universe.
    pub active_chains: Vec<Chain>,
    /// Enabled strategy lanes (`STRATEGIES`, comma-separated IDs like
    /// "P01,P02"; default = all five live lanes). Dispatch gates on it —
    /// a disabled strategy's candidates are never simulated.
    pub strategies: Vec<String>,
    pub velora_api_key: Option<SecretString>,
    pub flashbots_relay: String,
    pub flashbots_auth_key: Option<SecretString>,
    pub pimlico_api_key: Option<SecretString>,
    pub zerodev_project_id: Option<SecretString>,
    /// EOA private key — owner of the ERC-4337 smart account. Secrets boundary.
    pub private_key: Option<SecretString>,
    pub profit_wallet: String,
    pub native_usd: f64,
    pub profit_transfer_mode: String,
    pub profit_transfer_min_usd: f64,
    pub auto_execute: bool,
    pub min_profit_usd: f64,
    pub scan_interval_secs: u64,
    pub port: u16,
    /// EV submission mode (GAP-37, `EV_CHAINS`): chains where positive-net
    /// candidates below `min_profit_usd` may still be attempted — revert
    /// cost is ~zero on the sponsored path and the simgate dry-run stays
    /// the authority. Empty = EV mode off everywhere.
    pub ev_chains: Vec<Chain>,
    /// On-chain minProfit floor (USD) carried on EV-class signals
    /// (`EV_FLOOR_USD`, default 0.01). The contract reverts below it —
    /// sponsored gas bounds attempt cost to ~zero.
    pub ev_floor_usd: f64,
    /// Per-chain minimum seconds between EV-class ExecRequests
    /// (`EV_EMIT_SECS`, default 15) — bounds bundler/quota burn.
    pub ev_emit_secs: u64,
    /// Optional per-service port overrides (PORT_INGEST etc.) or defaults.
    pub ports: ServicePorts,
}

#[derive(Debug, Clone)]
pub struct ServicePorts {
    pub ingest: u16,
    pub simulation: u16,
    pub solver: u16,
    pub executor: u16,
    pub research: u16,
    pub dashboard: u16,
    /// Independent Opportunity Observatory — reads ingest, observes the
    /// opportunity universe without strategy thresholds.
    pub observatory: u16,
    /// Strategy Challenger — counterfactual economics on rejected
    /// candidates; audits the solver's decisions.
    pub challenger: u16,
    /// Sentinel — self-diagnosis & bounded remediation (AGENTS §34).
    pub sentinel: u16,
    /// RPC scout — discovery/qualification worker (AGENTS §36/§37).
    pub scout: u16,
}

/// Typed deserialization target for the merged figment layers.
///
/// Named fields are the documented scalar keys — every value stays a raw
/// `Option<String>` so numeric/boolean coercion keeps byte-for-byte
/// parity with the legacy `std::env::var` + `str::parse` semantics (see
/// `num_or`/`assemble`). Field names are the lowercased env var names;
/// figment key lookup is case-insensitive.
///
/// Chain-scoped keys (`<CHAIN>_RPC_URL[S]`, `<CHAIN>_WSS_URL`), provider
/// keys (`<PROVIDER>_API_KEY`) and any service-level extras
/// (`ALLBRIGHT_EXECUTOR_<CHAIN>`, `RPC_REGISTRY_PATH`, …) land in `vars`.
#[derive(Debug, Default, serde::Deserialize)]
struct EnvSpec {
    #[serde(default)]
    active_chains: Option<String>,
    #[serde(default)]
    strategies: Option<String>,
    #[serde(default)]
    velora_api_key: Option<String>,
    #[serde(default)]
    flashboots_relay: Option<String>,
    #[serde(default)]
    flashboots_auth_key: Option<String>,
    #[serde(default)]
    pimlico_api_key: Option<String>,
    #[serde(default)]
    zerodev_project_id: Option<String>,
    #[serde(default)]
    private_key: Option<String>,
    #[serde(default)]
    profit_wallet: Option<String>,
    #[serde(default)]
    native_usd: Option<String>,
    #[serde(default)]
    profit_transfer_mode: Option<String>,
    #[serde(default)]
    profit_transfer_min_usd: Option<String>,
    #[serde(default)]
    auto_execute: Option<String>,
    #[serde(default)]
    min_profit_usd: Option<String>,
    #[serde(default)]
    scan_interval_secs: Option<String>,
    #[serde(default)]
    port: Option<String>,
    #[serde(default)]
    port_ingest: Option<String>,
    #[serde(default)]
    port_sim: Option<String>,
    #[serde(default)]
    port_solver: Option<String>,
    #[serde(default)]
    port_exec: Option<String>,
    #[serde(default)]
    port_research: Option<String>,
    #[serde(default)]
    port_observatory: Option<String>,
    #[serde(default)]
    port_challenger: Option<String>,
    #[serde(default)]
    port_sentinel: Option<String>,
    #[serde(default)]
    port_scout: Option<String>,
    /// Flat view of every remaining merged-layer entry, keyed by env
    /// name (figment lookup is case-insensitive; `get` normalizes too).
    #[serde(flatten)]
    vars: HashMap<String, String>,
}

impl EnvSpec {
    /// Case-insensitive lookup over the non-scalar key space — the same
    /// contract `std::env::var` gave the legacy loader.
    fn get(&self, key: &str) -> Option<String> {
        self.vars.get(&key.to_ascii_lowercase()).cloned()
    }
}

/// First existing ENV file in `dir` (discovery order preserved).
fn env_file_in(dir: &Path) -> Option<PathBuf> {
    ENV_FILE_CANDIDATES
        .iter()
        .map(|cand| dir.join(cand))
        .find(|p| p.is_file())
}

/// Raw `KEY=value` pairs parsed by dotenvy — identical strings to what
/// `std::env::var` would yield after `dotenvy::from_path`, but captured
/// without mutating process state. Malformed lines are skipped, exactly
/// as `dotenvy::from_filename` skips them.
fn env_file_pairs(path: &Path) -> HashMap<String, String> {
    match dotenvy::from_path_iter(path) {
        Ok(iter) => iter.filter_map(|r| r.ok()).collect(),
        Err(_) => HashMap::new(),
    }
}

/// Numeric coercion with the legacy semantics: unparseable input falls
/// back to the default — but now the offending KEY is warned (never the
/// value, which may be adjacent to secret material).
fn num_or<T: std::str::FromStr>(key: &'static str, raw: Option<String>, default: T) -> T {
    match raw {
        Some(s) => match s.parse::<T>() {
            Ok(v) => v,
            Err(_) => {
                tracing::warn!(
                    config_key = key,
                    "malformed numeric config value — falling back to default"
                );
                default
            }
        },
        None => default,
    }
}

/// Config is deliberately non-Serializable: secrets must never be written
/// to disk, telemetry, or logs.
impl Config {
    pub fn load() -> anyhow::Result<Self> {
        Self::load_in(Path::new("."))
    }

    /// Load from a working directory — `dir` is where the ENV file is
    /// discovered; process env always overlays it.
    fn load_in(dir: &Path) -> anyhow::Result<Self> {
        let file = env_file_in(dir);
        // Process-env side effect preserved: readers outside this struct
        // (`executor_address`, `RPC_REGISTRY_PATH`, `JOURNAL_*`, …) read
        // `std::env::var` directly. dotenvy fills only missing vars, so
        // real process env keeps precedence — same as the merge order
        // in `figment_for`.
        if let Some(path) = &file {
            let _ = dotenvy::from_path(path);
        }
        let fig = Self::figment_for(file.as_deref(), Self::process_env_pairs());
        Self::extract(&fig)
    }

    /// Process environment as owned string pairs, skipping non-UTF-8
    /// names/values instead of panicking (fail-closed, same spirit as
    /// `std::env::var(..).ok()` in the legacy loader).
    fn process_env_pairs() -> impl Iterator<Item = (String, String)> {
        std::env::vars_os()
            .filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?)))
    }

    /// Lowercase map → `Serialized` provider. `Serialized` preserves key
    /// case but `EnvSpec` fields are lowercase — without normalization
    /// every named scalar misses its field and lands in `vars`.
    fn serialized_env(map: HashMap<String, String>) -> Serialized<HashMap<String, String>> {
        Serialized::defaults(
            map.into_iter()
                .map(|(k, v)| (k.to_ascii_lowercase(), v))
                .collect::<HashMap<_, _>>(),
        )
    }

    /// Build the layered figment: `defaults ← ENV file ← process env`.
    fn figment_for(
        file: Option<&Path>,
        env: impl Iterator<Item = (String, String)>,
    ) -> Figment {
        let mut fig = Figment::new();
        if let Some(path) = file {
            fig = fig.merge(Self::serialized_env(env_file_pairs(path)));
        }
        fig.merge(Self::serialized_env(
            env.collect::<HashMap<String, String>>(),
        ))
    }

    /// Typed extraction + assembly. figment errors name the offending
    /// key path; values are never included in our error messages.
    fn extract(fig: &Figment) -> anyhow::Result<Self> {
        let mut spec: EnvSpec = fig
            .extract()
            .context("config deserialization failed (see key path in error)")?;
        // Normalize the flattened key space: provider maps preserve the
        // env var's original case while `EnvSpec::get` looks up lowercase —
        // matching the (Windows) case-insensitive `std::env::var` contract
        // the legacy loader relied on.
        spec.vars = spec
            .vars
            .into_iter()
            .map(|(k, v)| (k.to_ascii_lowercase(), v))
            .collect();
        Ok(Self::assemble(spec))
    }

    /// Fold the merged env spec into the typed Config — defaults are
    /// applied here (the bottom layer of the config stack).
    fn assemble(spec: EnvSpec) -> Self {
        let get = |k: &str| spec.get(k);
        let secret =
            |v: Option<String>| v.filter(|s| !s.is_empty()).map(SecretString::from);

        let mut rpc_urls = HashMap::new();
        let mut wss_urls = HashMap::new();
        for chain in Chain::ALL {
            let key = format!("{}_RPC_URL", chain.name().to_uppercase());
            if let Some(url) = get(&key).filter(|s| !s.is_empty()) {
                rpc_urls.insert(chain, url);
            }
            let wkey = format!("{}_WSS_URL", chain.name().to_uppercase());
            if let Some(url) = get(&wkey).filter(|s| !s.is_empty()) {
                wss_urls.insert(chain, url);
            }
        }

        let mut rpc_pools = HashMap::new();
        for chain in Chain::ALL {
            let pkey = format!("{}_RPC_URLS", chain.name().to_uppercase());
            let mut pool: Vec<String> = get(&pkey)
                .map(|s| {
                    s.split(',')
                        .map(|u| u.trim().to_string())
                        .filter(|u| !u.is_empty())
                        .collect()
                })
                .unwrap_or_default();
            if pool.is_empty() {
                if let Some(u) = rpc_urls.get(&chain) {
                    pool.push(u.clone());
                }
            }
            if !pool.is_empty() {
                rpc_pools.insert(chain, pool);
            }
        }

        // §36.9 — private free-tier providers: a single API key in ENV
        // expands into a private endpoint on every chain the provider
        // supports and is prepended to that chain's pool (private quota
        // always outranks shared public capacity). URL shapes are the
        // providers' documented formats; every expanded URL still passes
        // the scout qualification ladder before production admission —
        // a bad slug fails honestly to Quarantined, never enters silently.
        // Keyed URLs live in rpc_pools in memory only; nothing logs or
        // persists the key material.
        // (provider table hoisted to module-level `PRIVATE_PROVIDERS`)
        let mut pending_providers: Vec<String> = Vec::new();
        for (env_key, tpl, nets) in PRIVATE_PROVIDERS {
            let Some(key) = get(env_key).filter(|s| !s.is_empty()) else {
                pending_providers.push(
                    env_key
                        .trim_end_matches("_API_KEY")
                        .trim_end_matches("_KEY")
                        .to_string(),
                );
                continue;
            };
            for (cname, slug) in *nets {
                if let Some(chain) = Chain::ALL.iter().find(|c| c.name() == *cname) {
                    let url = tpl.replace("{slug}", slug).replace("{key}", &key);
                    let pool = rpc_pools.entry(*chain).or_default();
                    if !pool.contains(&url) {
                        pool.insert(0, url);
                    }
                }
            }
        }

        // Active-chain scope: `ACTIVE_CHAINS=bsc,arb,poly,base,avax`.
        // Unknown names are dropped (logged downstream via the absence of
        // a collector, never silently widening scope).
        // Hoisted before any `spec` field moves — `get` borrows spec.
        // Keyless-premium floor tier: appended below env/keyed capacity;
        // WSS seeded only where the operator set no `<CHAIN>_WSS_URL`.
        for (cname, eps) in KEYLESS_PREMIUM_ENDPOINTS {
            if let Some(chain) = Chain::ALL.iter().find(|c| c.name() == *cname) {
                let pool = rpc_pools.entry(*chain).or_default();
                for u in *eps {
                    if !pool.iter().any(|e| e == u) {
                        pool.push((*u).to_string());
                    }
                }
            }
        }
        for (cname, w) in KEYLESS_PREMIUM_WSS {
            if let Some(chain) = Chain::ALL.iter().find(|c| c.name() == *cname) {
                wss_urls.entry(*chain).or_insert_with(|| (*w).to_string());
            }
        }

        let ev_chains_raw = get("EV_CHAINS");
        let ev_floor_raw = get("EV_FLOOR_USD");
        let ev_emit_raw = get("EV_EMIT_SECS");

        let active_chains: Vec<Chain> = spec
            .active_chains
            .map(|s| {
                s.split(',')
                    .filter_map(|n| {
                        let n = n.trim().to_ascii_lowercase();
                        Chain::ALL.iter().copied().find(|c| c.name() == n)
                    })
                    .collect()
            })
            .filter(|v: &Vec<Chain>| !v.is_empty())
            .unwrap_or_else(|| Chain::ALL.to_vec());

        let strategies = spec
            .strategies
            .map(|v| {
                v.split(',')
                    .map(|s| s.trim().to_uppercase())
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<String>>()
            })
            .filter(|v: &Vec<String>| !v.is_empty())
            .unwrap_or_else(|| {
                ["P01", "P02", "P03", "P04", "P05"]
                    .iter()
                    .map(|s| s.to_string())
                    .collect()
            });

        // EV-class submission set (GAP-37): parsed like ACTIVE_CHAINS;
        // unknown names dropped; empty = disabled. Eligibility is
        // additionally scoped by active_chains downstream — a chain can't
        // fire EV attempts it doesn't observe.
        let ev_chains: Vec<Chain> = ev_chains_raw
            .map(|s| {
                s.split(',')
                    .filter_map(|n| {
                        let n = n.trim().to_ascii_lowercase();
                        Chain::ALL.iter().copied().find(|c| c.name() == n)
                    })
                    .collect()
            })
            .unwrap_or_default();

        Config {
            rpc_urls,
            wss_urls,
            rpc_pools,
            pending_providers,
            active_chains,
            strategies,
            ev_chains,
            ev_floor_usd: num_or("EV_FLOOR_USD", ev_floor_raw, 0.01),
            ev_emit_secs: num_or("EV_EMIT_SECS", ev_emit_raw, 15u64),
            velora_api_key: secret(spec.velora_api_key),
            flashbots_relay: spec
                .flashboots_relay
                .unwrap_or_else(|| "https://relay.flashbots.net".into()),
            flashbots_auth_key: secret(spec.flashboots_auth_key),
            pimlico_api_key: secret(spec.pimlico_api_key),
            zerodev_project_id: secret(spec.zerodev_project_id),
            private_key: secret(spec.private_key),
            profit_wallet: spec.profit_wallet.unwrap_or_default(),
            native_usd: num_or("NATIVE_USD", spec.native_usd, 0.0),
            profit_transfer_mode: spec
                .profit_transfer_mode
                .unwrap_or_else(|| "MANUAL".into()),
            profit_transfer_min_usd: num_or(
                "PROFIT_TRANSFER_MIN_USD",
                spec.profit_transfer_min_usd,
                100.0,
            ),
            auto_execute: spec
                .auto_execute
                .map(|s| s.eq_ignore_ascii_case("true"))
                .unwrap_or(false),
            min_profit_usd: num_or("MIN_PROFIT_USD", spec.min_profit_usd, 5.0),
            scan_interval_secs: num_or("SCAN_INTERVAL_SECS", spec.scan_interval_secs, 5),
            port: num_or("PORT", spec.port.clone(), 3001),
            ports: ServicePorts {
                ingest: num_or("PORT_INGEST", spec.port_ingest, 3101),
                simulation: num_or("PORT_SIM", spec.port_sim, 3102),
                solver: num_or("PORT_SOLVER", spec.port_solver, 3103),
                executor: num_or("PORT_EXEC", spec.port_exec, 3104),
                research: num_or("PORT_RESEARCH", spec.port_research, 3105),
                dashboard: num_or("PORT", spec.port, 3001),
                observatory: num_or("PORT_OBSERVATORY", spec.port_observatory, 3106),
                challenger: num_or("PORT_CHALLENGER", spec.port_challenger, 3107),
                sentinel: num_or("PORT_SENTINEL", spec.port_sentinel, 3108),
                scout: num_or("PORT_SCOUT", spec.port_scout, 3109),
            },
        }
    }

    /// Present-but-redacted view for logs.
    pub fn redacted_summary(&self) -> RedactedConfig {
        RedactedConfig {
            chains: self.rpc_urls.keys().map(|c| c.name().to_string()).collect(),
            active: self.active_chains.iter().map(|c| c.name().to_string()).collect(),
            velora: self.velora_api_key.is_some(),
            flashbots_auth: self.flashbots_auth_key.is_some(),
            pimlico: self.pimlico_api_key.is_some(),
            zerodev: self.zerodev_project_id.is_some(),
            private_key: self.private_key.is_some(),
            auto_execute: self.auto_execute,
            min_profit_usd: self.min_profit_usd,
            scan_interval_secs: self.scan_interval_secs,
        }
    }

    /// Expose the key only at the signing boundary.
    pub fn signing_key(&self) -> Option<&str> {
        self.private_key.as_ref().map(|s| s.expose_secret())
    }

    /// Deployed `AllbrightExecutor` address on a chain, from
    /// `ALLBRIGHT_EXECUTOR_<CHAIN>` env. `None` = not deployed there —
    /// never substitute a default (AGENTS.md §14).
    ///
    /// NOTE: reads `std::env` directly — relies on the `dotenvy` side
    /// effect performed in `load_in`.
    pub fn executor_address(&self, chain: Chain) -> Option<String> {
        std::env::var(format!("ALLBRIGHT_EXECUTOR_{}", chain.name().to_uppercase()))
            .ok()
            .filter(|s| !s.is_empty())
    }
}

#[derive(Debug, Serialize)]
pub struct RedactedConfig {
    pub chains: Vec<String>,
    pub active: Vec<String>,
    pub velora: bool,
    pub flashbots_auth: bool,
    pub pimlico: bool,
    pub zerodev: bool,
    pub private_key: bool,
    pub auto_execute: bool,
    pub min_profit_usd: f64,
    pub scan_interval_secs: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::ExposeSecret;

    /// Assemble a Config from raw KEY=value pairs only — no ENV file
    /// discovery, no process-env overlay. Hermetic test path through the
    /// same figment pipeline (`Serialized` layer → `extract` → `assemble`).
    fn from_pairs(pairs: &[(&str, &str)]) -> anyhow::Result<Config> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let fig = Figment::new().merge(Config::serialized_env(map));
        Config::extract(&fig)
    }

    /// The legacy hand-rolled loader, preserved verbatim as a parity
    /// oracle (GAP-16 verification). `get` is injected so both loaders
    /// can be driven over the identical merged key space (ENV-file pairs
    /// under process-env pairs — the same precedence as the layered
    /// figment and the same resolution `dotenvy::from_filename` +
    /// `std::env::var` produced).
    fn legacy_load(get: &dyn Fn(&str) -> Option<String>) -> Config {
        let secret =
            |k: &str| get(k).filter(|s| !s.is_empty()).map(SecretString::from);

        let mut rpc_urls = HashMap::new();
        let mut wss_urls = HashMap::new();
        for chain in Chain::ALL {
            let key = format!("{}_RPC_URL", chain.name().to_uppercase());
            if let Some(url) = get(&key).filter(|s| !s.is_empty()) {
                rpc_urls.insert(chain, url);
            }
            let wkey = format!("{}_WSS_URL", chain.name().to_uppercase());
            if let Some(url) = get(&wkey).filter(|s| !s.is_empty()) {
                wss_urls.insert(chain, url);
            }
        }

        let mut rpc_pools = HashMap::new();
        for chain in Chain::ALL {
            let pkey = format!("{}_RPC_URLS", chain.name().to_uppercase());
            let mut pool: Vec<String> = get(&pkey)
                .map(|s| {
                    s.split(',')
                        .map(|u| u.trim().to_string())
                        .filter(|u| !u.is_empty())
                        .collect()
                })
                .unwrap_or_default();
            if pool.is_empty() {
                if let Some(u) = rpc_urls.get(&chain) {
                    pool.push(u.clone());
                }
            }
            if !pool.is_empty() {
                rpc_pools.insert(chain, pool);
            }
        }

        // (provider table hoisted to module-level `PRIVATE_PROVIDERS`)
        let mut pending_providers: Vec<String> = Vec::new();
        for (env_key, tpl, nets) in PRIVATE_PROVIDERS {
            let Some(key) = get(env_key).filter(|s| !s.is_empty()) else {
                pending_providers.push(
                    env_key
                        .trim_end_matches("_API_KEY")
                        .trim_end_matches("_KEY")
                        .to_string(),
                );
                continue;
            };
            for (cname, slug) in *nets {
                if let Some(chain) = Chain::ALL.iter().find(|c| c.name() == *cname) {
                    let url = tpl.replace("{slug}", slug).replace("{key}", &key);
                    let pool = rpc_pools.entry(*chain).or_default();
                    if !pool.contains(&url) {
                        pool.insert(0, url);
                    }
                }
            }
        }

        // Keyless-premium floor tier: appended below env/keyed capacity;
        // WSS seeded only where the operator set no `<CHAIN>_WSS_URL`.
        for (cname, eps) in KEYLESS_PREMIUM_ENDPOINTS {
            if let Some(chain) = Chain::ALL.iter().find(|c| c.name() == *cname) {
                let pool = rpc_pools.entry(*chain).or_default();
                for u in *eps {
                    if !pool.iter().any(|e| e == u) {
                        pool.push((*u).to_string());
                    }
                }
            }
        }
        for (cname, w) in KEYLESS_PREMIUM_WSS {
            if let Some(chain) = Chain::ALL.iter().find(|c| c.name() == *cname) {
                wss_urls.entry(*chain).or_insert_with(|| (*w).to_string());
            }
        }

        let active_chains: Vec<Chain> = get("ACTIVE_CHAINS")
            .map(|s| {
                s.split(',')
                    .filter_map(|n| {
                        let n = n.trim().to_ascii_lowercase();
                        Chain::ALL.iter().copied().find(|c| c.name() == n)
                    })
                    .collect()
            })
            .filter(|v: &Vec<Chain>| !v.is_empty())
            .unwrap_or_else(|| Chain::ALL.to_vec());

        let strategies = get("STRATEGIES")
            .map(|v| {
                v.split(',')
                    .map(|s| s.trim().to_uppercase())
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<String>>()
            })
            .filter(|v: &Vec<String>| !v.is_empty())
            .unwrap_or_else(|| {
                ["P01", "P02", "P03", "P04", "P05"]
                    .iter()
                    .map(|s| s.to_string())
                    .collect()
            });

        // EV-class submission set (GAP-37): parsed like ACTIVE_CHAINS;
        // unknown names dropped; empty = disabled.
        let ev_chains: Vec<Chain> = get("EV_CHAINS")
            .map(|s| {
                s.split(',')
                    .filter_map(|n| {
                        let n = n.trim().to_ascii_lowercase();
                        Chain::ALL.iter().copied().find(|c| c.name() == n)
                    })
                    .collect()
            })
            .unwrap_or_default();

        Config {
            rpc_urls,
            wss_urls,
            rpc_pools,
            pending_providers,
            active_chains,
            strategies,
            ev_chains,
            ev_floor_usd: get("EV_FLOOR_USD")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.01),
            ev_emit_secs: get("EV_EMIT_SECS")
                .and_then(|s| s.parse().ok())
                .unwrap_or(15),
            velora_api_key: secret("VELORA_API_KEY"),
            flashbots_relay: get("FLASHBOOTS_RELAY")
                .unwrap_or_else(|| "https://relay.flashbots.net".into()),
            flashbots_auth_key: secret("FLASHBOOTS_AUTH_KEY"),
            pimlico_api_key: secret("PIMLICO_API_KEY"),
            zerodev_project_id: secret("ZERODEV_PROJECT_ID"),
            private_key: secret("PRIVATE_KEY"),
            profit_wallet: get("PROFIT_WALLET").unwrap_or_default(),
            native_usd: get("NATIVE_USD")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.0),
            profit_transfer_mode: get("PROFIT_TRANSFER_MODE")
                .unwrap_or_else(|| "MANUAL".into()),
            profit_transfer_min_usd: get("PROFIT_TRANSFER_MIN_USD")
                .and_then(|s| s.parse().ok())
                .unwrap_or(100.0),
            auto_execute: get("AUTO_EXECUTE")
                .map(|s| s.eq_ignore_ascii_case("true"))
                .unwrap_or(false),
            min_profit_usd: get("MIN_PROFIT_USD")
                .and_then(|s| s.parse().ok())
                .unwrap_or(5.0),
            scan_interval_secs: get("SCAN_INTERVAL_SECS")
                .and_then(|s| s.parse().ok())
                .unwrap_or(5),
            port: get("PORT").and_then(|s| s.parse().ok()).unwrap_or(3001),
            ports: ServicePorts {
                ingest: get("PORT_INGEST").and_then(|s| s.parse().ok()).unwrap_or(3101),
                simulation: get("PORT_SIM").and_then(|s| s.parse().ok()).unwrap_or(3102),
                solver: get("PORT_SOLVER").and_then(|s| s.parse().ok()).unwrap_or(3103),
                executor: get("PORT_EXEC").and_then(|s| s.parse().ok()).unwrap_or(3104),
                research: get("PORT_RESEARCH").and_then(|s| s.parse().ok()).unwrap_or(3105),
                dashboard: get("PORT").and_then(|s| s.parse().ok()).unwrap_or(3001),
                observatory: get("PORT_OBSERVATORY").and_then(|s| s.parse().ok()).unwrap_or(3106),
                challenger: get("PORT_CHALLENGER").and_then(|s| s.parse().ok()).unwrap_or(3107),
                sentinel: get("PORT_SENTINEL").and_then(|s| s.parse().ok()).unwrap_or(3108),
                scout: get("PORT_SCOUT").and_then(|s| s.parse().ok()).unwrap_or(3109),
            },
        }
    }

    /// Field-wise diff of two Configs. Returns the NAMES of differing
    /// fields only — never values (secrets boundary holds in tests too).
    fn config_diff(a: &Config, b: &Config) -> Vec<&'static str> {
        let mut d = Vec::new();
        if a.rpc_urls != b.rpc_urls {
            d.push("rpc_urls");
        }
        if a.wss_urls != b.wss_urls {
            d.push("wss_urls");
        }
        if a.rpc_pools != b.rpc_pools {
            d.push("rpc_pools");
        }
        if a.pending_providers != b.pending_providers {
            d.push("pending_providers");
        }
        if a.active_chains != b.active_chains {
            d.push("active_chains");
        }
        if a.strategies != b.strategies {
            d.push("strategies");
        }
        let sec_eq = |x: &Option<SecretString>, y: &Option<SecretString>| match (x, y) {
            (Some(x), Some(y)) => x.expose_secret() == y.expose_secret(),
            (None, None) => true,
            _ => false,
        };
        if !sec_eq(&a.velora_api_key, &b.velora_api_key) {
            d.push("velora_api_key");
        }
        if a.flashbots_relay != b.flashbots_relay {
            d.push("flashbots_relay");
        }
        if !sec_eq(&a.flashbots_auth_key, &b.flashbots_auth_key) {
            d.push("flashbots_auth_key");
        }
        if !sec_eq(&a.pimlico_api_key, &b.pimlico_api_key) {
            d.push("pimlico_api_key");
        }
        if !sec_eq(&a.zerodev_project_id, &b.zerodev_project_id) {
            d.push("zerodev_project_id");
        }
        if !sec_eq(&a.private_key, &b.private_key) {
            d.push("private_key");
        }
        if a.profit_wallet != b.profit_wallet {
            d.push("profit_wallet");
        }
        if a.native_usd != b.native_usd {
            d.push("native_usd");
        }
        if a.profit_transfer_mode != b.profit_transfer_mode {
            d.push("profit_transfer_mode");
        }
        if a.profit_transfer_min_usd != b.profit_transfer_min_usd {
            d.push("profit_transfer_min_usd");
        }
        if a.auto_execute != b.auto_execute {
            d.push("auto_execute");
        }
        if a.min_profit_usd != b.min_profit_usd {
            d.push("min_profit_usd");
        }
        if a.scan_interval_secs != b.scan_interval_secs {
            d.push("scan_interval_secs");
        }
        if a.ev_chains != b.ev_chains {
            d.push("ev_chains");
        }
        if a.ev_floor_usd != b.ev_floor_usd {
            d.push("ev_floor_usd");
        }
        if a.ev_emit_secs != b.ev_emit_secs {
            d.push("ev_emit_secs");
        }
        if a.port != b.port {
            d.push("port");
        }
        let (pa, pb) = (&a.ports, &b.ports);
        if (pa.ingest, pa.simulation, pa.solver, pa.executor, pa.research,
            pa.dashboard, pa.observatory, pa.challenger, pa.sentinel, pa.scout)
            != (pb.ingest, pb.simulation, pb.solver, pb.executor, pb.research,
                pb.dashboard, pb.observatory, pb.challenger, pb.sentinel, pb.scout)
        {
            d.push("ports");
        }
        d
    }

    /// Representative ENV sample covering every config key class.
    const SAMPLE: &[(&str, &str)] = &[
        ("ETH_RPC_URL", "https://eth.example.test/rpc"),
        ("ARB_RPC_URL", "https://arb.example.test/rpc"),
        ("BSC_RPC_URL", "https://bsc.example.test/rpc"),
        ("ETH_WSS_URL", "wss://eth.example.test/ws"),
        (
            "ETH_RPC_URLS",
            "https://eth-a.test, https://eth-b.test ,,https://eth-c.test",
        ),
        ("BSC_RPC_URLS", "https://bsc-a.test,https://bsc-b.test"),
        ("ALCHEMY_API_KEY", "test-alchemy-key-material"),
        ("ACTIVE_CHAINS", "bsc,arb,poly,base,avax"),
        ("STRATEGIES", "p01, p03 ,P05"),
        ("VELORA_API_KEY", "test-velora-key"),
        ("FLASHBOOTS_RELAY", "https://relay.example.test"),
        ("FLASHBOOTS_AUTH_KEY", "test-flashbots-auth"),
        ("PIMLICO_API_KEY", "test-pimlico-key"),
        ("ZERODEV_PROJECT_ID", "test-zerodev-id"),
        (
            "PRIVATE_KEY",
            "0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
        ),
        ("PROFIT_WALLET", "0x0000000000000000000000000000000000000001"),
        ("NATIVE_USD", "412.5"),
        ("PROFIT_TRANSFER_MODE", "AUTO"),
        ("PROFIT_TRANSFER_MIN_USD", "42.5"),
        ("AUTO_EXECUTE", "TRUE"),
        ("MIN_PROFIT_USD", "9.75"),
        ("SCAN_INTERVAL_SECS", "7"),
        ("PORT", "4444"),
        ("PORT_INGEST", "4101"),
        ("PORT_SIM", "4102"),
        ("PORT_SOLVER", "4103"),
        ("PORT_EXEC", "4104"),
        ("PORT_RESEARCH", "4105"),
        ("PORT_OBSERVATORY", "4106"),
        ("PORT_CHALLENGER", "4107"),
        ("PORT_SENTINEL", "4108"),
        ("PORT_SCOUT", "4109"),
    ];

    /// ENV sample → Config roundtrip: every field lands where the legacy
    /// semantics put it.
    #[test]
    fn env_sample_roundtrip() {
        let cfg = from_pairs(SAMPLE).expect("sample must deserialize");

        assert_eq!(cfg.rpc_urls.len(), 3);
        assert_eq!(
            cfg.rpc_urls.get(&Chain::Eth).map(String::as_str),
            Some("https://eth.example.test/rpc")
        );
        assert_eq!(
            cfg.wss_urls.get(&Chain::Eth).map(String::as_str),
            Some("wss://eth.example.test/ws")
        );
        // Comma-separated pool, trimmed, empty entries dropped.
        assert_eq!(
            cfg.rpc_pools.get(&Chain::Eth).map(|v| v.len()),
            Some(12) // 3 listed + alchemy prepend + 8 keyless floor
        );
        // Alchemy key expands a private endpoint and PREPENDS it (§36.9).
        assert!(
            cfg.rpc_pools[&Chain::Eth][0]
                .starts_with("https://eth-mainnet.g.alchemy.com/v2/"),
            "private keyed endpoint must outrank public pool entries"
        );
        // Keyless-premium URLs fill the floor tier BELOW env/keyed capacity.
        assert!(
            cfg.rpc_pools[&Chain::Eth]
                .last()
                .unwrap()
                .contains("gateway.tenderly.co")
                || cfg.rpc_pools[&Chain::Eth]
                    .iter()
                    .any(|u| u.contains("blockreq.com")),
            "keyless-premium floor must be present and last"
        );
        // Arb had no <CHAIN>_RPC_URLS → pool falls back to <CHAIN>_RPC_URL,
        // then alchemy prepends, then the keyless floor appends.
        assert_eq!(cfg.rpc_pools[&Chain::Arb].len(), 2 + 5);
        // Providers without keys surface as pending.
        assert!(cfg.pending_providers.contains(&"INFURA".to_string()));
        assert!(cfg.pending_providers.contains(&"DRPC".to_string()));
        assert!(cfg.pending_providers.contains(&"BLASTAPI".to_string()));
        assert!(!cfg.pending_providers.contains(&"ALCHEMY".to_string()));

        assert_eq!(cfg.active_chains.len(), 5);
        assert_eq!(cfg.active_chains[0], Chain::Bsc);
        assert_eq!(cfg.strategies, vec!["P01", "P03", "P05"]);

        assert!(cfg.velora_api_key.is_some());
        assert_eq!(cfg.flashbots_relay, "https://relay.example.test");
        assert!(cfg.flashbots_auth_key.is_some());
        assert!(cfg.pimlico_api_key.is_some());
        assert!(cfg.zerodev_project_id.is_some());
        assert!(cfg.signing_key().is_some());
        assert_eq!(
            cfg.profit_wallet,
            "0x0000000000000000000000000000000000000001"
        );
        assert!((cfg.native_usd - 412.5).abs() < f64::EPSILON);
        assert_eq!(cfg.profit_transfer_mode, "AUTO");
        assert!((cfg.profit_transfer_min_usd - 42.5).abs() < f64::EPSILON);
        // "TRUE" — case-insensitive true, identical to legacy.
        assert!(cfg.auto_execute);
        assert!((cfg.min_profit_usd - 9.75).abs() < f64::EPSILON);
        assert_eq!(cfg.scan_interval_secs, 7);
        assert_eq!(cfg.port, 4444);
        assert_eq!(cfg.ports.dashboard, 4444);
        assert_eq!(cfg.ports.ingest, 4101);
        assert_eq!(cfg.ports.scout, 4109);
    }

    /// Defaults must be byte-identical to the legacy loader when nothing
    /// is configured.
    #[test]
    fn defaults_preserved() {
        let cfg = from_pairs(&[]).expect("empty env must deserialize");
        assert_eq!(cfg.flashbots_relay, "https://relay.flashbots.net");
        assert_eq!(cfg.profit_transfer_mode, "MANUAL");
        assert_eq!(cfg.port, 3001);
        assert_eq!(cfg.ports.ingest, 3101);
        assert_eq!(cfg.ports.scout, 3109);
        assert_eq!(cfg.active_chains.len(), Chain::ALL.len());
        assert_eq!(cfg.strategies, vec!["P01", "P02", "P03", "P04", "P05"]);
        assert!(!cfg.auto_execute);
        assert!(cfg.pending_providers.contains(&"ALCHEMY".to_string()));
    }

    /// GAP-16 acceptance: the new figment pipeline produces the IDENTICAL
    /// Config the legacy hand-rolled loader produced, over (a) the
    /// representative sample and (b) the real ENV file + live process
    /// environment when present. Diff reports field names only.
    #[test]
    fn figment_matches_legacy_semantics() {
        // (a) hermetic sample: both loaders over the same merged key space.
        let merged: HashMap<String, String> = SAMPLE
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let legacy = legacy_load(&|k| merged.get(k).cloned());
        let new = from_pairs(SAMPLE).unwrap();
        let diff = config_diff(&legacy, &new);
        assert!(
            diff.is_empty(),
            "figment/legacy semantic diff on sample: {diff:?}"
        );

        // (b) real ENV file at the repo root (if present on this machine)
        // layered under the live process environment — mirrors `load()`
        // exactly. File pairs take the lower precedence slot, matching
        // dotenvy's never-override behavior.
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        if let Some(file) = env_file_in(&root) {
            // The oracle's `get` is an exact HashMap lookup; uppercasing
            // keys at merge time reproduces the case-insensitive env
            // semantics both `std::env::var` (Windows) and figment
            // provide — and keeps the env-over-file precedence exact.
            let mut merged: HashMap<String, String> = env_file_pairs(&file)
                .into_iter()
                .map(|(k, v)| (k.to_uppercase(), v))
                .collect();
            merged.extend(
                Config::process_env_pairs().map(|(k, v)| (k.to_uppercase(), v)),
            );
            let legacy = legacy_load(&|k| merged.get(k).cloned());
            let fig = Config::figment_for(Some(&file), Config::process_env_pairs());
            let new = Config::extract(&fig).unwrap();
            let diff = config_diff(&legacy, &new);
            assert!(
                diff.is_empty(),
                "figment/legacy semantic diff on real ENV file: {diff:?}"
            );
        }
    }
}
