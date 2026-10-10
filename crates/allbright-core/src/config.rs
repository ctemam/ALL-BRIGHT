//! Configuration contract — loads the `ENV` file. Secrets are held behind
//! `SecretString` and are never logged, serialized, or telemetried.

use secrecy::{ExposeSecret, SecretString};
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;

use crate::events::Chain;

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

/// Config is deliberately non-Serializable: secrets must never be written
/// to disk, telemetry, or logs.
impl Config {
    pub fn load() -> anyhow::Result<Self> {
        // Prefer project ENV file; fall back to process environment.
        for cand in ["ENV", ".env", "env"] {
            if Path::new(cand).exists() {
                let _ = dotenvy::from_filename(cand);
                break;
            }
        }

        let get = |k: &str| std::env::var(k).ok();
        let secret = |k: &str| get(k).filter(|s| !s.is_empty()).map(SecretString::from);

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
        const PRIVATE_PROVIDERS: &[(&str, &str, &[(&str, &str)])] = &[
            (
                "ALCHEMY_API_KEY",
                "https://{slug}.g.alchemy.com/v2/{key}",
                &[
                    ("eth", "eth-mainnet"), ("arb", "arb-mainnet"),
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
                    ("eth", "mainnet"), ("arb", "arbitrum-mainnet"),
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
        ];
        let mut pending_providers: Vec<String> = Vec::new();
        for (env_key, tpl, nets) in PRIVATE_PROVIDERS {
            let Some(key) = get(env_key).filter(|s| !s.is_empty()) else {
                pending_providers
                    .push(env_key.trim_end_matches("_API_KEY").trim_end_matches("_KEY").to_string());
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

        Ok(Config {
            rpc_urls,
            wss_urls,
            rpc_pools,
            pending_providers,
            active_chains,
            strategies,
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
        })
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
