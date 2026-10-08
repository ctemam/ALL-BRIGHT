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

use std::net::SocketAddr;

use allbright_core::{bus::BusServer, chain, config::Config, events::Event};
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
    while let Some(ev) = rx.recv().await {
        let _ = journal.record(&ev); // append-only journal for replay parity
        let _ = bus.publish(&ev);
    }
    Ok(())
}

/// Share the default pool descriptors with submodules.
pub fn pools() -> Vec<chain::PoolDescriptor> {
    chain::load_pools()
}
