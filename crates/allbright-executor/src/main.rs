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

use alloy::primitives::Address;

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
                        // GAP-32 — bounded signal freshness: a signal
                        // built on reserves older than ~2× the chain's
                        // block cadence is stale; submitting wastes a
                        // sponsored-UserOp slot. Classified Rejected —
                        // never submitted, never silently dropped.
                        let stamped = if req.signal.ts_ms > 0 {
                            req.signal.ts_ms
                        } else {
                            req.ts_ms
                        };
                        let age_ms = now_ms().saturating_sub(stamped);
                        let bound = max_signal_age_ms(req.signal.chain);
                        if stamped > 0 && age_ms > bound {
                            let detail = format!(
                                "stale signal: age {age_ms}ms > {bound}ms bound (2x block cadence)"
                            );
                            let _ = bus.publish(&Event::Rejected(RejectedCandidate {
                                chain: req.signal.chain,
                                kind: req.signal.kind,
                                strategy: req.signal.strategy.clone(),
                                req_id: req.ts_ms,
                                route: req.signal.route.clone(),
                                input_amount: req.signal.input_amount,
                                input_token: req.signal.input_token.clone(),
                                state: OppState::RiskApproved,
                                reason: RejectReason::StaleState,
                                gross_usd: req.signal.expected_profit_usd,
                                threshold_usd: req.signal.min_profit_usd,
                                gas_est: 0,
                                ts_ms: now_ms(),
                            }));
                            let _ = bus.publish(&Event::Failure(
                                FailureEvent::new(Component::Executor, FailureClass::StaleState, "executor.signal_age", detail.clone())
                                    .on_chain(req.signal.chain),
                            ));
                            let _ = bus.publish(&Event::ExecResult(ExecutionResult {
                                chain: req.signal.chain,
                                strategy: req.signal.strategy.clone(),
                                status: ExecStatus::Failed,
                                tx_hash: None,
                                realized_profit_usd: 0.0,
                                gas_used: 0,
                                detail,
                                ts_ms: now_ms(),
                            }));
                            continue;
                        }
                        let sig = req.signal.clone();
                        let res = execute(&cfg, req).await;
                        match res.status {
                            ExecStatus::Submitted => {
                                sub.fetch_add(1, Ordering::Relaxed);
                                // Track the UserOp to a CONFIRMED,
                                // FINALIZED on-chain result — realized
                                // profit is measured from the contract's
                                // Executed event, never the expected
                                // figure (GAP-19/26).
                                if let (Some(hash), Some(pim)) =
                                    (res.tx_hash.clone(), pimlico_feed.clone())
                                {
                                    let settled = settled.clone();
                                    let inc2 = inc.clone();
                                    let bus2 = bus.clone();
                                    let chain = res.chain;
                                    let expected = sig.expected_profit_usd;
                                    let strat = res.strategy.clone();
                                    let rpc = cfg.rpc_urls.get(&chain).cloned();
                                    let executor_addr = cfg
                                        .executor_address(chain)
                                        .and_then(|a| a.parse::<Address>().ok());
                                    let token_in = sig
                                        .route
                                        .first()
                                        .and_then(|l| l.token_in.parse::<Address>().ok());
                                    tokio::spawn(async move {
                                        for _ in 0..90 {
                                            tokio::time::sleep(
                                                std::time::Duration::from_secs(5),
                                            )
                                            .await;
                                            match pim
                                                .user_op_receipt(chain.name(), &hash)
                                                .await
                                            {
                                                Ok(Some(receipt)) => {
                                                    // A receipt exists:
                                                    // nonce consumed
                                                    // on-chain either way.
                                                    aa::note_userop_included(&hash);
                                                    if !receipt.success {
                                                        tracing::warn!(%hash, "userop reverted on-chain");
                                                        let _ = bus2.publish(&Event::Failure(
                                                            FailureEvent::new(Component::AccountAbstraction, FailureClass::OnchainRevert, "aa.userop", format!("userop {hash} included but reverted"))
                                                                .on_chain(chain),
                                                        ));
                                                        let _ = bus2.publish(&Event::ExecResult(
                                                            ExecutionResult {
                                                                chain,
                                                                strategy: strat.clone(),
                                                                status: ExecStatus::Reverted,
                                                                tx_hash: Some(format!("{:?}", receipt.receipt.transaction_hash)),
                                                                realized_profit_usd: 0.0,
                                                                gas_used: receipt.actual_gas_used.saturating_to::<u64>(),
                                                                detail: format!("userop reverted on-chain; reason={:?}", receipt.reason),
                                                                ts_ms: now_ms(),
                                                            },
                                                        ));
                                                        return;
                                                    }
                                                    // GAP-26 — inclusion ≠ finality.
                                                    let block = receipt.receipt.block_number.unwrap_or(0);
                                                    let finalized = match &rpc {
                                                        Some(rpc) => pim.is_finalized(rpc, chain, block).await,
                                                        None => false,
                                                    };
                                                    if !finalized {
                                                        continue; // landed, not final — keep waiting
                                                    }
                                                    // GAP-19 — MEASURED profit from Executed logs.
                                                    let evs = aa::decode_receipt_events(
                                                        &receipt.logs,
                                                        executor_addr,
                                                    );
                                                    let (measured, note) = match &evs.executed {
                                                        Some(ev) => (
                                                            measured_profit(&pim, rpc.as_deref(), chain, token_in, ev).await,
                                                            format!("profit_raw={} token={:?}", ev.profit, ev.token),
                                                        ),
                                                        None => (None, "no Executed event in receipt".to_string()),
                                                    };
                                                    let realized = measured.unwrap_or(0.0);
                                                    inc2.fetch_add(1, Ordering::Relaxed);
                                                    credit_settled(&settled, realized);
                                                    tracing::info!(%hash, expected, realized, "userop confirmed+final");
                                                    let _ = bus2.publish(&Event::ExecResult(
                                                        ExecutionResult {
                                                            chain,
                                                            strategy: strat.clone(),
                                                            status: ExecStatus::Included,
                                                            tx_hash: Some(format!("{:?}", receipt.receipt.transaction_hash)),
                                                            realized_profit_usd: realized,
                                                            gas_used: receipt.actual_gas_used.saturating_to::<u64>(),
                                                            detail: format!(
                                                                "confirmed+final: expected ${expected:.4} → measured ${realized:.4} | {note}"
                                                            ),
                                                            ts_ms: now_ms(),
                                                        },
                                                    ));
                                                    return;
                                                }
                                                Ok(None) => {} // still in mempool
                                                Err(e) => {
                                                    tracing::debug!(%hash, "userop receipt poll: {e}")
                                                }
                                            }
                                        }
                                        tracing::warn!(%hash, "userop outcome unknown after ~450s — nonce released");
                                        aa::note_userop_dropped(&hash);
                                        let _ = bus2.publish(&Event::Failure(
                                            FailureEvent::new(Component::AccountAbstraction, FailureClass::NotIncluded, "aa.userop", format!("userop {hash} outcome unknown after 450s"))
                                                .on_chain(chain),
                                        ));
                                    });
                                }
                            }
                            ExecStatus::Included => {
                                inc.fetch_add(1, Ordering::Relaxed);
                                // realized_profit_usd is MEASURED (0.0 when
                                // unmeasured) — credit_settled is the only
                                // sanctioned crediting path.
                                credit_settled(&settled, res.realized_profit_usd);
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
            // GAP-21 — `last_swept` advances only on a CONFIRMED `Swept`
            // event; `sweep_inflight` prevents a second submission while
            // one is unconfirmed (avoids nonce waste + double sweeps).
            let last_swept = Arc::new(AtomicU64::new(0u64));
            let sweep_inflight = Arc::new(AtomicU64::new(0u64));
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
                let pending = accrued - last_swept.load(Ordering::Relaxed) as f64 / 1e6;
                if sweep_inflight.load(Ordering::Relaxed) != 0
                    || !(cfg.profit_transfer_mode.eq_ignore_ascii_case("AUTO")
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
                        .send_gasless_sweep(&cfg, pending, last_swept.load(Ordering::Relaxed), &dest)
                        .await
                    {
                        Ok(Some(sub)) => {
                            // Ledger row + last_swept advance happen in
                            // the tracker ONLY on a confirmed Swept event.
                            sweep_inflight.store(1, Ordering::Relaxed);
                            eligible_at = None; // next sweep re-draws jitter
                            let last_swept2 = last_swept.clone();
                            let inflight2 = sweep_inflight.clone();
                            let settled2 = settled.clone();
                            let bus3 = bus.clone();
                            let pim2 = p.clone();
                            tokio::spawn(async move {
                                track_sweep(pim2, sub, settled2, last_swept2, inflight2, bus3).await;
                            });
                            tracing::warn!(
                                "EUREKA — profit sweep submitted: ${pending:.2} → {dest}"
                            );
                        }
                        Ok(None) => {
                            tracing::debug!("sweep deferred (no key/executor configured)");
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
            realized_profit_usd: 0.0,
            gas_used: 0,
            detail: format!(
                "dry-run{}: {} legs, calldata {}B (expected ${:.4} — NOT realized)",
                if req.signal.ev_mode { " [EV]" } else { "" },
                req.signal.route.len(),
                encoded.len(),
                req.signal.expected_profit_usd,
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
            realized_profit_usd: 0.0,
            gas_used: 0,
            detail: format!("submitted{} (expected ${:.4} — pending measurement)",
                if req.signal.ev_mode { " [EV]" } else { "" },
                req.signal.expected_profit_usd),
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

/// Realized-profit credit — the ONLY sanctioned caller of
/// `settled.fetch_add`. Measured evidence only; expected values are
/// never credited (AGENTS.md §27).
fn credit_settled(settled: &Arc<AtomicU64>, measured_usd: f64) {
    if measured_usd.is_finite() {
        settled.fetch_add((measured_usd.max(0.0) * 1e6) as u64, Ordering::Relaxed);
    }
}

/// Convert the contract's `Executed.profit` (raw token units) to USD.
/// Stable-18/6: price 1. Non-stable profit token: no price source in
/// the executor — `None` (0 credited; the raw event amount is journaled
/// in the ExecResult detail for the sentinel to flag as under-measure).
async fn measured_profit(
    pim: &aa::PimlicoClient,
    rpc: Option<&str>,
    chain: Chain,
    token_hint: Option<Address>,
    ev: &aa::ExecutedEvent,
) -> Option<f64> {
    let tok = ev.token;
    let tok = if tok == Address::ZERO { token_hint? } else { tok };
    if !relay::stable_addr(chain, tok) {
        // Non-stable profit — no USD quote available in-process.
        return None;
    }
    let rpc = rpc?;
    let decimals = pim.token_decimals(rpc, tok).await.ok()?;
    Some(ev.profit.to::<u128>() as f64 / 10f64.powi(decimals as i32))
}

/// GAP-21 — poll a sweep UserOp to a CONFIRMED `Swept` event; only
/// then advance `last_swept` and append `data/transfers.jsonl`.
/// Terminal failure (revert / timeout) clears `sweep_inflight` so the
/// next eligible tick retries; `last_swept` stays put.
async fn track_sweep(
    pim: aa::PimlicoClient,
    sub: aa::SweepSubmission,
    settled: Arc<AtomicU64>,
    last_swept: Arc<AtomicU64>,
    inflight: Arc<AtomicU64>,
    bus: Arc<BusServer>,
) {
    for _ in 0..90 {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        match pim.user_op_receipt(sub.chain.name(), &sub.user_op_hash).await {
            Ok(Some(receipt)) => {
                aa::note_userop_included(&sub.user_op_hash);
                if !receipt.success {
                    tracing::warn!(hash = %sub.user_op_hash, "sweep userop reverted");
                    let _ = bus.publish(&Event::Failure(
                        FailureEvent::new(Component::AccountAbstraction, FailureClass::OnchainRevert, "aa.sweep", format!("sweep userop {} reverted", sub.user_op_hash))
                            .on_chain(sub.chain),
                    ));
                    inflight.store(0, Ordering::Relaxed);
                    return;
                }
                let block = receipt.receipt.block_number.unwrap_or(0);
                if !pim.is_finalized(&sub.rpc_url, sub.chain, block).await {
                    continue;
                }
                let evs = aa::decode_receipt_events(&receipt.logs, Some(sub.executor));
                match evs.swept {
                    Some(s) => {
                        last_swept.store(settled.load(Ordering::Relaxed), Ordering::Relaxed);
                        let amount_usd = if relay::stable_addr(sub.chain, s.token) {
                            pim.token_decimals(&sub.rpc_url, s.token)
                                .await
                                .ok()
                                .map(|d| s.amount.to::<u128>() as f64 / 10f64.powi(d as i32))
                        } else {
                            None
                        };
                        append_transfer(
                            "sweep",
                            sub.chain,
                            &format!("{:?}", receipt.receipt.transaction_hash),
                            &format!("{:?}", s.token),
                            &s.amount.to_string(),
                            amount_usd,
                            &sub.to,
                        );
                        tracing::warn!(hash = %sub.user_op_hash, "sweep CONFIRMED on-chain — ledger written");
                        inflight.store(0, Ordering::Relaxed);
                        return;
                    }
                    None => {
                        tracing::warn!(hash = %sub.user_op_hash, "sweep finalized but no Swept event — not crediting");
                        inflight.store(0, Ordering::Relaxed);
                        return;
                    }
                }
            }
            Ok(None) => {}
            Err(e) => tracing::debug!(hash = %sub.user_op_hash, "sweep poll: {e}"),
        }
    }
    aa::note_userop_dropped(&sub.user_op_hash);
    tracing::warn!(hash = %sub.user_op_hash, "sweep outcome unknown after ~450s");
    inflight.store(0, Ordering::Relaxed);
}

/// Append one row to `data/transfers.jsonl` — the file
/// `/api/transfers` reads. Written ONLY from confirmed on-chain
/// evidence (called after a verified `Swept` event); errors are logged,
/// never fatal.
fn append_transfer(
    kind: &str,
    chain: Chain,
    tx_hash: &str,
    token: &str,
    amount: &str,
    amount_usd: Option<f64>,
    to: &str,
) {
    let row = serde_json::json!({
        "ts_ms": now_ms(),
        "kind": kind,
        "chain": chain.name(),
        "chain_id": chain.chain_id(),
        "tx_hash": tx_hash,
        "token": token,
        "amount": amount,
        "amount_usd": amount_usd,
        "to": to,
    });
    let line = match serde_json::to_string(&row) {
        Ok(l) => l,
        Err(e) => {
            tracing::warn!("transfers.jsonl encode: {e}");
            return;
        }
    };
    let path = std::path::Path::new("data/transfers.jsonl");
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        Ok(mut f) => {
            use std::io::Write;
            if let Err(e) = writeln!(f, "{line}") {
                tracing::warn!("transfers.jsonl write: {e}");
            }
        }
        Err(e) => tracing::warn!("transfers.jsonl open: {e}"),
    }
}

/// GAP-32 — staleness bound per chain: ~2× block cadence. A signal
/// older than this is built on stale reserves and must not burn a
/// sponsored UserOp slot.
fn max_signal_age_ms(chain: Chain) -> u64 {
    match chain {
        Chain::Arb | Chain::Avax | Chain::Linea | Chain::Scroll => 2_000,
        Chain::Base | Chain::Op | Chain::Bsc => 4_000,
        Chain::Poly => 6_000,
        Chain::Eth => 24_000,
        Chain::Blast => 12_000,
    }
}
