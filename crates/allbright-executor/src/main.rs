//! Allbright Execution Layer
//!
//! Architectural lineage:
//! - Flashbots MEV-Share: private transaction/bundle concepts, builder/relay
//!   abstraction, inclusion monitoring.
//! - Allbright original: multi-chain BuilderRelay abstraction + contract
//!   calldata submission + ERC-4337 gasless ops (off hot path).
//!
//! This module is an independent implementation and does not copy
//! proprietary source code.

use allbright_executor::{aa, calldata, relay};

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use secrecy::ExposeSecret;
use tracing::info;

use allbright_core::{
    bus::{BusClient, BusServer},
    config::Config,
    events::*,
    lineage::{Component, FailureClass, FailureEvent},
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env()
            .add_directive("allbright=info".parse().unwrap()))
        .init();
    allbright_core::init_crypto_provider();

    let cfg = Config::load()?;
    info!(config = ?cfg.redacted_summary(), "allbright-executor starting");

    let addr: SocketAddr = ([127, 0, 0, 1], cfg.ports.executor).into();
    let bus = Arc::new(BusServer::bind(addr).await?);
    info!(%addr, "executor bus listening");

    // Gasless AA client (Pimlico sponsorship) — ops path only, never hot path.
    let pimlico = cfg
        .pimlico_api_key
        .as_ref()
        .map(|k| aa::PimlicoClient::new(k.expose_secret().to_string()));

    let settled_profit = Arc::new(AtomicU64::new(0)); // micro-USD
    let submitted = Arc::new(AtomicU64::new(0));
    let included = Arc::new(AtomicU64::new(0));

    // Subscribe to the solver bus for ExecutionRequests.
    let solver_addr = format!("127.0.0.1:{}", cfg.ports.solver);
    {
        let bus = bus.clone();
        let cfg = cfg.clone();
        let settled = settled_profit.clone();
        let sub = submitted.clone();
        let inc = included.clone();
        let pimlico_feed = pimlico.clone();
        tokio::spawn(async move {
            loop {
                let mut cli = BusClient::connect_retry(&solver_addr).await;
                while let Some(ev) = cli.incoming.recv().await {
                    if let Event::ExecRequest(req) = ev {
                        let res = execute(&cfg, req).await;
                        match res.status {
                            ExecStatus::Submitted => {
                                sub.fetch_add(1, Ordering::Relaxed);
                                // Track the UserOp to inclusion — profit only
                                // accrues on a confirmed on-chain result.
                                if let (Some(hash), Some(pim)) =
                                    (res.tx_hash.clone(), pimlico_feed.clone())
                                {
                                    let settled = settled.clone();
                                    let inc2 = inc.clone();
                                    let bus2 = bus.clone();
                                    let chain = res.chain;
                                    let expected = res.realized_profit_usd;
                                    tokio::spawn(async move {
                                        for _ in 0..40 {
                                            tokio::time::sleep(
                                                std::time::Duration::from_secs(5),
                                            )
                                            .await;
                                            match pim
                                                .user_op_outcome(chain.name(), &hash)
                                                .await
                                            {
                                                Ok((true, true)) => {
                                                    inc2.fetch_add(1, Ordering::Relaxed);
                                                    settled.fetch_add(
                                                        (expected.max(0.0) * 1e6) as u64,
                                                        Ordering::Relaxed,
                                                    );
                                                    tracing::info!(%hash, "userop included");
                                                    let _ = bus2.publish(&Event::ExecResult(
                                                        ExecutionResult {
                                                            chain,
                                                            status: ExecStatus::Included,
                                                            tx_hash: Some(hash.clone()),
                                                            realized_profit_usd: expected,
                                                            gas_used: 0,
                                                            detail: "userop included".into(),
                                                            ts_ms: now_ms(),
                                                        },
                                                    ));
                                                    return;
                                                }
                                                Ok((true, false)) => {
                                                    tracing::warn!(%hash, "userop failed on-chain");
                                                    return;
                                                }
                                                _ => {}
                                            }
                                        }
                                        tracing::warn!(%hash, "userop outcome unknown after 200s");
                                    });
                                }
                            }
                            ExecStatus::Included => {
                                inc.fetch_add(1, Ordering::Relaxed);
                                settled.fetch_add(
                                    (res.realized_profit_usd.max(0.0) * 1e6) as u64,
                                    Ordering::Relaxed,
                                );
                            }
                            _ => {}
                        }
                        if res.status == ExecStatus::Failed {
                            let _ = bus.publish(&Event::Failure(
                                FailureEvent::new(Component::BuilderRelay, FailureClass::BuilderRejected, "relay.submit", res.detail.clone())
                                    .on_chain(res.chain),
                            ));
                        }
                        let _ = bus.publish(&Event::ExecResult(res));
                    }
                }
                tracing::warn!("solver feed dropped; reconnecting");
                let _ = bus.publish(&Event::Failure(
                    FailureEvent::new(Component::Bus, FailureClass::StreamDrop, "executor.solver_feed", "solver bus connection dropped".into()),
                ));
            }
        });
    }

    // Profit-transfer loop (gasless AA, per ENV: AUTO ≥ $100).
    {
        let cfg = cfg.clone();
        let settled = settled_profit.clone();
        let bus = bus.clone();
        tokio::spawn(async move {
            let mut last_swept = 0u64;
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                let accrued = settled.load(Ordering::Relaxed) as f64 / 1e6;
                let pending = accrued - last_swept as f64 / 1e6;
                if cfg.profit_transfer_mode.eq_ignore_ascii_case("AUTO")
                    && pending >= cfg.profit_transfer_min_usd
                {
                    match &pimlico {
                        Some(p) => match p
                            .send_gasless_sweep(&cfg, pending, last_swept)
                            .await
                        {
                            Ok(_) => {
                                last_swept = settled.load(Ordering::Relaxed);
                                tracing::warn!(
                                    "EUREKA — profit sweep submitted: ${pending:.2} → {}",
                                    cfg.profit_wallet
                                );
                            }
                            Err(e) => {
                                tracing::warn!("gasless sweep failed: {e}");
                                let _ = bus.publish(&Event::Failure(
                                    FailureEvent::new(Component::AccountAbstraction, FailureClass::Unknown, "aa.sweep", e.to_string()),
                                ));
                            }
                        },
                        None => tracing::debug!("pimlico key absent; sweep deferred"),
                    }
                }
            }
        });
    }

    // Telemetry heartbeat.
    {
        let bus = bus.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                let _ = bus.publish(&Event::Telemetry(TelemetryFrame {
                    submitted: submitted.load(Ordering::Relaxed),
                    included: included.load(Ordering::Relaxed),
                    settled: settled_profit.load(Ordering::Relaxed) / 1_000_000,
                    ts_ms: now_ms(),
                    ..Default::default()
                }));
            }
        });
    }

    tokio::signal::ctrl_c().await.ok();
    Ok(())
}

/// Execute (or dry-run) an ExecutionRequest.
async fn execute(cfg: &Config, req: ExecutionRequest) -> ExecutionResult {
    // Kill switch (AGENTS §38.11): HALT.flag in the repo root halts NEW
    // execution activity — checked per request so engagement is immediate
    // without restart. Does not affect already-submitted transactions.
    if std::path::Path::new("HALT.flag").exists() {
        return ExecutionResult {
            chain: req.signal.chain,
            status: ExecStatus::Failed,
            tx_hash: None,
            realized_profit_usd: 0.0,
            gas_used: 0,
            detail: "KILL SWITCH ENGAGED — execution halted by operator".into(),
            ts_ms: now_ms(),
        };
    }
    // Always encode — calldata validity is checked even in dry-run so that
    // flipping AUTO_EXECUTE never surfaces encoding surprises.
    let encoded = calldata::encode_execute(&req.signal, &req.executor_address);
    let has_relay = !cfg.flashbots_auth_key.is_none()
        || req.signal.chain != Chain::Eth; // BSC/L2: public or builder adapters

    if req.dry_run || !has_relay {
        return ExecutionResult {
            chain: req.signal.chain,
            status: ExecStatus::Simulated,
            tx_hash: None,
            realized_profit_usd: req.signal.expected_profit_usd,
            gas_used: 0,
            detail: format!(
                "dry-run: {} legs, calldata {}B",
                req.signal.route.len(),
                encoded.len()
            ),
            ts_ms: now_ms(),
        };
    }

    match relay::submit(cfg, &req, &encoded).await {
        Ok(hash) => ExecutionResult {
            chain: req.signal.chain,
            status: ExecStatus::Submitted,
            tx_hash: Some(hash),
            realized_profit_usd: req.signal.expected_profit_usd,
            gas_used: 0,
            detail: "submitted".into(),
            ts_ms: now_ms(),
        },
        Err(e) => ExecutionResult {
            chain: req.signal.chain,
            status: ExecStatus::Failed,
            tx_hash: None,
            realized_profit_usd: 0.0,
            gas_used: 0,
            detail: e.to_string(),
            ts_ms: now_ms(),
        },
    }
}
