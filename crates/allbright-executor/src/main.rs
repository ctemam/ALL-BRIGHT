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
    bait::{self, BaitRegistry},
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
    let _ = bus.publish(&Event::System(SystemEvent {
        component: "executor".into(),
        action: "startup".into(),
        detail: "process boot".into(),
        ts_ms: now_ms(),
    }));

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
                                    let strat = res.strategy.clone();
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
                                                            strategy: strat.clone(),
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
    // Commander directive: sweeps go to a randomly-chosen ACTIVATED
    // wallet at a jittered time — no detectable pattern of interval or
    // destination. `sweepNative` takes no amount parameter, so timing
    // variance is what makes each swept amount different organically.
    const SWEEP_MAX_DELAY_SECS: u64 = 21_600; // 0–6 h jitter window
    {
        let cfg = cfg.clone();
        let settled = settled_profit.clone();
        let bus = bus.clone();
        tokio::spawn(async move {
            let mut last_swept = 0u64;
            let mut eligible_at: Option<u64> = None;
            let mut delay_secs: u64 = 0;
            // xorshift64 — pattern-breaking jitter, not a CSPRNG.
            let mut seed = now_ms() ^ 0x9e37_79b9_7f4a_7c15;
            let next = |s: &mut u64| -> u64 {
                *s ^= *s >> 12;
                *s ^= *s << 25;
                *s ^= *s >> 27;
                s.wrapping_mul(0x2545_F491_4F6C_DD1D)
            };
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                let accrued = settled.load(Ordering::Relaxed) as f64 / 1e6;
                let pending = accrued - last_swept as f64 / 1e6;
                if !(cfg.profit_transfer_mode.eq_ignore_ascii_case("AUTO")
                    && pending >= cfg.profit_transfer_min_usd)
                {
                    continue;
                }
                // First eligible tick → draw this sweep's jitter once.
                if eligible_at.is_none() {
                    eligible_at = Some(now_ms());
                    delay_secs = next(&mut seed) % (SWEEP_MAX_DELAY_SECS + 1);
                    tracing::info!(delay_secs, "sweep eligible — jittered trigger drawn");
                }
                if now_ms() < eligible_at.unwrap_or(0) + delay_secs * 1000 {
                    continue; // inside the jitter window — hold off
                }
                // Destination: random pick from ACTIVATED registry
                // wallets; env PROFIT_WALLET is the fallback when no
                // wallet is activated yet (never orphans profit).
                let dests = active_wallets();
                let dest = if dests.is_empty() {
                    cfg.profit_wallet.clone()
                } else {
                    dests[(next(&mut seed) as usize) % dests.len()].clone()
                };
                if dest.is_empty() {
                    continue;
                }
                match &pimlico {
                    Some(p) => match p
                        .send_gasless_sweep(&cfg, pending, last_swept, &dest)
                        .await
                    {
                        Ok(_) => {
                            last_swept = settled.load(Ordering::Relaxed);
                            eligible_at = None; // next sweep re-draws jitter
                            tracing::warn!(
                                "EUREKA — profit sweep submitted: ${pending:.2} → {dest}"
                            );
                        }
                        Err(e) => {
                            tracing::warn!("gasless sweep failed: {e}");
                            let _ = bus.publish(&Event::Failure(
                                FailureEvent::new(Component::AccountAbstraction, FailureClass::Unknown, "aa.sweep", e.to_string()),
                            ));
                            // Back off inside the jitter window rather
                            // than hammering a failing rail every 30 s.
                            delay_secs = (delay_secs + 300).min(SWEEP_MAX_DELAY_SECS);
                        }
                    },
                    None => tracing::debug!("pimlico key absent; sweep deferred"),
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

/// Activated profit destinations from the shared registry
/// (`data/profit_wallets.json` — written by the dashboard wallet page,
/// read here each sweep cycle). Address-validated; empty = the env
/// `PROFIT_WALLET` fallback is used by the caller.
fn active_wallets() -> Vec<String> {
    let path = std::env::var("PROFIT_WALLETS_PATH")
        .unwrap_or_else(|_| "data/profit_wallets.json".into());
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .map(|v| {
            v["wallets"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|w| w["active"].as_bool().unwrap_or(false))
                .filter_map(|w| w["address"].as_str().map(String::from))
                .filter(|a| {
                    let h = a.trim_start_matches("0x");
                    h.len() == 40 && h.chars().all(|c| c.is_ascii_hexdigit())
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Execute (or dry-run) an ExecutionRequest.
/// Convict route pools whose venue name appears inside the revert reason
/// (e.g. `CamelotPair: K` → venue "camelot"). Pool-level, persistent.
fn convict_bait_pools(chain: Chain, route: &[SwapLeg], reason: &str) {
    let rl = reason.to_lowercase();
    let token = rl.split([':', ' ', '(']).next().unwrap_or("");
    if token.len() < 3 {
        return;
    }
    let hits: Vec<String> = route
        .iter()
        .filter(|l| {
            let v = l.venue.to_lowercase();
            token.contains(&v) || v.contains(token)
        })
        .map(|l| l.pool_address.clone())
        .collect();
    if hits.is_empty() {
        return;
    }
    let path = BaitRegistry::path_for(chain);
    let mut reg = BaitRegistry::load(&path);
    for p in &hits {
        reg.convict(p, reason);
    }
    if reg.save(&path).is_ok() {
        tracing::warn!(chain = %chain.name(), pools = ?hits, %reason,
            "bait pools convicted — suppressing at pool level");
    }
}

async fn execute(cfg: &Config, req: ExecutionRequest) -> ExecutionResult {
    // Kill switch (AGENTS §38.11): HALT.flag in the repo root halts NEW
    // execution activity — checked per request so engagement is immediate
    // without restart. Does not affect already-submitted transactions.
    if std::path::Path::new("HALT.flag").exists() {
        return ExecutionResult {
            chain: req.signal.chain,
            strategy: req.signal.strategy.clone(),
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
            strategy: req.signal.strategy.clone(),
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
            strategy: req.signal.strategy.clone(),
            status: ExecStatus::Submitted,
            tx_hash: Some(hash),
            realized_profit_usd: req.signal.expected_profit_usd,
            gas_used: 0,
            detail: "submitted".into(),
            ts_ms: now_ms(),
        },
        Err(e) => {
            let detail = e.to_string();
            // Bait-pool forensics (7,380× CamelotPair: K evidence): a
            // Solidity Error(string) naming a venue on this route means
            // its pools fake reserves — convict them at pool level so
            // the solver stops feeding the funnel with them.
            if let Some(reason) = bait::decode_error_string(&detail) {
                convict_bait_pools(req.signal.chain, &req.signal.route, &reason);
            }
            ExecutionResult {
                chain: req.signal.chain,
                strategy: req.signal.strategy.clone(),
                status: ExecStatus::Failed,
                tx_hash: None,
                realized_profit_usd: 0.0,
                gas_used: 0,
                detail,
                ts_ms: now_ms(),
            }
        },
    }
}
