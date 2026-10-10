//! Allbright IOO — Independent Opportunity Observatory.
//!
//! Lineage: Artemis observer pattern + Allbright original.
//!
//! The observatory watches the raw ingest feed and records the widest
//! practical opportunity envelope WITHOUT applying the solver's profit
//! threshold. It is the independent denominator of truth: "no qualified
//! opportunity" must remain distinguishable from "no market opportunity."
//!
//! It never decides whether Allbright should trade.

use std::collections::HashMap;
use std::net::SocketAddr;

use tokio::sync::Mutex;
use std::sync::Arc;
use tracing::info;

use allbright_core::{
    bus::{BusClient, BusServer},
    config::Config,
    events::{now_ms, Event, ObservedOpportunity, PoolState, SystemEvent, UniverseStats},
};

/// Noise floor for a recorded spread — deliberately far below any
/// strategy profit threshold; this is observation, not qualification.
const NOISE_FLOOR_PCT: f64 = 0.03;
/// Per-(chain,pair,venue-pair) emission cooldown so repeated identical
/// spreads don't flood the journal.
const EMIT_COOLDOWN_MS: u64 = 60_000;

struct State {
    pools: HashMap<String, PoolState>,          // pool addr → state
    /// (chain_id, pair key) → pool addrs — venue lookup is O(same-pair)
    /// not O(universe); pair_key is computed once at insert, not per scan.
    pair_index: HashMap<(u64, String), Vec<String>>,
    cex_usd: HashMap<String, f64>,              // "BTC" → usd mid
    last_emit: HashMap<String, u64>,            // dedupe key → ts
    spreads_emitted: u64,
    max_spread_pct: f64,
}

/// Sorted "addrA/addrB" — venue-independent pair identity.
fn pair_key(p: &PoolState) -> String {
    let (a, b) = (&p.token0.address, &p.token1.address);
    let (a, b) = if a <= b { (a, b) } else { (b, a) };
    format!("{a}/{b}")
}

/// Signal-level USD price for a token symbol from the CEX mid map.
/// Stablecoins → 1.0; wrapped natives → their CEX proxy; unknown → 0.
fn token_usd(symbol: &str, cex: &HashMap<String, f64>) -> f64 {
    let s = symbol.to_uppercase();
    match s.as_str() {
        "USDC" | "USDT" | "DAI" | "FDUSD" | "BUSD" | "USDE" | "USDS" | "USD1" | "PYUSD" => 1.0,
        _ => {
            let base = s
                .strip_prefix('W').unwrap_or(&s)
                .trim_end_matches(".E");
            cex.get(base).copied().unwrap_or(0.0)
        }
    }
}

/// Rough shallow-side depth of a V2-style pool in USD.
fn depth_usd(p: &PoolState, cex: &HashMap<String, f64>) -> f64 {
    let a = p.reserve0 * token_usd(&p.token0.symbol, cex);
    let b = p.reserve1 * token_usd(&p.token1.symbol, cex);
    a.min(b)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env()
            .add_directive("allbright=info".parse().unwrap()))
        .init();
    allbright_core::init_crypto_provider();

    let cfg = Config::load()?;
    info!(config = ?cfg.redacted_summary(), "allbright-observatory starting");

    let addr: SocketAddr = ([127, 0, 0, 1], cfg.ports.observatory).into();
    let bus = Arc::new(BusServer::bind(addr).await?);
    info!(%addr, "observatory bus listening");
    let _ = bus.publish(&Event::System(SystemEvent {
        component: "observatory".into(),
        action: "startup".into(),
        detail: "process boot".into(),
        ts_ms: now_ms(),
    }));

    let state = Arc::new(Mutex::new(State {
        pools: HashMap::new(),
        pair_index: HashMap::new(),
        cex_usd: HashMap::new(),
        last_emit: HashMap::new(),
        spreads_emitted: 0,
        max_spread_pct: 0.0,
    }));

    // Ingest feed :3101 — the only input; no solver dependency.
    let ingest_addr = format!("127.0.0.1:{}", cfg.ports.ingest);
    {
        let state = state.clone();
        let bus = bus.clone();
        tokio::spawn(async move {
            loop {
                let mut cli = BusClient::connect_retry(&ingest_addr).await;
                while let Some(ev) = cli.incoming.recv().await {
                    match ev {
                        Event::PoolUpdate(p) => {
                            observe(&state, &bus, p).await;
                        }
                        Event::CexTick(t) => {
                            let mut s = state.lock().await;
                            let sym = t.symbol.trim_end_matches("USDT").to_string();
                            s.cex_usd.insert(sym, (t.bid + t.ask) / 2.0);
                        }
                        _ => {}
                    }
                }
                tracing::warn!("ingest feed dropped; reconnecting");
            }
        });
    }

    // Periodic universe measure.
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            let mut s = state.lock().await;
            let mut pair_groups: HashMap<(u64, String), usize> = HashMap::new();
            for p in s.pools.values() {
                *pair_groups
                    .entry((p.chain.chain_id(), pair_key(p)))
                    .or_default() += 1;
            }
            let observed_pairs = pair_groups.values().filter(|&&n| n >= 2).count() as u64;
            let stats = UniverseStats {
                observed_pairs,
                spreads_above_floor: s.spreads_emitted,
                max_spread_pct: s.max_spread_pct,
                ts_ms: now_ms(),
            };
            s.spreads_emitted = 0;
            s.max_spread_pct = 0.0;
            drop(s);
            let _ = bus.publish(&Event::Universe(stats));
        }
    });

    tokio::signal::ctrl_c().await.ok();
    Ok(())
}

/// On each pool update, recompute the pair's cross-venue spread.
async fn observe(state: &Arc<Mutex<State>>, bus: &Arc<BusServer>, p: PoolState) {
    let mut s = state.lock().await;
    let chain = p.chain;
    let key = pair_key(&p);
    if !s.pools.contains_key(&p.address) {
        s.pair_index
            .entry((chain.chain_id(), key.clone()))
            .or_default()
            .push(p.address.clone());
    }
    s.pools.insert(p.address.clone(), p.clone());

    // All live venues quoting this pair on this chain — index lookup,
    // not a full-universe scan.
    let addrs = s
        .pair_index
        .get(&(chain.chain_id(), key.clone()))
        .cloned()
        .unwrap_or_default();
    let venues: Vec<(String, f64, f64)> = addrs
        .iter()
        .filter_map(|a| s.pools.get(a))
        .map(|x| (x.venue.clone(), x.spot_price(), depth_usd(x, &s.cex_usd)))
        .filter(|(_, px, _)| *px > 0.0)
        .collect();
    if venues.len() < 2 {
        return;
    }
    let min = venues.iter().map(|v| v.1).fold(f64::INFINITY, f64::min);
    let max = venues.iter().map(|v| v.1).fold(0.0, f64::max);
    let spread_pct = if min > 0.0 { (max / min - 1.0) * 100.0 } else { 0.0 };
    if spread_pct < NOISE_FLOOR_PCT {
        return;
    }
    let (buy_venue, buy_depth) = {
        let v = venues.iter().min_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
        (v.0.clone(), v.2)
    };
    let sell_venue = venues.iter().max_by(|a, b| a.1.total_cmp(&b.1)).unwrap().0.clone();

    let dedupe = format!("{}:{key}:{buy_venue}:{sell_venue}", chain.name());
    let now = now_ms();
    if s.last_emit.get(&dedupe).is_some_and(|t| now - *t < EMIT_COOLDOWN_MS) {
        return;
    }
    s.last_emit.insert(dedupe, now);
    s.spreads_emitted += 1;
    s.max_spread_pct = s.max_spread_pct.max(spread_pct);
    let ev = Event::Observed(ObservedOpportunity {
        chain,
        pair: key,
        buy_venue,
        sell_venue,
        spread_pct,
        est_depth_usd: buy_depth,
        ts_ms: now,
    });
    drop(s);
    let _ = bus.publish(&ev);
}
