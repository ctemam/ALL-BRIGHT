//! Allbright Ingestion Layer
//!
//! Architectural lineage:
//! - Paradigm Artemis: modular asynchronous collector architecture —
//!   independent collectors emitting typed events, fully decoupled from
//!   strategy/execution.
//! - Binance market-data infrastructure: CEX stream as external predictive
//!   signal.
//! - Velora (ex-ParaSwap): aggregated DEX quote signal.
//!
//! This module is an independent implementation and does not copy
//! proprietary source code.

mod cex_stream;
mod evm_stream;
mod recorder;
mod velora_stream;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use allbright_core::{
    bus::BusServer,
    chain,
    config::Config,
    events::{Chain, Event},
};
use tokio::sync::mpsc;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env()
            .add_directive("allbright=info".parse().unwrap()))
        .init();
    allbright_core::init_crypto_provider();

    let cfg = Config::load()?;
    info!(config = ?cfg.redacted_summary(), "allbright-ingest starting");

    let addr: SocketAddr = ([127, 0, 0, 1], cfg.ports.ingest).into();
    let bus = BusServer::bind(addr).await?;
    info!(%addr, "event bus listening");

    let (tx, mut rx) = mpsc::channel::<Event>(8192);

    // Collectors — independent, restartable, decoupled (Artemis lineage).
    tokio::spawn(cex_stream::run(tx.clone()));
    tokio::spawn(evm_stream::run(cfg.clone(), tx.clone()));
    tokio::spawn(velora_stream::run(cfg.clone(), tx.clone()));

    // Audit fan-in: subscribe to sibling buses and journal their
    // governance events so the full decision trail lands in the
    // append-only journal (IOO observations, solver rejections,
    // challenger verdicts, executor results/failures). Telemetry and
    // Sim* frames are excluded — journaled data must be meaningful.
    for port in [
        cfg.ports.solver,
        cfg.ports.executor,
        cfg.ports.observatory,
        cfg.ports.challenger,
        cfg.ports.sentinel,
        cfg.ports.scout,
    ] {
        let tx = tx.clone();
        tokio::spawn(async move {
            let addr = format!("127.0.0.1:{port}");
            loop {
                let mut cli =
                    allbright_core::bus::BusClient::connect_retry(&addr).await;
                while let Some(ev) = cli.incoming.recv().await {
                    match &ev {
                        Event::Rejected(_)
                        | Event::Challenge(_)
                        | Event::Observed(_)
                        | Event::Universe(_)
                        | Event::Opportunity(_)
                        | Event::ExecRequest(_)
                        | Event::ExecResult(_)
                        | Event::Heal(_)
                        | Event::RpcScan(_)
                        | Event::Failure(_) => {
                            let _ = tx.send(ev).await;
                        }
                        _ => {}
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        });
    }

    let mut journal = recorder::Journal::new("journal")?;
    // PoolUpdate dedup: collectors poll ~30 endpoints and republish
    // identical reserves every cycle (~530 ev/s observed) — that flood
    // starves every subscriber's inbound queue (CexTicks arrived minutes
    // late → wrong gas pricing) and was the main driver of 28 GB/day
    // journals. Publish only on state change, with a 10 s heartbeat so
    // downstream staleness detection still sees a live pool.
    let mut last_pub: HashMap<(Chain, String), (u64, Instant)> = HashMap::new();
    while let Some(ev) = rx.recv().await {
        if let Event::PoolUpdate(p) = &ev {
            let sig = p.reserve0.to_bits()
                ^ p.reserve1.to_bits().rotate_left(32)
                ^ p.sqrt_price_x96.unwrap_or(0.0).to_bits()
                ^ p.liquidity.unwrap_or(0.0).to_bits();
            let key = (p.chain, p.address.clone());
            let (changed, heartbeat_due) = match last_pub.get(&key) {
                Some((s, t)) => (*s != sig, t.elapsed() > Duration::from_secs(10)),
                None => (true, true),
            };
            if !changed && !heartbeat_due {
                continue;
            }
            last_pub.insert(key, (sig, Instant::now()));
        }
        let _ = journal.record(&ev); // append-only journal for replay parity
        let _ = bus.publish(&ev);
    }
    Ok(())
}

/// Share the default pool descriptors with submodules.
pub fn pools() -> Vec<chain::PoolDescriptor> {
    chain::load_pools()
}
