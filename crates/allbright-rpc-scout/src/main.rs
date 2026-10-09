//! allbright-rpc-scout — RPC discovery & qualification worker.
//!
//! Implements AGENTS §36 (200+ live RPC infrastructure) and §37
//! (Opportunity Visibility Doctrine). Sole writer of `rpc_registry.json`;
//! publishes sweep summaries on the bus (:3109) so qualification evidence
//! is journaled by ingest.
//!
//! Pipeline per AGENTS §36:
//!   discover (multiple independent sources)
//!     → live JSON-RPC probe → chain-ID validation → method capability
//!     → block/state freshness → market-visibility leg (sample pool read)
//!     → QUALIFIED / DEGRADED / RATE_LIMITED / STALE / BROKEN
//!
//! Scout can certify up to V4 (market visible). V5/V6 — opportunity and
//! execution visibility — are business-level verdicts granted only by the
//! downstream engine actually consuming the endpoint (§37.3).

use allbright_core::{
    bus::BusServer,
    chain,
    config::Config,
    events::{Chain, DexKind, Event},
    lineage::{Component, FailureClass, FailureEvent},
    rpc_registry::*,
};
use futures_util::stream::{self, StreamExt};
use serde_json::json;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;
use std::time::{Duration, Instant};
use tracing::{info, warn};

/// Public registry aggregating thousands of independent RPC listings.
/// chainlist.org/rpcs.json — NOT chainid.network/chains.json: the latter
/// encodes `rpc` as plain strings, the former as `{"url": ...}` objects
/// (the schema `discover` parses).
const CHAINLIST_URL: &str = "https://chainlist.org/rpcs.json";
/// Second independent source — Pocket's keyless public gateway slugs
/// (§36.1.A). Only our enabled chains are seeded.
const POCKET_SLUGS: &[(Chain, &str)] = &[
    (Chain::Eth, "eth"),
    (Chain::Bsc, "bsc"),
    (Chain::Arb, "arb-one"),
    (Chain::Base, "base"),
    (Chain::Avax, "avax"),
    (Chain::Op, "op"),
    (Chain::Poly, "poly"),
    (Chain::Linea, "linea"),
    (Chain::Scroll, "scroll"),
];

const PROBE_TIMEOUT: Duration = Duration::from_secs(7);
const CONCURRENCY: usize = 64;
/// Head older than this ⇒ stale state, not a healthy endpoint.
const MAX_HEAD_LAG_SECS: u64 = 300;
/// Multicall3 — deployed identically on every enabled chain; used as the
/// method-capability and event-visibility probe target.
const MULTICALL3: &str = "0xcA11bde05977b3631167028862bE2a173976CA11";

async fn rpc_call(
    client: &reqwest::Client,
    url: &str,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
    let resp = client
        .post(url)
        .json(&body)
        .timeout(PROBE_TIMEOUT)
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                "timeout".into()
            } else {
                e.to_string()
            }
        })?;
    let status = resp.status();
    if status.as_u16() == 429 {
        return Err("429 rate limited".into());
    }
    if !status.is_success() {
        return Err(format!("http {status}"));
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    if let Some(err) = v.get("error") {
        return Err(format!(
            "rpc error {}",
            err.get("code").and_then(|c| c.as_i64()).unwrap_or(0)
        ));
    }
    Ok(v.get("result").cloned().unwrap_or(serde_json::Value::Null))
}

fn hex_u64(v: &serde_json::Value) -> Option<u64> {
    v.as_str()
        .and_then(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).ok())
}

/// Full qualification probe for one candidate endpoint (§36.5 ladder).
async fn qualify(client: &reqwest::Client, mut rec: RpcEndpointRecord) -> RpcEndpointRecord {
    rec.probes += 1;
    rec.last_probe_ms = allbright_core::events::now_ms();
    let expected = rec.chain.chain_id();
    let url = rec.url.clone();

    // Ladder step: JSON-RPC + chain-ID validation.
    let t0 = Instant::now();
    match rpc_call(client, &url, "eth_chainId", json!([])).await {
        Ok(v) => {
            rec.push_rtt(t0.elapsed().as_millis() as u64);
            rec.visibility = VisibilityClass::V1Connected;
            rec.state = QualState::Live;
            if hex_u64(&v) == Some(expected) {
                rec.caps.chain_id = CapState::Available;
                rec.visibility = VisibilityClass::V2ChainVisible;
                rec.state = QualState::ChainVerified;
            } else {
                // Wrong chain ID — misconfigured endpoint, never usable.
                rec.caps.chain_id = CapState::Unavailable;
                rec.state = QualState::Quarantined;
                rec.last_error = Some(format!("wrong chainId {v} expected {expected:#x}"));
                rec.errors += 1;
                rec.compute_ovs();
                return rec;
            }
        }
        Err(e) => {
            rec.errors += 1;
            rec.consecutive_errors += 1;
            rec.last_error = Some(e.clone());
            rec.state = if e.contains("429") {
                QualState::RateLimited
            } else {
                QualState::Broken
            };
            rec.visibility = VisibilityClass::V0Dead;
            rec.compute_ovs();
            return rec;
        }
    }

    // Latest block.
    let head = match rpc_call(client, &url, "eth_blockNumber", json!([])).await {
        Ok(v) => {
            rec.caps.latest_block = CapState::Available;
            hex_u64(&v)
        }
        Err(e) => {
            rec.last_error = Some(e);
            rec.state = QualState::Degraded;
            rec.compute_ovs();
            return rec;
        }
    };

    // Block freshness via eth_getBlockByNumber timestamp.
    if let Ok(v) = rpc_call(
        client,
        &url,
        "eth_getBlockByNumber",
        json!(["latest", false]),
    )
    .await
    {
        if let Some(ts) = v.get("timestamp").and_then(hex_u64) {
            let now = allbright_core::events::now_ms() / 1000;
            let lag = now.saturating_sub(ts);
            rec.head_lag_secs = Some(lag);
            rec.caps.block_fresh = if lag <= MAX_HEAD_LAG_SECS {
                CapState::Available
            } else {
                CapState::Unavailable
            };
        }
    }

    // Method capability: eth_call against canonical Multicall3.
    match rpc_call(
        client,
        &url,
        "eth_call",
        json!([{"to": MULTICALL3, "data": "0x0f28c97d"}, "latest"]),
    )
    .await
    {
        Ok(v) if v.as_str().map(|s| s.len() > 2).unwrap_or(false) => {
            rec.caps.eth_call = CapState::Available;
            rec.visibility = VisibilityClass::V3StateVisible;
            rec.state = QualState::MethodVerified;
        }
        Ok(_) => rec.caps.eth_call = CapState::Partial,
        Err(_) => rec.caps.eth_call = CapState::Unavailable,
    }

    // Balance read.
    if rpc_call(
        client,
        &url,
        "eth_getBalance",
        json!([MULTICALL3, "latest"]),
    )
    .await
    .is_ok()
    {
        rec.caps.get_balance = CapState::Available;
    }

    // Event visibility: bounded eth_getLogs (last 32 blocks, any logs at
    // the Multicall3 address — presence is not required, *capability* is:
    // a successful empty result proves the method is not restricted).
    if let Some(h) = head {
        let from = format!("{:#x}", h.saturating_sub(32));
        let to = format!("{h:#x}");
        match rpc_call(
            client,
            &url,
            "eth_getLogs",
            json!([{"fromBlock": from, "toBlock": to, "address": MULTICALL3}]),
        )
        .await
        {
            Ok(_) => rec.caps.get_logs = CapState::Available,
            Err(e) => {
                rec.caps.get_logs = if e.contains("429") {
                    CapState::Unknown
                } else {
                    CapState::Unavailable
                };
            }
        }
    }

    // Two extra RTT samples for a latency distribution leg.
    for _ in 0..2 {
        let t = Instant::now();
        if rpc_call(client, &url, "eth_chainId", json!([]))
            .await
            .is_ok()
        {
            rec.push_rtt(t.elapsed().as_millis() as u64);
        }
    }

    // Verdict: QUALIFIED requires the state-visibility floor; market
    // visibility (V4) is upgraded by the separate market pass below.
    let base_ok = rec.caps.chain_id == CapState::Available
        && rec.caps.latest_block == CapState::Available
        && rec.caps.block_fresh == CapState::Available
        && rec.caps.eth_call == CapState::Available;
    rec.state = if base_ok && rec.median_rtt().map(|r| r < 1500).unwrap_or(false) {
        QualState::Qualified
    } else if base_ok {
        QualState::Degraded // correct but slow
    } else if rec.caps.block_fresh == CapState::Unavailable {
        QualState::Stale
    } else {
        QualState::Degraded
    };
    rec.consecutive_errors = 0;
    rec.compute_ovs();
    rec
}

/// Market-visibility leg (§37.4): read a real pool's state through the
/// endpoint. V2/Stable → `getReserves()` (0x0902f1ac); V3 → `slot0()`
/// (0x3850c98b). Success upgrades the record to V4.
///
/// §37.1 enforcement: RPC ALIVE ≠ RPC OPPORTUNITY-VISIBLE. An endpoint that
/// passes the V3 floor but cannot read pool state is economically blind and
/// must not hold `Qualified` — it is demoted to `Degraded` (basic/fallback
/// tier) and the blindness is recorded in `last_error`.
async fn market_probe(
    client: &reqwest::Client,
    rec: &mut RpcEndpointRecord,
    pool: &chain::PoolDescriptor,
) {
    let selector = match pool.dex {
        DexKind::V3 => "0x3850c98b",
        _ => "0x0902f1ac",
    };
    match rpc_call(
        client,
        &rec.url,
        "eth_call",
        json!([{"to": pool.address, "data": selector}, "latest"]),
    )
    .await
    {
        Ok(v) if v.as_str().map(|s| s.len() > 2).unwrap_or(false) => {
            rec.caps.pool_state = CapState::Available;
            rec.visibility = VisibilityClass::V4MarketVisible;
        }
        Ok(_) | Err(_) => {
            rec.caps.pool_state = CapState::Unavailable;
            if rec.state == QualState::Qualified {
                rec.state = QualState::Degraded;
            }
            rec.last_error = Some(
                "economically blind: pool state unreadable — fallback tier only (§37.1)".into(),
            );
        }
    }
    rec.compute_ovs();
}

/// Discovery — multiple independent sources (§36.1). Returns deduplicated
/// candidate records; templated `{KEY}` URLs are recorded as NeedsKey
/// (counted, never probed — no static trust).
async fn discover(
    client: &reqwest::Client,
    cfg: &Config,
    registry: &RpcRegistry,
) -> Vec<RpcEndpointRecord> {
    let mut out: Vec<RpcEndpointRecord> = Vec::new();
    let wanted: HashMap<u64, Chain> = Chain::ALL.iter().map(|c| (c.chain_id(), *c)).collect();

    // Source A: Chainlist-style registry (aggregates many providers).
    match client
        .get(CHAINLIST_URL)
        .timeout(Duration::from_secs(30))
        .send()
        .await
    {
        Ok(r) => match r.json::<serde_json::Value>().await {
            Ok(v) => {
                let mut n = 0usize;
                for entry in v.as_array().cloned().unwrap_or_default() {
                    let Some(cid) = entry.get("chainId").and_then(|c| c.as_u64()) else {
                        continue;
                    };
                    let Some(chain) = wanted.get(&cid) else {
                        continue;
                    };
                    for r in entry
                        .get("rpc")
                        .and_then(|r| r.as_array())
                        .cloned()
                        .unwrap_or_default()
                    {
                        let url = r.get("url").and_then(|u| u.as_str()).unwrap_or("");
                        if url.is_empty() || !url.starts_with("http") {
                            continue; // wss endpoints tracked separately
                        }
                        let mut rec = RpcEndpointRecord::new(url, *chain, "chainlist");
                        if url.contains('{') {
                            rec.state = QualState::NeedsKey;
                        }
                        out.push(rec);
                        n += 1;
                    }
                }
                info!(candidates = n, "chainlist discovery");
            }
            Err(e) => warn!(error = %e, "chainlist parse failed"),
        },
        Err(e) => warn!(error = %e, "chainlist fetch failed"),
    }

    // Source B: Pocket keyless gateway (independent infrastructure).
    for (chain, slug) in POCKET_SLUGS {
        out.push(RpcEndpointRecord::new(
            &format!("https://{slug}.rpc.grove.city/v1/lb/public"),
            *chain,
            "pocket",
        ));
    }

    // Source C: configured ENV pools — operator-curated, already verified.
    for (chain, urls) in &cfg.rpc_pools {
        for u in urls {
            out.push(RpcEndpointRecord::new(u, *chain, "env"));
        }
    }

    // Carry-forward prior discoveries so the universe persists across
    // sweeps even when a source is temporarily unreachable.
    for e in &registry.endpoints {
        out.push(e.clone());
    }

    // Dedupe by (url, chain).
    let mut seen = std::collections::HashSet::new();
    out.retain(|r| seen.insert((r.url.clone(), r.chain)));
    out
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    allbright_core::init_crypto_provider();
    let cfg = Config::load()?;
    let reg_path = std::env::var("RPC_REGISTRY_PATH").unwrap_or_else(|_| REGISTRY_FILE.to_string());
    let interval = Duration::from_secs(
        std::env::var("RPC_SCOUT_INTERVAL_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1800),
    );

    let bus = BusServer::bind(SocketAddr::from(([127, 0, 0, 1], cfg.ports.scout))).await?;
    info!(port = cfg.ports.scout, "rpc-scout bus listening");

    let client = reqwest::Client::builder()
        .timeout(PROBE_TIMEOUT)
        .pool_max_idle_per_host(0)
        .user_agent("allbright-rpc-scout/0.1")
        .build()?;

    loop {
        let started = Instant::now();
        let mut registry = RpcRegistry::load(Path::new(&reg_path));

        // ---- DISCOVER ----
        let candidates = discover(&client, &cfg, &registry).await;
        let probeable: Vec<RpcEndpointRecord> = candidates
            .into_iter()
            .filter(|r| r.state != QualState::NeedsKey)
            .collect();
        info!(candidates = probeable.len(), "qualification sweep starting");

        // ---- QUALIFY (bounded concurrency) ----
        let results: Vec<RpcEndpointRecord> = stream::iter(probeable)
            .map(|rec| qualify(&client, rec))
            .buffer_unordered(CONCURRENCY)
            .collect()
            .await;
        for rec in results {
            registry.upsert(rec);
        }

        // ---- MARKET VISIBILITY PASS (§37.4 — every live candidate, not top-N) ----
        // QUALIFIED requires V4 market visibility: a real pool read through the
        // endpoint. Endpoints that fail are demoted to Degraded (economically
        // blind — fallback tier only, §37.1).
        let pools = chain::load_pools();
        let by_chain = chain::pools_by_chain(&pools);
        for chain in Chain::ALL {
            let Some(sample) = by_chain.get(&chain).and_then(|p| p.first()).cloned() else {
                continue;
            };
            let targets: Vec<RpcEndpointRecord> = registry
                .for_chain(chain)
                .filter(|e| matches!(e.state, QualState::Qualified | QualState::Degraded))
                .cloned()
                .collect();
            let probed: Vec<RpcEndpointRecord> = stream::iter(targets)
                .map(|mut rec| {
                    let client = client.clone();
                    let sample = sample.clone();
                    async move {
                        market_probe(&client, &mut rec, &sample).await;
                        rec
                    }
                })
                .buffer_unordered(CONCURRENCY)
                .collect()
                .await;
            for rec in probed {
                registry.upsert(rec);
            }
        }

        if let Err(e) = registry.save(Path::new(&reg_path)) {
            warn!(error = %e, "registry save failed");
        }
        let summary = registry.summary();
        info!(total = summary.total, live = summary.live,
              qualified = summary.qualified, providers = summary.providers,
              elapsed = ?started.elapsed(), "sweep complete");
        let _ = bus.publish(&Event::RpcScan(summary.clone()));

        // Chains with zero qualified endpoints are blind — classified,
        // journaled, never silent (§37.6/§37.12). Zero V4-visible endpoints
        // while some remain "alive" is RPC_OPPORTUNITY_BLIND — distinct from
        // total connectivity failure.
        for chain in Chain::ALL {
            let (q, _) = registry.chain_diversity(chain);
            if !cfg.rpc_pools.contains_key(&chain) {
                continue;
            }
            if q == 0 {
                let _ = bus.publish(&Event::Failure(
                    FailureEvent::new(
                        Component::RpcScout,
                        FailureClass::RpcChainVisibilityFailure,
                        "scout.sweep",
                        format!("{}: zero qualified RPC endpoints after sweep", chain.name()),
                    )
                    .on_chain(chain),
                ));
            } else {
                let v4 = registry
                    .for_chain(chain)
                    .filter(|e| {
                        e.state == QualState::Qualified
                            && e.visibility == VisibilityClass::V4MarketVisible
                    })
                    .count();
                if v4 == 0 {
                    let _ = bus.publish(&Event::Failure(FailureEvent::new(
                        Component::RpcScout,
                        FailureClass::RpcOpportunityBlind,
                        "scout.market_probe",
                        format!("{}: endpoints alive but zero V4 market visibility — economically blind", chain.name()),
                    ).on_chain(chain)));
                }
            }
        }

        tokio::time::sleep(interval).await;
    }
}
