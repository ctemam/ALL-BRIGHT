//! Allbright Simulation Layer
//!
//! Architectural lineage:
//! - Rusty-Sando: cached EVM-state / rapid simulation concepts — persistent
//!   pool cache, ephemeral per-opportunity evaluation contexts.
//! - revm: EVM execution engine (playback backend lands with the executor
//!   contract's calldata path; the math backend serves route evaluation now).
//! - Allbright original: persistent multi-chain opportunity simulation.
//!
//! This module is an independent implementation and does not copy
//! proprietary source code.

mod math;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::RwLock;
use tracing::info;

use allbright_core::{
    bus::{BusClient, BusServer},
    config::Config,
    events::{now_ms, Event, PoolState, SimResult},
};

/// Persistent per-chain pool cache — the "CacheDB" analog. Updated by the
/// ingest feed; cloned cheaply into ephemeral sim contexts per request.
type PoolCache = Arc<RwLock<HashMap<String, PoolState>>>;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env()
            .add_directive("allbright=info".parse().unwrap()))
        .init();
    allbright_core::init_crypto_provider();

    let cfg = Config::load()?;
    info!(config = ?cfg.redacted_summary(), "allbright-simulation starting");

    let cache: PoolCache = Arc::new(RwLock::new(HashMap::new()));
    let simulated = Arc::new(std::sync::atomic::AtomicU64::new(0));

    // Serve SimRequest on :3102.
    let addr: SocketAddr = ([127, 0, 0, 1], cfg.ports.simulation).into();
    let bus = BusServer::bind(addr).await?;
    info!(%addr, "simulation endpoint listening");

    // Warm the cache from the ingest feed (process boundary).
    let ingest_addr = format!("127.0.0.1:{}", cfg.ports.ingest);
    {
        let cache = cache.clone();
        let simulated = simulated.clone();
        tokio::spawn(async move {
            loop {
                let mut cli = BusClient::connect_retry(&ingest_addr).await;
                while let Some(ev) = cli.incoming.recv().await {
                    match ev {
                        Event::PoolUpdate(p) => {
                            cache.write().await.insert(p.address.clone(), p);
                        }
                        Event::PoolHeartbeat(h) => {
                            let mut c = cache.write().await;
                            for addr in &h.pools {
                                if let Some(p) = c.get_mut(addr) {
                                    p.block = h.block;
                                    p.ts_ms = h.ts_ms;
                                }
                            }
                        }
                        Event::SimRequest(req) => {
                            // In case solver posts requests on the ingest bus.
                            let c = cache.read().await;
                            let res = simulate(&c, &req, &simulated);
                            let _ = cli.send(&Event::SimResult(res));
                        }
                        _ => {}
                    }
                }
                tracing::warn!("ingest feed dropped; reconnecting");
            }
        });
    }

    // Request loop: SimRequest events arriving on our own bus get answered.
    // The bus echoes client-sent events to all clients, so solver sees the
    // SimResult we publish here.
    let srv = Arc::new(bus);
    let srv2 = srv.clone();
    let cache2 = cache.clone();
    let simd = simulated.clone();
    tokio::spawn(async move {
        // Nothing else needed: we can't observe inbound on a BusServer in this
        // minimal design without a client, so park a self-client that handles
        // SimRequest → publishes SimResult.
        let addr2 = format!("127.0.0.1:{}", srv2.addr.port());
        let mut cli = BusClient::connect_retry(&addr2).await;
        loop {
            while let Some(ev) = cli.incoming.recv().await {
                if let Event::SimRequest(req) = ev {
                    let c = cache2.read().await;
                    let res = simulate(&c, &req, &simd);
                    let _ = cli.send(&Event::SimResult(res));
                }
            }
            tracing::warn!("sim self-link dropped; reconnecting");
            cli = BusClient::connect_retry(&addr2).await;
        }
    });

    // Keep alive.
    tokio::signal::ctrl_c().await.ok();
    Ok(())
}

fn simulate(
    cache: &HashMap<String, PoolState>,
    req: &allbright_core::events::SimRequest,
    counter: &std::sync::atomic::AtomicU64,
) -> SimResult {
    let t0 = Instant::now();
    let has_v3 = req
        .route
        .iter()
        .any(|l| l.dex == allbright_core::events::DexKind::V3);

    let result = math::route_out(
        |addr| cache.get(addr),
        &req.route,
        &req.input_token,
        req.input_amount,
    );

    counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let elapsed_us = t0.elapsed().as_micros() as u64;

    match result {
        Some((out, sym)) => SimResult {
            req_id: req.req_id,
            chain: req.chain,
            ok: true,
            output_amount: out,
            output_token: sym,
            gas_est: math::gas_estimate(req.route.len(), has_v3),
            elapsed_us,
            detail: format!("{} legs", req.route.len()),
            ts_ms: now_ms(),
        },
        None => SimResult {
            req_id: req.req_id,
            chain: req.chain,
            ok: false,
            output_amount: 0.0,
            output_token: String::new(),
            gas_est: 0,
            elapsed_us,
            detail: "route failed (missing pool or empty output)".into(),
            ts_ms: now_ms(),
        },
    }
}
