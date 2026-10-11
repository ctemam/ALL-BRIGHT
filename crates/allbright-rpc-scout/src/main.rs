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
    events::{now_ms, Chain, DexKind, Event, SystemEvent},
    lineage::{Component, FailureClass, FailureEvent},
    rpc_registry::*,
};
use futures_util::stream::{self, StreamExt};
use serde_json::json;
use throttle_net::{AdaptiveLimiter, Aimd};
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
const CONCURRENCY: usize = 24;
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
    rec.probes = rec.probes.saturating_add(1);
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
                rec.errors = rec.errors.saturating_add(1);
                rec.compute_ovs();
                return rec;
            }
        }
        Err(e) => {
            rec.errors = rec.errors.saturating_add(1);
            rec.consecutive_errors = rec.consecutive_errors.saturating_add(1);
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

/// One JSON-RPC call over a WebSocket — send request, await the frame
/// carrying the matching id (subscription notifications are skipped).
async fn ws_rpc(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    id: u64,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;

    let req = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
    ws.send(Message::Text(req.to_string()))
        .await
        .map_err(|e| format!("ws send: {e}"))?;
    let deadline = Instant::now() + PROBE_TIMEOUT;
    while Instant::now() < deadline {
        let msg = match tokio::time::timeout(PROBE_TIMEOUT, ws.next()).await {
            Ok(Some(Ok(m))) => m,
            Ok(Some(Err(e))) => return Err(format!("ws read: {e}")),
            Ok(None) => return Err("ws closed".into()),
            Err(_) => return Err("ws read timeout".into()),
        };
        let Message::Text(t) = msg else { continue };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&t) else {
            continue;
        };
        if v.get("id").and_then(|i| i.as_u64()) != Some(id) {
            continue; // async notification or unrelated reply
        }
        if let Some(err) = v.get("error") {
            return Err(format!(
                "rpc error {}",
                err.get("code").and_then(|c| c.as_i64()).unwrap_or(0)
            ));
        }
        return Ok(v.get("result").cloned().unwrap_or(serde_json::Value::Null));
    }
    Err("ws reply window expired".into())
}

/// WSS qualification leg (GAP-34): prove the endpoint actually speaks
/// JSON-RPC over WebSocket and grants `eth_subscribe` — the capability
/// the event-driven detection layer (GAP-3) is dead without. Ladder:
///   WS connect → eth_chainId (chain match) → eth_subscribe newHeads
///   (subscription id granted).
/// First-head delivery is NOT awaited — a granted subscription id is the
/// measured capability; delivery health is observed by the consumer.
async fn wss_probe(rec: &mut RpcEndpointRecord) {
    use tokio_tungstenite::connect_async;

    rec.probes = rec.probes.saturating_add(1);
    rec.last_probe_ms = now_ms();
    let t0 = Instant::now();

    let mut ws = match tokio::time::timeout(PROBE_TIMEOUT, connect_async(&rec.url)).await {
        Ok(Ok((s, _))) => s,
        Ok(Err(e)) => {
            rec.errors = rec.errors.saturating_add(1);
            rec.consecutive_errors = rec.consecutive_errors.saturating_add(1);
            rec.last_error = Some(format!("ws connect: {e}"));
            rec.state = QualState::Broken;
            rec.caps.websocket = CapState::Unavailable;
            rec.compute_ovs();
            return;
        }
        Err(_) => {
            rec.errors = rec.errors.saturating_add(1);
            rec.consecutive_errors = rec.consecutive_errors.saturating_add(1);
            rec.last_error = Some("ws connect timeout".into());
            rec.state = QualState::Broken;
            rec.caps.websocket = CapState::Unavailable;
            rec.compute_ovs();
            return;
        }
    };
    rec.push_rtt(t0.elapsed().as_millis() as u64);
    rec.visibility = VisibilityClass::V1Connected;
    rec.state = QualState::Live;

    // Chain-ID validation over the WS transport.
    match ws_rpc(&mut ws, 1, "eth_chainId", json!([])).await {
        Ok(v) if hex_u64(&v) == Some(rec.chain.chain_id()) => {
            rec.caps.chain_id = CapState::Available;
            rec.visibility = VisibilityClass::V2ChainVisible;
            rec.state = QualState::ChainVerified;
        }
        Ok(v) => {
            rec.caps.chain_id = CapState::Unavailable;
            rec.state = QualState::Quarantined;
            rec.last_error = Some(format!(
                "ws wrong chainId {v} expected {:#x}",
                rec.chain.chain_id()
            ));
            rec.errors = rec.errors.saturating_add(1);
            rec.compute_ovs();
            return;
        }
        Err(e) => {
            rec.errors = rec.errors.saturating_add(1);
            rec.consecutive_errors = rec.consecutive_errors.saturating_add(1);
            rec.last_error = Some(format!("ws chainId: {e}"));
            rec.state = QualState::Broken;
            rec.compute_ovs();
            return;
        }
    }

    // The capability that matters: eth_subscribe grant.
    match ws_rpc(&mut ws, 2, "eth_subscribe", json!(["newHeads"])).await {
        Ok(v) if v.as_str().map(|s| s.starts_with("0x")).unwrap_or(false) => {
            rec.caps.websocket = CapState::Available;
            rec.state = if rec.median_rtt().map(|r| r < 1500).unwrap_or(false) {
                QualState::Qualified
            } else {
                QualState::Degraded
            };
        }
        Ok(v) => {
            rec.caps.websocket = CapState::Partial;
            rec.state = QualState::Degraded;
            rec.last_error = Some(format!("ws subscribe odd reply {v}"));
        }
        Err(e) => {
            rec.caps.websocket = CapState::Unavailable;
            rec.state = QualState::Degraded;
            rec.last_error = Some(format!("ws subscribe: {e}"));
        }
    }
    rec.consecutive_errors = 0;
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
                        if url.is_empty()
                            || !(url.starts_with("http") || url.starts_with("ws"))
                        {
                            continue;
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

    // Source D (GAP-34): derived WSS candidates — every live HTTPS
    // endpoint is a potential WS host; keyed providers expose documented
    // wss:// forms. Derived records are *candidates*: the wss_probe pass
    // below measures `eth_subscribe` capability — never static trust.
    // Derive WSS candidates from every HTTPS endpoint that is not
    // definitively dead (Broken/Quarantined/NeedsKey). Unknown-state
    // entries exist on first boot — excluding them would produce zero
    // WSS candidates to probe before the first HTTP qualification pass
    // completes. Wrong-form guesses cost one bounded probe each (7 s
    // timeout) — never a blind trust.
    let derived: Vec<RpcEndpointRecord> = out
        .iter()
        .filter(|e| {
            e.transport == "https"
                && !matches!(
                    e.state,
                    QualState::Broken | QualState::Quarantined | QualState::NeedsKey
                )
                && !e.url.contains('{')
        })
        .filter_map(|e| {
            derive_wss(&e.url).map(|w| RpcEndpointRecord::new(&w, e.chain, "derived-wss"))
        })
        .collect();
    out.extend(derived);

    // Dedupe by (url, chain).
    let mut seen = std::collections::HashSet::new();
    out.retain(|r| seen.insert((r.url.clone(), r.chain)));
    out
}

/// HTTPS → WSS candidate derivation (GAP-34). Provider-documented WS
/// forms are applied where the HTTPS path differs from the WS path —
/// wrong-form guesses cost one bounded probe each, never a blind trust.
fn derive_wss(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://")?;
    let mut w = format!("wss://{rest}");
    // Infura serves WS under /ws/: infura.io/v3/{key} → infura.io/ws/v3/{key}
    if w.contains(".infura.io/v3/") {
        w = w.replace(".infura.io/v3/", ".infura.io/ws/v3/");
    }
    // dRPC load balancer: /ogrpc (http) ↔ /ogws (websocket)
    if w.contains("lb.drpc.org/ogrpc") {
        w = w.replace("/ogrpc", "/ogws");
    }
    Some(w)
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
    let _ = bus.publish(&Event::System(SystemEvent {
        component: "rpc-scout".into(),
        action: "startup".into(),
        detail: "process boot".into(),
        ts_ms: now_ms(),
    }));

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

        // ---- QUALIFY (adaptive concurrency — GAP-17) ----
        // throttle-net AIMD limiter (prebuilt — no hand-rolled congestion
        // control): probes that hit transport failures shrink the
        // in-flight limit; responsive stretches grow it toward the
        // ceiling. A static CONCURRENCY was the PM2 kill-loop's root
        // mechanism — 64 concurrent TLS commits spiked RSS past cap.
        let limiter = AdaptiveLimiter::builder()
            .floor(4)
            .ceiling(CONCURRENCY as u32)
            .initial(12)
            .build(Aimd::default());
        let results: Vec<RpcEndpointRecord> = stream::iter(probeable)
            .map(|rec| {
                let limiter = &limiter;
                let client = &client;
                async move {
                    let permit = limiter.acquire().await;
                    let out = qualify(client, rec).await;
                    match out.state {
                        QualState::Broken | QualState::RateLimited => {
                            permit.failure();
                        }
                        _ => {
                            permit.success();
                        }
                    }
                    out
                }
            })
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
            // Skip chains with no configured RPC pool — probing them
            // doubles the pass (and its memory commit) for zero value.
            if !cfg.rpc_pools.contains_key(&chain) {
                continue;
            }
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
                    let limiter = &limiter;
                    async move {
                        let permit = limiter.acquire().await;
                        market_probe(&client, &mut rec, &sample).await;
                        match rec.state {
                            QualState::Broken | QualState::RateLimited => {
                                permit.failure();
                            }
                            _ => {
                                permit.success();
                            }
                        }
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

        // ---- WSS VISIBILITY PASS (GAP-34) ----
        // Every ws/wss record is probed for a granted `eth_subscribe` —
        // the transport the event-driven detection layer requires. A
        // chain with zero qualified WS endpoints cannot react to swaps
        // inside their own block; that blindness is classified below,
        // never assumed away.
        let wss_targets: Vec<RpcEndpointRecord> = registry
            .endpoints
            .iter()
            .filter(|e| {
                (e.transport == "wss" || e.transport == "ws")
                    && e.state != QualState::NeedsKey
                    && !e.url.contains('{')
            })
            .cloned()
            .collect();
        if !wss_targets.is_empty() {
            info!(candidates = wss_targets.len(), "wss qualification pass");
            let probed: Vec<RpcEndpointRecord> = stream::iter(wss_targets)
                .map(|mut rec| {
                    let limiter = &limiter;
                    async move {
                        let permit = limiter.acquire().await;
                        wss_probe(&mut rec).await;
                        match rec.state {
                            QualState::Broken | QualState::RateLimited => {
                                permit.failure();
                            }
                            _ => {
                                permit.success();
                            }
                        }
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

        // Merge-on-save: the sweep loaded the registry minutes ago —
        // anything written into the file meanwhile (external candidate
        // injection, a concurrent writer) must not be clobbered. Reload,
        // let this sweep's fresher records win on overlap, keep file-only
        // records so the next sweep carries them forward.
        let mut merged = RpcRegistry::load(Path::new(&reg_path));
        for rec in registry.endpoints {
            merged.upsert(rec);
        }
        if let Err(e) = merged.save(Path::new(&reg_path)) {
            warn!(error = %e, "registry save failed");
        }
        let registry = merged;
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
                // GAP-34: HTTP-V4 endpoints but zero WS-qualified = the
                // chain can read state but can never subscribe — the
                // event-driven detection layer is dead there.
                let wss_q = registry
                    .for_chain(chain)
                    .filter(|e| {
                        (e.transport == "wss" || e.transport == "ws")
                            && e.state == QualState::Qualified
                    })
                    .count();
                if wss_q == 0 {
                    let _ = bus.publish(&Event::Failure(FailureEvent::new(
                        Component::RpcScout,
                        FailureClass::RpcMethodUnavailable,
                        "scout.wss_probe",
                        format!("{}: zero WSS-qualified endpoints — event-driven visibility impossible (GAP-34)", chain.name()),
                    ).on_chain(chain)));
                }
            }
        }

        tokio::time::sleep(interval).await;
    }
}
