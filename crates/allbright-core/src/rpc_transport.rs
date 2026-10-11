//! Resilient RPC transport (GAP-10) — an alloy `Provider` backed by
//! `FallbackLayer` + `RetryBackoffLayer` over a chain's ordered endpoint
//! pool.
//!
//! ## Adoption
//!
//! Call sites that currently pin a *single* endpoint can swap their
//! construction line for [`resilient_provider`] wherever an ordered URL
//! list is already in hand — `Config::rpc_pools[chain]` (private keyed
//! endpoints first, §36.9) or `RpcRegistry::qualified_urls(chain)` /
//! `fallback_urls(chain)`:
//!
//! - `allbright-ingest::evm_stream` — `ProviderBuilder::new().connect_http(url)`
//! - `allbright-executor` (`relay.rs`, `aa.rs`) — raw reqwest JSON-RPC;
//!   adopt by routing `eth_*` calls through the returned `Provider`
//! - `allbright-challenger` / `allbright-rpc-scout` — live `eth_gasPrice`,
//!   qualification probes
//!
//! The returned [`DynProvider`] satisfies the same
//! `alloy::providers::Provider` call surface as `connect_http`, so
//! adoption is a construction-site-only change. Notes for adopters:
//!
//! - `FallbackLayer` *races* the top `active_transports` endpoints per
//!   call and continuously re-ranks them on measured latency/stability —
//!   hot-path reads get the fastest healthy endpoint without operator
//!   ordering. Non-deterministic methods (`eth_sendRawTransaction`,
//!   `eth_call` at `pending`, …) are executed sequentially by default.
//! - `RetryBackoffLayer` sits *above* the fan-out: a pool-wide 429 /
//!   rate-limit backoff retries the call instead of surfacing an error.
//! - `wss://` endpoints are skipped — pubsub (`newHeads`) still needs
//!   its own `connect_ws` transport; this layer is call-path only.
//! - URL parse failures are fatal and name the offending endpoint
//!   **host** — never the full URL, which may embed API-key material
//!   (§36.9 secrets boundary).

use std::num::NonZeroUsize;

use alloy::{
    providers::{DynProvider, Provider, ProviderBuilder},
    rpc::client::RpcClient,
    transports::{
        http::{reqwest::Url, Http},
        layers::{FallbackLayer, RetryBackoffLayer},
    },
};
use anyhow::{bail, Context as _};
use tower::ServiceBuilder;

/// Tunables for [`resilient_provider_with`].
#[derive(Debug, Clone)]
pub struct ResilientTransportConfig {
    /// How many pool endpoints `FallbackLayer` races in parallel
    /// (the rest stay in reserve, promoted as actives degrade).
    pub active_transports: usize,
    /// Rate-limit retries before a call fails to the caller.
    pub max_rate_limit_retries: u32,
    /// Initial exponential backoff for rate-limit retries (ms).
    pub initial_backoff_ms: u64,
    /// Compute-units-per-second budget the pool may spend
    /// (`RetryBackoffLayer` pacing against per-key quota, §36.7).
    pub compute_units_per_sec: u64,
}

impl Default for ResilientTransportConfig {
    fn default() -> Self {
        Self {
            active_transports: 3,
            max_rate_limit_retries: 4,
            initial_backoff_ms: 100,
            compute_units_per_sec: 600,
        }
    }
}

/// Build a resilient provider over `urls` (ordered by preference —
/// `Config::rpc_pools` order is preserved: private keyed capacity first).
pub fn resilient_provider(urls: &[String]) -> anyhow::Result<DynProvider> {
    resilient_provider_with(urls, &ResilientTransportConfig::default())
}

/// [`resilient_provider`] with explicit transport tunables.
pub fn resilient_provider_with(
    urls: &[String],
    cfg: &ResilientTransportConfig,
) -> anyhow::Result<DynProvider> {
    let mut transports = Vec::with_capacity(urls.len());
    for raw in urls {
        let url: Url = raw.parse().with_context(|| {
            format!(
                "invalid RPC endpoint URL (host: {})",
                crate::rpc_registry::host_of(raw)
            )
        })?;
        match url.scheme() {
            "http" | "https" => transports.push(Http::new(url)),
            // Pubsub endpoints are call-path-incapable; skip silently —
            // presence of a wss:// URL in a pool is normal (§32).
            _ => continue,
        }
    }
    if transports.is_empty() {
        bail!("resilient_provider: pool has no http(s) endpoints");
    }

    let fallback = FallbackLayer::default().with_active_transport_count(
        NonZeroUsize::new(cfg.active_transports.max(1)).expect("clamped to >= 1"),
    );
    let retry = RetryBackoffLayer::new(
        cfg.max_rate_limit_retries,
        cfg.initial_backoff_ms,
        cfg.compute_units_per_sec,
    );
    // retry ∘ fallback: if every raced endpoint rate-limits or drops,
    // the whole fan-out is retried with backoff — a saturated provider
    // never surfaces as an application error while capacity exists.
    let service = ServiceBuilder::new()
        .layer(retry)
        .layer(fallback)
        .service(transports);
    let client = RpcClient::builder().transport(service, false);
    Ok(ProviderBuilder::new().connect_client(client).erased())
}
