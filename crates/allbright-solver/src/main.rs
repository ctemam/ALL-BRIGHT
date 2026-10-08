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

use tokio::sync::Mutex;
use tracing::info;

use allbright_core::{
    bus::{BusClient, BusServer},
    config::Config,
    events::*,
};

/// Shared solver state.
struct State {
    pools: HashMap<String, PoolState>,          // pool address → state
    cex: HashMap<String, CexTick>,              // CEX symbol → latest tick
    pending: HashMap<u64, PendingSim>,          // req_id → context
    req_seq: u64,
    funnel: Funnel,
}

struct PendingSim {
    route: Vec<SwapLeg>,
    chain: Chain,
    input_amount: f64,
    input_token: String,
    input_symbol: String,
    kind: OppKind,
}

#[derive(Default)]
struct Funnel {
    seen: AtomicU64,
    simulated: AtomicU64,
    profitable: AtomicU64,
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
    }));

    // Own bus :3103 — publishes Opportunity/ExecRequest/Telemetry.
    let addr: SocketAddr = ([127, 0, 0, 1], cfg.ports.solver).into();
    let bus = Arc::new(BusServer::bind(addr).await?);
    info!(%addr, "solver bus listening");

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

    // Ingest feed :3101 — PoolUpdate/CexTick/DexQuote drive candidate routes.
    let ingest_addr = format!("127.0.0.1:{}", cfg.ports.ingest);
    tokio::spawn({
        let state = state.clone();
        let sim_out = sim_out.clone();
        async move {
            loop {
                let mut cli = BusClient::connect_retry(&ingest_addr).await;
                while let Some(ev) = cli.incoming.recv().await {
                    match ev {
                        Event::PoolUpdate(p) => {
                            let mut reqs = Vec::new();
                            {
                                let mut s = state.lock().await;
                                s.funnel.seen.fetch_add(1, Ordering::Relaxed);
                                s.pools.insert(p.address.clone(), p);
                                for c in arb::candidates(&s.pools) {
                                    s.req_seq += 1;
                                    let req_id = s.req_seq;
                                    s.pending.insert(req_id, PendingSim {
                                        route: c.route.clone(),
                                        chain: c.chain,
                                        input_amount: c.optimal_input,
                                        input_token: c.input_token.clone(),
                                        input_symbol: c.input_symbol.clone(),
                                        kind: OppKind::AtomicDexArb,
                                    });
                                    s.funnel.simulated.fetch_add(1, Ordering::Relaxed);
                                    reqs.push(SimRequest {
                                        req_id,
                                        chain: c.chain,
                                        route: c.route,
                                        input_amount: c.optimal_input,
                                        input_token: c.input_token,
                                        block: 0,
                                        ts_ms: now_ms(),
                                    });
                                }
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
                        Event::CexTick(t) => {
                            state.lock().await.cex.insert(t.symbol.clone(), t);
                        }
                        _ => {}
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
    let gross = res.output_amount - p.input_amount;
    if gross <= 0.0 {
        let ev = reject(OppState::Simulated, RejectReason::NegativeEdge, &p, 0.0, &res);
        drop(s);
        let _ = bus.publish(&ev);
        return;
    }
    // Convert to USD: cyclic arb ends in the input token.
    let usd_per_unit =
        pricing::usd_per_unit(&p.input_symbol, p.chain, &s.cex, cfg.native_usd);
    if usd_per_unit <= 0.0 {
        let ev = reject(OppState::Simulated, RejectReason::NoUsdPricing, &p, 0.0, &res);
        drop(s);
        let _ = bus.publish(&ev);
        return;
    }
    let profit_usd = gross * usd_per_unit;
    if profit_usd < cfg.min_profit_usd {
        info!(
            chain = %p.chain.name(), profit_usd, min = cfg.min_profit_usd,
            "positive edge below threshold"
        );
        let ev = reject(OppState::Simulated, RejectReason::BelowStrategyThreshold, &p, profit_usd, &res);
        drop(s);
        let _ = bus.publish(&ev);
        return;
    }
    s.funnel.profitable.fetch_add(1, Ordering::Relaxed);

    let signal = OpportunitySignal {
        chain: p.chain,
        kind: p.kind,
        route: p.route.clone(),
        input_amount: p.input_amount,
        input_token: p.input_token,
        expected_profit_usd: profit_usd,
        min_profit_usd: cfg.min_profit_usd,
        calldata: None, // executor encodes
        ts_ms: now_ms(),
    };
    info!(
        chain = %p.chain.name(), profit_usd, legs = p.route.len(),
        "profitable opportunity"
    );
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
