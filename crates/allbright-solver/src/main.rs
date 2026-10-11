//! Allbright Solver Layer
//!
//! Architectural lineage:
//! - Artemis / Rusty-Sando concepts + Allbright original optimization:
//!   pool-graph path search, optimal sizing, divergence engine.
//! - The solver emits pre-validated ExecutionRequests; the executor never
//!   recomputes routes.
//!
//! This module is an independent implementation and does not copy
//! proprietary source code.

mod arb;
mod pricing;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::Mutex;
use tracing::info;

use allbright_core::{
    bus::{BusClient, BusServer},
    config::Config,
    events::*,
    lineage::{Component, FailureClass, FailureEvent},
};

/// Shared solver state.
struct State {
    pools: HashMap<String, PoolState>,          // pool address → state
    cex: HashMap<String, CexTick>,              // CEX symbol → latest tick
    pending: HashMap<u64, PendingSim>,          // req_id → context
    req_seq: u64,
    funnel: Funnel,
    /// Per-chain live gas price (wei) + fetch time — fed by the
    /// background gas task so handle_sim_result never awaits the network
    /// while holding the state lock.
    gas_price: HashMap<Chain, (u128, Instant)>,
    /// Rate-limit for coherence-drop evidence (one StaleState event per
    /// chain per 30 s; the counter keeps the full count).
    last_coherence_emit: HashMap<Chain, Instant>,
    /// Per-CEX-symbol cooldown for divergence-triggered rescans.
    cex_trigger: HashMap<String, Instant>,
    /// Pool addresses that materially shifted since the last candidate
    /// regen per chain (P05 trigger set, accumulated across the debounce
    /// window).
    dirty_moved: HashMap<Chain, std::collections::HashSet<String>>,
    /// Candidate regeneration debounce — collectors push hundreds of
    /// updates/sec; regenerating per update burned ~200% CPU. One scan
    /// per REGEN_DEBOUNCE per chain sees all accumulated state anyway.
    last_regen: HashMap<Chain, Instant>,
    /// Bait-pool convictions (executor-written) — convicted pools are
    /// excluded from the graph entirely; reloaded every 30 s.
    bait: HashMap<Chain, allbright_core::bait::BaitRegistry>,
    bait_checked: Instant,
    /// Per-chain rate cap on EV-class ExecRequests (GAP-37) — bounds
    /// bundler/quota burn while keeping the class journaled.
    ev_last_emit: HashMap<Chain, Instant>,
    /// Per-CEX-symbol throttle for the P09 predictive scan (gap-based —
    /// runs on every tick, not just on sharp moves).
    pred_scan: HashMap<String, Instant>,
}

/// Minimum interval between candidate regenerations per chain.
const REGEN_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(400);

/// Minimum CEX mid move (basis points) that justifies an asset-scoped
/// rescan — below this the next regular pool poll is soon enough.
const CEX_MOVE_BPS: f64 = 15.0;

/// Material reserve shift (basis points): a ≥2% relative move on either
/// reserve means a real swap or LP event landed on the pool — the
/// residual imbalance it leaves is the P05 backrun window.
const RESERVE_SHIFT_BPS: f64 = 200.0;

/// True when the pool's reserves moved ≥ RESERVE_SHIFT_BPS on either side.
fn material_shift(prev: &PoolState, cur: &PoolState) -> bool {
    let rel = |a: f64, b: f64| -> f64 {
        if a <= 0.0 {
            return if b > 0.0 { f64::MAX } else { 0.0 };
        }
        ((b - a) / a).abs() * 1e4
    };
    rel(prev.reserve0, cur.reserve0) >= RESERVE_SHIFT_BPS
        || rel(prev.reserve1, cur.reserve1) >= RESERVE_SHIFT_BPS
}

/// On-chain token symbols that track a CEX symbol's base asset
/// ("ETHUSDT" → {ETH, WETH}). Quote suffixes stripped longest-first.
fn cex_asset_syms(symbol: &str) -> std::collections::HashSet<String> {
    let up = symbol.to_ascii_uppercase();
    for q in ["USDT", "USDC", "FDUSD", "BUSD", "USD"] {
        if let Some(b) = up.strip_suffix(q) {
            if !b.is_empty() {
                return [b.to_string(), format!("W{b}"), b.trim_start_matches('W').to_string()]
                    .into_iter()
                    .collect();
            }
        }
    }
    Default::default()
}

struct PendingSim {
    route: Vec<SwapLeg>,
    chain: Chain,
    input_amount: f64,
    input_token: String,
    input_symbol: String,
    kind: OppKind,
    /// Owning strategy lane (P01..P05) — carried into SimRequest,
    /// OpportunitySignal and RejectedCandidate for attribution.
    strategy: &'static str,
}

#[derive(Default)]
struct Funnel {
    seen: AtomicU64,
    simulated: AtomicU64,
    profitable: AtomicU64,
    /// Routes dropped pre-sim because their leg states were incoherent
    /// (block/ts skew) — phantom-edge counter, journaled via StaleState.
    incoherent: AtomicU64,
}

/// Live `eth_gasPrice`, same pattern as the challenger's cost model.
async fn gas_price_wei(client: &reqwest::Client, rpc: &str) -> Option<u128> {
    let body = serde_json::json!({"jsonrpc":"2.0","id":1,"method":"eth_gasPrice","params":[]});
    let v: serde_json::Value = client.post(rpc).json(&body).send().await.ok()?.json().await.ok()?;
    u128::from_str_radix(v["result"].as_str()?.trim_start_matches("0x"), 16).ok()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env()
            .add_directive("allbright=info".parse().unwrap()))
        .init();
    allbright_core::init_crypto_provider();

    let cfg = Config::load()?;
    info!(config = ?cfg.redacted_summary(), "allbright-solver starting");

    let state = Arc::new(Mutex::new(State {
        pools: HashMap::new(),
        cex: HashMap::new(),
        pending: HashMap::new(),
        req_seq: 0,
        funnel: Funnel::default(),
        gas_price: HashMap::new(),
        last_coherence_emit: HashMap::new(),
        cex_trigger: HashMap::new(),
        dirty_moved: HashMap::new(),
        last_regen: HashMap::new(),
        bait: HashMap::new(),
        bait_checked: Instant::now() - std::time::Duration::from_secs(60),
        ev_last_emit: HashMap::new(),
        pred_scan: HashMap::new(),
    }));

    // Own bus :3103 — publishes Opportunity/ExecRequest/Telemetry.
    let addr: SocketAddr = ([127, 0, 0, 1], cfg.ports.solver).into();
    let bus = Arc::new(BusServer::bind(addr).await?);
    info!(%addr, "solver bus listening");
    let _ = bus.publish(&Event::System(SystemEvent {
        component: "solver".into(),
        action: "startup".into(),
        detail: "process boot".into(),
        ts_ms: now_ms(),
    }));

    // Simulation link :3102 — a reconnect loop maintains a live BusSender
    // in `sim_out`; inbound SimResults are scored in this same task.
    let sim_addr = format!("127.0.0.1:{}", cfg.ports.simulation);
    let sim_out: Arc<Mutex<Option<allbright_core::bus::BusSender>>> =
        Arc::new(Mutex::new(None));
    tokio::spawn({
        let state = state.clone();
        let bus = bus.clone();
        let cfg_c = cfg.clone();
        let sim_out = sim_out.clone();
        async move {
            loop {
                let cli = BusClient::connect_retry(&sim_addr).await;
                let (mut rx, tx) = cli.split();
                *sim_out.lock().await = Some(tx);
                while let Some(ev) = rx.recv().await {
                    if let Event::SimResult(res) = ev {
                        handle_sim_result(&state, &bus, &cfg_c, res).await;
                    }
                }
                *sim_out.lock().await = None;
                tracing::warn!("sim link dropped; reconnecting");
            }
        }
    });

    // Gas-price feeder — refreshes eth_gasPrice per chain every 30 s so
    // the profit gate can price gas in USD (net-at-gate, AGENTS §9).
    tokio::spawn({
        let state = state.clone();
        let cfg_g = cfg.clone();
        async move {
            let http = reqwest::Client::new();
            loop {
                for chain in &cfg_g.active_chains {
                    let chain = *chain;
                    let rpc = cfg_g
                        .rpc_urls
                        .get(&chain)
                        .or_else(|| cfg_g.rpc_pools.get(&chain).and_then(|v| v.first()));
                    let Some(rpc) = rpc else { continue };
                    if let Some(g) = gas_price_wei(&http, rpc).await {
                        state.lock().await.gas_price.insert(chain, (g, Instant::now()));
                    }
                }
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            }
        }
    });

    // Ingest feed :3101 — PoolUpdate/CexTick/DexQuote drive candidate routes.
    let ingest_addr = format!("127.0.0.1:{}", cfg.ports.ingest);
    tokio::spawn({
        let state = state.clone();
        let bus = bus.clone();
        let sim_out = sim_out.clone();
        let cfg_i = cfg.clone();
        async move {
            loop {
                let mut cli = BusClient::connect_retry(&ingest_addr).await;
                while let Some(ev) = cli.incoming.recv().await {
                    let mut reqs: Vec<SimRequest> = Vec::new();
                    let mut stale_ev: Option<Event> = None;
                    match ev {
                        Event::CexTick(t) => {
                            // P03 (honest atomic subset): a sharp CEX mid
                            // move implies on-chain pools are stale — fire
                            // an asset-scoped rescan immediately instead of
                            // waiting for the next pool poll.
                            let mut s = state.lock().await;
                            let prev_mid = s
                                .cex
                                .get(&t.symbol)
                                .map(|p| (p.bid + p.ask) / 2.0);
                            let mid = (t.bid + t.ask) / 2.0;
                            s.cex.insert(t.symbol.clone(), t.clone());

                            // P09 — predictive scan (CEX-leadership /
                            // stale-quote loop): on every tick — throttled
                            // 1 s/symbol — evaluate each pool's implied USD
                            // price vs the CEX fair mid. A persistent gap
                            // triggers a directed strike even when the CEX
                            // isn't moving (a pool that never repriced is a
                            // standing target).
                            let pred_due = s
                                .pred_scan
                                .get(&t.symbol)
                                .map(|ts| ts.elapsed()
                                    >= std::time::Duration::from_secs(1))
                                .unwrap_or(true);
                            if pred_due {
                                s.pred_scan.insert(t.symbol.clone(), Instant::now());
                                let syms = cex_asset_syms(&t.symbol);
                                if !syms.is_empty() {
                                    for chain in &cfg_i.active_chains {
                                        let strikes = arb::predictive_strikes(
                                            &s.pools,
                                            *chain,
                                            &syms,
                                            mid,
                                            &|q| pricing::usd_per_unit(
                                                q, *chain, &s.cex, cfg_i.native_usd,
                                            ),
                                        );
                                        queue_sims(&mut s, strikes, &cfg_i, &mut reqs);
                                    }
                                }
                            }

                            let Some(pm) = prev_mid else { continue };
                            let move_bps = if pm > 0.0 { ((mid - pm) / pm).abs() * 1e4 } else { 0.0 };
                            let cooldown_ok = s
                                .cex_trigger
                                .get(&t.symbol)
                                .map(|ts| ts.elapsed() > std::time::Duration::from_secs(5))
                                .unwrap_or(true);
                            if move_bps < CEX_MOVE_BPS || !cooldown_ok {
                                continue;
                            }
                            s.cex_trigger.insert(t.symbol.clone(), Instant::now());
                            let syms = cex_asset_syms(&t.symbol);
                            if syms.is_empty() {
                                continue;
                            }
                            let mut incoh = 0u64;
                            let mut cands = Vec::new();
                            for chain in &cfg_i.active_chains {
                                let (mut c, i) = arb::candidates_for_asset(&s.pools, *chain, &syms);
                                incoh += i;
                                cands.append(&mut c);
                            }
                            if cands.is_empty() {
                                continue;
                            }
                            info!(
                                symbol = %t.symbol, move_bps,
                                candidates = cands.len(),
                                "CEX divergence trigger — rescanning touched assets"
                            );
                            queue_sims(&mut s, cands, &cfg_i, &mut reqs);
                            if incoh > 0 {
                                s.funnel.incoherent.fetch_add(incoh, Ordering::Relaxed);
                            }
                        }
                        Event::PendingSwap(ps) => {
                            // U1 pre-confirmation trigger: a mempool tx
                            // touching a watched pool predicts an imminent
                            // state change — mark the pool moved and rescan
                            // NOW (pre-inclusion), not at the next poll.
                            let mut s = state.lock().await;
                            if !s.pools.contains_key(&ps.pool_address) {
                                continue;
                            }
                            s.dirty_moved
                                .entry(ps.chain)
                                .or_default()
                                .insert(ps.pool_address.clone());
                            // Tighter debounce than pool updates — the
                            // point is beating the pending tx's inclusion.
                            let due = s
                                .last_regen
                                .get(&ps.chain)
                                .map(|t| t.elapsed()
                                    >= std::time::Duration::from_millis(150))
                                .unwrap_or(true);
                            if !due {
                                continue;
                            }
                            s.last_regen.insert(ps.chain, Instant::now());
                            let moved =
                                s.dirty_moved.remove(&ps.chain).unwrap_or_default();
                            let (cands, _incoh) =
                                arb::candidates(&s.pools, ps.chain, &moved);
                            queue_sims(&mut s, cands, &cfg_i, &mut reqs);
                        }
                        Event::PoolHeartbeat(h) => {
                            // Post-dedup freshness: unchanged pools keep
                            // their state but get honest "verified-at"
                            // stamps — the coherence filter must see the
                            // read block, not the last-change block.
                            let mut s = state.lock().await;
                            for addr in &h.pools {
                                if let Some(p) = s.pools.get_mut(addr) {
                                    p.block = h.block;
                                    p.ts_ms = h.ts_ms;
                                }
                            }
                        }
                        Event::PoolUpdate(p) => {
                            {
                                let mut s = state.lock().await;
                                s.funnel.seen.fetch_add(1, Ordering::Relaxed);
                                let chain = p.chain;
                                // Bait-pool convictions: reload evidence
                                // files (30 s) and drop convicted pools —
                                // they fake reserves then revert on swap.
                                if s.bait_checked.elapsed()
                                    >= std::time::Duration::from_secs(30)
                                {
                                    s.bait_checked = Instant::now();
                                    for c in &cfg_i.active_chains {
                                        s.bait.insert(
                                            *c,
                                            allbright_core::bait::BaitRegistry::load(
                                                &allbright_core::bait::BaitRegistry::path_for(*c),
                                            ),
                                        );
                                    }
                                    let convicted: std::collections::HashSet<(Chain, String)> = s
                                        .bait
                                        .iter()
                                        .flat_map(|(c, b)| {
                                            b.pools.keys().cloned().map(move |a| (*c, a))
                                        })
                                        .collect();
                                    s.pools.retain(|addr, pl| {
                                        !convicted.contains(&(pl.chain, addr.clone()))
                                    });
                                }
                                if s
                                    .bait
                                    .get(&chain)
                                    .map(|b| b.is_convicted(&p.address))
                                    .unwrap_or(false)
                                {
                                    continue;
                                }
                                // P05 trigger: a material reserve shift on
                                // this pool means a real swap just landed —
                                // routes through it carry the residual window.
                                if let Some(prev) = s.pools.get(&p.address) {
                                    if material_shift(prev, &p) {
                                        s.dirty_moved
                                            .entry(chain)
                                            .or_default()
                                            .insert(p.address.clone());
                                    }
                                }
                                s.pools.insert(p.address.clone(), p);
                                // Debounce regen: at most one scan per
                                // REGEN_DEBOUNCE per chain — the update
                                // stream is far denser than block cadence.
                                let due = s
                                    .last_regen
                                    .get(&chain)
                                    .map(|t| t.elapsed() >= REGEN_DEBOUNCE)
                                    .unwrap_or(true);
                                if !due {
                                    continue;
                                }
                                s.last_regen.insert(chain, Instant::now());
                                let moved =
                                    s.dirty_moved.remove(&chain).unwrap_or_default();
                                // Regenerate only the updated chain's
                                // subgraph — O(chain²) not O(universe²).
                                let (cands, incoh) = arb::candidates(&s.pools, chain, &moved);
                                if incoh > 0 {
                                    let n = s.funnel.incoherent.fetch_add(incoh, Ordering::Relaxed) + incoh;
                                    // Bounded evidence: at most one StaleState
                                    // event per chain per 30 s — the counter
                                    // carries the true total.
                                    let emit_ok = s
                                        .last_coherence_emit
                                        .get(&chain)
                                        .map(|t| t.elapsed() > std::time::Duration::from_secs(30))
                                        .unwrap_or(true);
                                    if emit_ok {
                                        s.last_coherence_emit.insert(chain, Instant::now());
                                        stale_ev = Some(Event::Failure(
                                            FailureEvent::new(
                                                Component::Solver,
                                                FailureClass::StaleState,
                                                "arb.coherence",
                                                format!(
                                                    "{incoh} routes dropped this scan \
                                                     ({n} total): legs' pool states diverged \
                                                     in block/ts — phantom-edge filter"
                                                ),
                                            )
                                            .on_chain(chain),
                                        ));
                                    }
                                }
                                queue_sims(&mut s, cands, &cfg_i, &mut reqs);
                            }
                        }
                        _ => {}
                    }
                    if let Some(ev) = stale_ev {
                        let _ = bus.publish(&ev);
                    }
                    if let Some(t) = sim_out.lock().await.as_ref() {
                        let n = reqs.len();
                        for req in reqs {
                            let _ = t.send(&Event::SimRequest(req));
                        }
                        if n > 0 {
                            tracing::info!(candidates = n, "sims dispatched");
                        }
                    }
                }
                tracing::warn!("ingest feed dropped; reconnecting");
            }
        }
    });

    // Telemetry heartbeat.
    tokio::spawn({
        let state = state.clone();
        let bus = bus.clone();
        async move {
            let start = std::time::Instant::now();
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                let s = state.lock().await;
                let frame = TelemetryFrame {
                    seen: s.funnel.seen.load(Ordering::Relaxed),
                    simulated: s.funnel.simulated.load(Ordering::Relaxed),
                    profitable: s.funnel.profitable.load(Ordering::Relaxed),
                    uptime_secs: start.elapsed().as_secs(),
                    ts_ms: now_ms(),
                    ..Default::default()
                };
                drop(s);
                let _ = bus.publish(&Event::Telemetry(frame));
            }
        }
    });

    tokio::signal::ctrl_c().await.ok();
    Ok(())
}

/// Push candidates through the strategy allowlist into the sim pipeline —
/// one code path for pool-update, pending-tx (U1) and CEX predictive (P09)
/// scans. `kind` is derived from the strategy tag for attribution.
fn queue_sims(
    s: &mut State,
    cands: Vec<arb::Candidate>,
    cfg: &Config,
    reqs: &mut Vec<SimRequest>,
) {
    for c in cands {
        // P09 rides the P03 lane: admissible wherever P03 is enabled
        // (the predictive form of the same CEX/DEX strategy family).
        let allowed = cfg.strategies.iter().any(|t| t == c.strategy)
            || (c.strategy == "P09" && cfg.strategies.iter().any(|t| t == "P03"));
        if !allowed {
            continue;
        }
        let kind = match c.strategy {
            "P05" => OppKind::Backrun,
            "P03" | "P09" => OppKind::CexDexDivergence,
            _ => OppKind::AtomicDexArb,
        };
        s.req_seq += 1;
        let req_id = s.req_seq;
        s.pending.insert(req_id, PendingSim {
            route: c.route.clone(),
            chain: c.chain,
            input_amount: c.optimal_input,
            input_token: c.input_token.clone(),
            input_symbol: c.input_symbol.clone(),
            kind,
            strategy: c.strategy,
        });
        s.funnel.simulated.fetch_add(1, Ordering::Relaxed);
        reqs.push(SimRequest {
            req_id,
            chain: c.chain,
            route: c.route,
            input_amount: c.optimal_input,
            input_token: c.input_token,
            strategy: c.strategy.to_string(),
            block: 0,
            ts_ms: now_ms(),
        });
    }
}

/// SimResult arrived — score profitability and emit downstream events.
async fn handle_sim_result(
    state: &Arc<Mutex<State>>,
    bus: &Arc<BusServer>,
    cfg: &Config,
    res: SimResult,
) {
    let mut s = state.lock().await;
    info!(req_id = res.req_id, ok = res.ok, out = res.output_amount,
          us = res.elapsed_us, "sim result");
    let Some(p) = s.pending.remove(&res.req_id) else { return };
    // Every funnel exit is recorded as a classified Rejected candidate —
    // the observatory/challenger need the full rejection trail, not just
    // the survivors. ("Zero opportunity" ≠ "zero qualified opportunity".)
    let reject = |state: OppState, reason: RejectReason,
                  p: &PendingSim, gross_usd: f64, res: &SimResult| {
        Event::Rejected(RejectedCandidate {
            chain: p.chain,
            kind: p.kind,
            strategy: p.strategy.to_string(),
            req_id: res.req_id,
            route: p.route.clone(),
            input_amount: p.input_amount,
            input_token: p.input_token.clone(),
            state,
            reason,
            gross_usd,
            threshold_usd: cfg.min_profit_usd,
            gas_est: res.gas_est,
            ts_ms: now_ms(),
        })
    };
    if !res.ok {
        let ev = reject(OppState::Candidate, RejectReason::SimulationFailed, &p, 0.0, &res);
        drop(s);
        let _ = bus.publish(&ev);
        return;
    }
    // In-flight bait re-check: a conviction may have landed between sim
    // dispatch and result. A convicted pool means the quoted edge was
    // faked — drop the candidate, never let it reach the exec path.
    if p.route.iter().any(|leg| {
        s.bait
            .get(&p.chain)
            .map(|b| b.is_convicted(&leg.pool_address))
            .unwrap_or(false)
    }) {
        let ev = reject(OppState::Simulated, RejectReason::BaitPool, &p, 0.0, &res);
        drop(s);
        let _ = bus.publish(&ev);
        return;
    }
    let gross = res.output_amount - p.input_amount;
    if gross <= 0.0 {
        let ev = reject(OppState::Simulated, RejectReason::NegativeEdge, &p, 0.0, &res);
        drop(s);
        let _ = bus.publish(&ev);
        return;
    }
    // Reserve-bound sanity (MATH_ARTIFACT): the route's final leg must
    // pay out in the input token, and no pool can pay more than it
    // holds. output_amount above that reserve means the sim ran on
    // corrupt/incoherent state — reject as artifact before any USD
    // conversion turns it into a phantom nine-figure "profit".
    if let Some(last) = p.route.last() {
        let st = s
            .pools
            .get(&last.pool_address)
            .or_else(|| s.pools.get(&last.pool_address.to_lowercase()));
        if let Some(st) = st {
            let out_reserve = if st.token0.address.eq_ignore_ascii_case(&p.input_token) {
                st.reserve0
            } else if st.token1.address.eq_ignore_ascii_case(&p.input_token) {
                st.reserve1
            } else {
                // input token isn't a constituent of the paying pool —
                // route is malformed; bound on the smaller reserve
                st.reserve0.min(st.reserve1)
            };
            if res.output_amount > out_reserve && out_reserve > 0.0 {
                let ev = reject(OppState::Simulated, RejectReason::MathArtifact, &p, 0.0, &res);
                drop(s);
                let _ = bus.publish(&ev);
                return;
            }
        }
    }
    // Convert to USD: cyclic arb ends in the input token. If the CEX/
    // stable map can't price it, fall back to the solver's own pool
    // cache (V4 market-visible state) before conceding NoUsdPricing.
    let mut usd_per_unit =
        pricing::usd_per_unit(&p.input_symbol, p.chain, &s.cex, cfg.native_usd);
    if usd_per_unit <= 0.0 {
        usd_per_unit = pricing::usd_per_unit_via_pools(
            &p.input_symbol,
            p.chain,
            &s.pools,
            &s.cex,
            cfg.native_usd,
        );
    }
    if usd_per_unit <= 0.0 {
        let ev = reject(OppState::Simulated, RejectReason::NoUsdPricing, &p, 0.0, &res);
        drop(s);
        let _ = bus.publish(&ev);
        return;
    }
    let gross_usd = gross * usd_per_unit;
    // NET gate (AGENTS §9/§13): threshold applies to gross minus ALL
    // friction costs:
    //   gas_usd       = gas_est × live_gas_price × native_usd
    //   flash_usd     = Aave V3 flash premium (5 bps of input_amount);
    //                   other providers (Balancer 0%, DODO 0%) will lower
    //                   this term once GAP-7 is wired — conservative until
    //                   then (over-deducting is safe; under-deducting loses).
    // Both terms are carried on the Rejected record so the challenger can
    // reconstruct the full cost model independently.
    let native_usd = pricing::usd_per_unit(
        pricing::native_symbol(p.chain),
        p.chain,
        &s.cex,
        cfg.native_usd,
    );
    let gas_wei = s
        .gas_price
        .get(&p.chain)
        .map(|(g, _)| *g)
        .unwrap_or(2_000_000_000); // 2 gwei conservative fallback
    let gas_usd = res.gas_est as f64 * gas_wei as f64 / 1e18 * native_usd;
    // Flash premium: 5 bps of borrowed amount in input-token units.
    // If the sim route has no flash leg (future: non-flash cyclic) this
    // still over-deducts by at most 0.05% — safe direction.
    const AAVE_FLASH_BPS: f64 = 5.0; // 0.05%
    let flash_usd = p.input_amount * (AAVE_FLASH_BPS / 10_000.0) * usd_per_unit;
    let profit_usd = gross_usd - gas_usd - flash_usd;
    // EV-class submission (GAP-37): on EV-enabled chains, a positive-net
    // candidate below the standard floor is attempted rather than
    // discarded — revert cost on the sponsored path is ~zero, the
    // simgate dry-run + on-chain minProfit remain the authorities, and
    // every attempt produces real outcome evidence (the data the
    // calibration stack needs). Per-chain rate cap bounds quota burn;
    // EV_CHAINS empty = class disabled. The standard floor itself is
    // never lowered — this is a separate, marked submission class.
    let ev_eligible = cfg.ev_chains.contains(&p.chain)
        && profit_usd > 0.0
        && s
            .ev_last_emit
            .get(&p.chain)
            .map(|t| t.elapsed() >= std::time::Duration::from_secs(cfg.ev_emit_secs))
            .unwrap_or(true);
    let ev_mode = profit_usd < cfg.min_profit_usd && ev_eligible;
    if profit_usd < cfg.min_profit_usd && !ev_mode {
        info!(
            chain = %p.chain.name(), gross_usd, gas_usd, net = profit_usd,
            min = cfg.min_profit_usd, "positive edge below net threshold"
        );
        let ev = reject(OppState::Simulated, RejectReason::BelowStrategyThreshold, &p, gross_usd, &res);
        drop(s);
        let _ = bus.publish(&ev);
        return;
    }
    if ev_mode {
        s.ev_last_emit.insert(p.chain, Instant::now());
    } else {
        // `profitable` counts only candidates that cleared the standard
        // floor — EV-class emissions stay visible via ev_mode on the
        // signal, never folded into the standard-gate KPI.
        s.funnel.profitable.fetch_add(1, Ordering::Relaxed);
    }

    let signal = OpportunitySignal {
        chain: p.chain,
        kind: p.kind,
        strategy: p.strategy.to_string(),
        route: p.route.clone(),
        input_amount: p.input_amount,
        input_token: p.input_token,
        expected_profit_usd: profit_usd,
        // EV signals carry the bounded EV floor as the on-chain guard —
        // the contract still reverts below it; the $5 standard floor
        // applies to standard-class signals only.
        min_profit_usd: if ev_mode { cfg.ev_floor_usd } else { cfg.min_profit_usd },
        ev_mode,
        calldata: None, // executor encodes
        ts_ms: now_ms(),
    };
    let msg = if ev_mode {
        "EV-class opportunity (sub-floor, sponsored attempt)"
    } else {
        "profitable opportunity"
    };
    info!(chain = %p.chain.name(), profit_usd, legs = p.route.len(), ev = ev_mode, "{}", msg);
    let _ = bus.publish(&Event::Opportunity(signal.clone()));
    let _ = bus.publish(&Event::ExecRequest(ExecutionRequest {
        signal,
        executor_address: executor_addr_for(p.chain),
        dry_run: !cfg.auto_execute,
        ts_ms: now_ms(),
    }));
}

fn executor_addr_for(chain: Chain) -> String {
    std::env::var(format!("ALLBRIGHT_EXECUTOR_{}", chain.name().to_uppercase()))
        .unwrap_or_default()
}
