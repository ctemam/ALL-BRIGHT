//! Allbright Strategy Challenger — the independent auditor.
//!
//! Lineage: Allbright original (the anti-blindness layer).
//!
//! Every strategy rejection enters a counterfactual economics check:
//! the recorded gross edge is charged against the FULL cost model —
//! live gas, flash-loan premium, slippage buffer. If net is still
//! positive, the solver's decision is *challenged*: evidence the
//! threshold needs recalibration review. Challenged does NOT mean the
//! threshold is lowered — it means the rejection is flagged for review.
//!
//! The challenger never executes and never validates a strategy's claims
//! with the strategy's own numbers.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;
use tracing::info;

use allbright_core::{
    bus::{BusClient, BusServer},
    config::Config,
    events::{now_ms, Chain, Event, ChallengeVerdict},
};

/// Aave V3 flash-loan premium fraction (0.05%).
const FLASH_PREMIUM_PCT: f64 = 0.0005;
/// Adverse-movement / slippage buffer applied to the recorded gross edge.
const SLIPPAGE_BUFFER_PCT: f64 = 0.10;
/// Fallback gas when the sim didn't estimate one.
const DEFAULT_GAS_EST: u64 = 400_000;

struct State {
    /// (chain, token_addr) → symbol — learned from ingest PoolUpdates.
    token_symbols: HashMap<(u64, String), String>,
    /// CEX mids: "ETH" → usd.
    cex_usd: HashMap<String, f64>,
    /// Per-chain gas price (wei) with fetch timestamp.
    gas_price: HashMap<Chain, (u128, Instant)>,
    rejected: u64,
    challenged: u64,
    rescued_usd: f64,
}

fn token_usd(symbol: &str, cex: &HashMap<String, f64>) -> f64 {
    let s = symbol.to_uppercase();
    match s.as_str() {
        "USDC" | "USDT" | "DAI" | "FDUSD" | "BUSD" | "USDE" | "USDS" | "USD1" | "PYUSD" => 1.0,
        _ => {
            let base = s.strip_prefix('W').unwrap_or(&s).trim_end_matches(".E");
            cex.get(base).copied().unwrap_or(0.0)
        }
    }
}

/// Live `eth_gasPrice` on the chain's free public RPC, cached 60 s.
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
    info!(config = ?cfg.redacted_summary(), "allbright-challenger starting");

    let addr: SocketAddr = ([127, 0, 0, 1], cfg.ports.challenger).into();
    let bus = BusServer::bind(addr).await?;
    info!(%addr, "challenger bus listening");

    let state = Arc::new(Mutex::new(State {
        token_symbols: HashMap::new(),
        cex_usd: HashMap::new(),
        gas_price: HashMap::new(),
        rejected: 0,
        challenged: 0,
        rescued_usd: 0.0,
    }));
    let http = reqwest::Client::new();

    // Ingest feed :3101 — token symbols + CEX pricing only.
    let ingest_addr = format!("127.0.0.1:{}", cfg.ports.ingest);
    {
        let state = state.clone();
        tokio::spawn(async move {
            loop {
                let mut cli = BusClient::connect_retry(&ingest_addr).await;
                while let Some(ev) = cli.incoming.recv().await {
                    match ev {
                        Event::CexTick(t) => {
                            let sym = t.symbol.trim_end_matches("USDT").to_string();
                            state.lock().await.cex_usd.insert(sym, (t.bid + t.ask) / 2.0);
                        }
                        Event::PoolUpdate(p) => {
                            let mut s = state.lock().await;
                            let cid = p.chain.chain_id();
                            s.token_symbols.insert((cid, p.token0.address.clone()), p.token0.symbol.clone());
                            s.token_symbols.insert((cid, p.token1.address.clone()), p.token1.symbol.clone());
                        }
                        _ => {}
                    }
                }
                tracing::warn!("ingest feed dropped; reconnecting");
            }
        });
    }

    // Solver feed :3103 — the audited surface: every Rejected candidate.
    let solver_addr = format!("127.0.0.1:{}", cfg.ports.solver);
    {
        let state = state.clone();
        let cfg = cfg.clone();
        let http = http.clone();
        tokio::spawn(async move {
            loop {
                let mut cli = BusClient::connect_retry(&solver_addr).await;
                while let Some(ev) = cli.incoming.recv().await {
                    if let Event::Rejected(r) = ev {
                        challenge(&state, &bus, &cfg, &http, r).await;
                    }
                }
                tracing::warn!("solver feed dropped; reconnecting");
            }
        });
    }

    tokio::signal::ctrl_c().await.ok();
    Ok(())
}

/// Counterfactual economics on one rejected candidate.
async fn challenge(
    state: &Arc<Mutex<State>>,
    bus: &BusServer,
    cfg: &Config,
    http: &reqwest::Client,
    r: allbright_core::events::RejectedCandidate,
) {
    // Only economically-material rejections are worth a verdict.
    if r.gross_usd <= 0.0 {
        return;
    }
    let mut s = state.lock().await;
    s.rejected += 1;

    // Cost model: gas + flash premium + slippage buffer.
    let cid = r.chain.chain_id();
    let input_symbol = s
        .token_symbols
        .get(&(cid, r.input_token.clone()))
        .cloned()
        .unwrap_or_default();
    let input_usd = r.input_amount * token_usd(&input_symbol, &s.cex_usd);
    let native_usd = token_usd("WETH", &s.cex_usd).max(cfg.native_usd);

    let gas_wei = match s.gas_price.get(&r.chain) {
        Some((p, t)) if t.elapsed() < Duration::from_secs(60) => *p,
        _ => match cfg.rpc_urls.get(&r.chain) {
            Some(rpc) => {
                let p = gas_price_wei(http, rpc).await.unwrap_or(2_000_000_000); // 2 gwei fallback
                s.gas_price.insert(r.chain, (p, Instant::now()));
                p
            }
            None => 2_000_000_000,
        },
    };

    let gas_est = if r.gas_est == 0 { DEFAULT_GAS_EST } else { r.gas_est };
    let gas_usd = gas_est as f64 * gas_wei as f64 / 1e18 * native_usd;
    let premium_usd = FLASH_PREMIUM_PCT * input_usd;
    let buffer_usd = SLIPPAGE_BUFFER_PCT * r.gross_usd;
    let net = r.gross_usd - gas_usd - premium_usd - buffer_usd;
    let challenged = net > 0.0;

    if challenged {
        s.challenged += 1;
        s.rescued_usd += net;
        info!(
            chain = %r.chain.name(), req_id = r.req_id, gross = r.gross_usd,
            net, "THRESHOLD CHALLENGED — rejection refuted by counterfactual"
        );
    }
    let detail = format!(
        "gross={:.4} gas={:.4} prem={:.4} buf={:.4} net={:.4}",
        r.gross_usd, gas_usd, premium_usd, buffer_usd, net
    );
    let ev = Event::Challenge(ChallengeVerdict {
        chain: r.chain,
        req_id: r.req_id,
        reason: r.reason,
        counterfactual_net_usd: net,
        challenged,
        detail,
        ts_ms: now_ms(),
    });
    drop(s);
    let _ = bus.publish(&ev);
}
