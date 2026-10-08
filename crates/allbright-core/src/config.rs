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
    /// This is the failover substrate — a chain with len()==1 is
    /// single-homed (AGENTS §32/§35 wiring gap).
    pub rpc_pools: HashMap<Chain, Vec<String>>,
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

        Ok(Config {
            rpc_urls,
            wss_urls,
            rpc_pools,
            velora_api_key: secret("VELORA_API_KEY"),
            flashbots_relay: get("FLASHBOOTS_RELAY")
                .unwrap_or_else(|| "https://relay.flashbots.net".into()),
            flashbots_auth_key: secret("FLASHBOOTS_AUTH_KEY"),
            pimlico_api_key: secret("PIMLICO_API_KEY"),
            zerodev_project_id: secret("ZERODEV_PROJECT_ID"),
            private_key: secret("PRIVATE_KEY"),
            profit_wallet: get("PROFIT_WALLET").unwrap_or_default(),
            native_usd: get("ZCA_NATIVE_USD")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.0),
            profit_transfer_mode: get("PROFIT_TRANSFER_MODE")
                .unwrap_or_else(|| "MANUAL".into()),
            profit_transfer_min_usd: get("PROFIT_TRANSFER_MIN_USD")
                .and_then(|s| s.parse().ok())
                .unwrap_or(100.0),
            auto_execute: get("ZCA_AUTO_EXECUTE")
                .map(|s| s.eq_ignore_ascii_case("true"))
                .unwrap_or(false),
            min_profit_usd: get("ZCA_MIN_PROFIT_USD")
                .and_then(|s| s.parse().ok())
                .unwrap_or(5.0),
            scan_interval_secs: get("ZCA_SCAN_INTERVAL_SECS")
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
    pub velora: bool,
    pub flashbots_auth: bool,
    pub pimlico: bool,
    pub zerodev: bool,
    pub private_key: bool,
    pub auto_execute: bool,
    pub min_profit_usd: f64,
    pub scan_interval_secs: u64,
}
