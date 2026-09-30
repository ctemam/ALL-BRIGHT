use crate::alert_manager::{AlertConfig, AlertEvent, AlertManager};
use crate::discovery::DiscoveryService;
use crate::gas_bidder::{GasBidConfig, GasBidStrategy, GasBidder};
use crate::mev_guard::{MevGuard, MevGuardConfig, MevRiskLevel};
use crate::paper_trader::{BacktestConfig, BacktestResult, PaperTradeMode, PaperTrader};
use crate::pimlico_client::PimlicoClient;
use crate::portfolio_manager::{PortfolioConfig, PortfolioManager, StrategyAllocation};
use crate::profit_splitter::{ProfitSplitter, SplitterConfig, SplitterWallet};
use crate::profit_transfer::ProfitTransferService;
use crate::radar_scanner::RadarScanner;
use crate::rpc_pool::RpcPool;
use crate::rules_engine::{ExecutionRule, RuleAction, RuleField, RuleOperator, RulesEngine};
use crate::types::*;
use crate::velora_client::VeloraClient;
use crate::websocket;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use parking_lot::RwLock;
use std::sync::Arc;
use std::time::Instant;
use tracing::{info, warn};

/// Message returned by endpoints that cannot perform the work they advertise.
///
/// Several routes used to answer with fabricated success: a `status: "simulated"` result,
/// a random transaction hash, or randomised risk numbers. They now fail with 501 Not
/// Implemented plus this explanation, so a missing capability is never mistaken for
/// completed work. The required work is tracked in `docs/ARBITRAGE-COMPARISON.md`.
const NOT_IMPLEMENTED_EXECUTION: &str = "Arbitrage execution is not implemented: this build has no EOA signer and no direct transaction broadcast path, so nothing was submitted and no transaction hash can exist. Exception: gas_strategy \"Pimlico\" with a client-signed `user_operation` field really submits an ERC-4337 UserOperation to the Pimlico bundler (see /api/pimlico/*). See docs/ARBITRAGE-COMPARISON.md (P0-2, B-P0-1..B-P0-3).";

#[derive(Clone)]
pub struct AppState {
    pub scanner: Arc<RadarScanner>,
    pub velora: Arc<VeloraClient>,
    pub pimlico: Arc<PimlicoClient>,
    pub start_time: Instant,
    pub bot_status: Arc<RwLock<Option<BotStatus>>>,
    pub bot_config: Arc<RwLock<BotConfig>>,
    pub llm_config: Arc<RwLock<Option<LLMConfig>>>,
    pub paper_trader: Arc<RwLock<PaperTrader>>,
    pub portfolio_manager: Arc<RwLock<PortfolioManager>>,
    pub mev_guard: Arc<RwLock<MevGuard>>,
    pub alert_manager: Arc<RwLock<AlertManager>>,
    pub rules_engine: Arc<RwLock<RulesEngine>>,
    pub profit_splitter: Arc<RwLock<ProfitSplitter>>,
    pub gas_bidder: Arc<RwLock<GasBidder>>,
    /// AUTO/MANUAL profit withdrawal service.
    pub profit_transfer: Arc<ProfitTransferService>,
    /// Multi-endpoint RPC pool with health tracking and failover.
    pub rpc_pool: Arc<RpcPool>,
    /// External discovery feeds (DEX Screener, DeFiLlama, Binance).
    pub discovery: Arc<DiscoveryService>,
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/api/rpc/health", get(rpc_health))
        .route("/api/profit/config", get(profit_transfer_config))
        .route("/api/profit/preview", post(profit_transfer_preview))
        .route("/api/profit/transfer", post(profit_transfer_manual))
        .route("/api/profit/history", get(profit_transfer_history))
        .route("/api/health", get(health_check))
        .route("/api/live/opportunities", get(live_opportunities))
        .route("/api/live/status", get(live_status))
        .route("/api/live/trades", get(live_trades))
        .route("/api/live/chains", get(live_chains))
        .route("/monitor", get(monitor_dashboard))
        .route("/api/scan", post(scan_token))
        .route("/api/scan/comprehensive", post(scan_comprehensive))
        .route("/api/all-prices", post(get_all_prices))
        .route("/api/all-opportunities", post(get_all_opportunities))
        .route("/api/velora/price", post(get_velora_price))
        .route("/api/velora/swap", post(get_velora_swap))
        .route("/api/velora/build-tx", post(build_velora_tx))
        .route("/api/velora/delta", post(submit_delta_order))
        .route("/api/execute", post(execute_arbitrage))
        .route("/api/execute/advanced", post(execute_advanced))
        .route("/api/bot/config", get(get_bot_config))
        .route("/api/bot/config", post(update_bot_config))
        .route("/api/bot/start", post(start_bot))
        .route("/api/bot/stop", post(stop_bot))
        .route("/api/bot/status", get(get_bot_status))
        .route("/api/bot/logs", get(get_bot_logs))
        .route("/api/llm/config", get(get_llm_config))
        .route("/api/llm/config", post(update_llm_config))
        .route("/api/llm/advise", post(get_llm_advice))
        .route("/api/liquidity", post(get_liquidity_data))
        .route("/api/bubbles", get(get_bubble_data))
        .route("/api/dashboard", get(get_dashboard))
        // â”€â”€â”€ Paper Trading / Backtesting â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
        .route("/api/paper/start", post(paper_trading_start))
        .route("/api/paper/stop", post(paper_trading_stop))
        .route("/api/paper/status", get(paper_trading_status))
        .route("/api/paper/simulate", post(paper_simulate_trade))
        .route("/api/paper/backtest", post(paper_run_backtest))
        .route("/api/paper/reset", post(paper_trading_reset))
        // â”€â”€â”€ Portfolio Manager â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
        .route("/api/portfolio/config", get(get_portfolio_config))
        .route("/api/portfolio/config", post(update_portfolio_config))
        .route("/api/portfolio/status", get(get_portfolio_status))
        // â”€â”€â”€ MEV Guard â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
        .route("/api/mev/config", get(get_mev_config))
        .route("/api/mev/config", post(update_mev_config))
        .route("/api/mev/analyze", post(mev_analyze))
        // â”€â”€â”€ Alerts (Telegram/Discord) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
        .route("/api/alerts/config", get(get_alerts_config))
        .route("/api/alerts/config", post(update_alerts_config))
        .route("/api/alerts/history", get(get_alerts_history))
        .route("/api/alerts/test", post(test_alert))
        // â”€â”€â”€ Rules Engine â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
        .route("/api/rules", get(get_rules))
        .route("/api/rules", post(update_rules))
        .route("/api/rules/evaluate", post(evaluate_rules))
        // â”€â”€â”€ Profit Splitter â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
        .route("/api/splitter/config", get(get_splitter_config))
        .route("/api/splitter/config", post(update_splitter_config))
        .route("/api/splitter/calculate", post(calculate_split))
        // â”€â”€â”€ Gas Bidder â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
        .route("/api/gas/config", get(get_gas_config))
        .route("/api/gas/config", post(update_gas_config))
        .route("/api/gas/recommend", post(recommend_gas))
        .route("/api/chains", get(list_chains))
        // NOTE: axum 0.7 (matchit 0.7) uses `:param` path syntax; the previous
        // `/api/dexes/{chain_id}` was axum 0.8 syntax and never matched (always 404).
        .route("/api/dexes/:chain_id", get(list_dexes))
        // â”€â”€â”€ Pimlico (ERC-4337) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
        // The API key stays server-side: the browser's bundler/paymaster transport
        // points at /api/pimlico/rpc/:chain_id, which injects PIMLICO_API_KEY.
        .route("/api/pimlico/status/:chain_id", get(pimlico_status))
        .route("/api/pimlico/rpc/:chain_id", post(pimlico_rpc))
        .route("/api/pimlico/estimate", post(pimlico_estimate))
        .route("/api/pimlico/sponsor", post(pimlico_sponsor))
        .route("/api/pimlico/send", post(pimlico_send))
        .route("/api/pimlico/receipt/:chain_id/:hash", get(pimlico_receipt))
        .route(
            "/api/pimlico/status-op/:chain_id/:hash",
            get(pimlico_op_status),
        )
        .route("/api/pimlico/gas-price/:chain_id", get(pimlico_gas_price))
        .route("/ws", get(websocket::ws_handler))
        .with_state(state)
}

async fn health_check(State(state): State<AppState>) -> Json<HealthResponse> {
    let chains = crate::chains::get_chains();
    Json(HealthResponse {
        status: "ok".to_string(),
        chains_connected: chains.iter().map(|c| c.name.clone()).collect(),
        uptime_secs: state.start_time.elapsed().as_secs(),
    })
}

async fn live_opportunities(State(state): State<AppState>) -> Json<Vec<OpportunityDetail>> {
    Json(state.scanner.latest_opportunities())
}

async fn live_status(State(state): State<AppState>) -> Json<serde_json::Value> {
    let opps = state.scanner.latest_opportunities();
    let profitable = opps.iter().filter(|o| o.profit_breakdown.is_profitable).count();
    let total_net: f64 = opps.iter().map(|o| o.profit_breakdown.net_profit_usd).sum();
    let cumulative = state.scanner.cumulative_profit_usd();
    let accrued = state.profit_transfer.accrued_usd().await;

    // Per-chain breakdown
    let mut by_chain = std::collections::BTreeMap::new();
    for opp in &opps {
        let entry = by_chain
            .entry(opp.chain_name.clone())
            .or_insert_with(|| serde_json::json!({"count": 0, "net_usd": 0.0}));
        entry["count"] = serde_json::json!(entry["count"].as_i64().unwrap_or(0) + 1);
        let prev = entry["net_usd"].as_f64().unwrap_or(0.0);
        entry["net_usd"] = serde_json::json!(prev + opp.profit_breakdown.net_profit_usd);
    }

    Json(serde_json::json!({
        "scanner": "running",
        "live_opportunities": opps.len(),
        "profitable": profitable,
        "current_cycle_net_usd": total_net,
        "cumulative_profit_usd": cumulative,
        "accrued_for_transfer_usd": accrued,
        "uptime_secs": state.start_time.elapsed().as_secs(),
        "by_chain": by_chain,
    }))
}

async fn live_trades(State(state): State<AppState>) -> Json<serde_json::Value> {
    let trades = state.scanner.trade_history();
    Json(serde_json::json!({
        "trades": trades,
        "total": trades.len(),
    }))
}

async fn live_chains(State(state): State<AppState>) -> Json<serde_json::Value> {
    let chains = state.scanner.chain_profit_breakdown();
    let total: f64 = chains.iter().map(|c| c.total_profit_usd).sum();
    Json(serde_json::json!({
        "chains": chains,
        "total_profit_usd": total,
    }))
}

async fn monitor_dashboard() -> axum::response::Html<String> {
    axum::response::Html(MONITOR_HTML.to_string())
}

/// Inline HTML dashboard — auto-refreshing monitor showing profit per chain,
/// per trade, and cumulative progress toward the $100 auto-transfer threshold.
const MONITOR_HTML: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>ZeroCap Arbitrage Monitor</title>
<style>
  * { margin: 0; padding: 0; box-sizing: border-box; }
  body { font-family: 'Segoe UI', system-ui, -apple-system, sans-serif; background: #0a0e17; color: #e0e0e0; }
  .header { background: linear-gradient(135deg, #1a1f2e 0%, #0d1117 100%); padding: 20px 30px; border-bottom: 1px solid #2d333b; display: flex; justify-content: space-between; align-items: center; }
  .header h1 { font-size: 22px; font-weight: 600; color: #58a6ff; }
  .header .status { display: flex; align-items: center; gap: 8px; }
  .header .dot { width: 10px; height: 10px; border-radius: 50%; background: #3fb950; animation: pulse 2s infinite; }
  @keyframes pulse { 0%, 100% { opacity: 1; } 50% { opacity: 0.4; } }
  .header .mode { color: #3fb950; font-weight: 600; font-size: 14px; }
  .grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(280px, 1fr)); gap: 16px; padding: 20px 30px; }
  .card { background: #161b22; border: 1px solid #2d333b; border-radius: 10px; padding: 20px; }
  .card h2 { font-size: 13px; text-transform: uppercase; letter-spacing: 1px; color: #8b949e; margin-bottom: 12px; }
  .card .value { font-size: 32px; font-weight: 700; }
  .card .sub { font-size: 12px; color: #8b949e; margin-top: 4px; }
  .green { color: #3fb950; }
  .yellow { color: #d29922; }
  .blue { color: #58a6ff; }
  .section { padding: 0 30px 20px; }
  .section h2 { font-size: 16px; color: #58a6ff; margin-bottom: 12px; padding-top: 10px; border-top: 1px solid #2d333b; }
  table { width: 100%; border-collapse: collapse; font-size: 13px; }
  th { text-align: left; color: #8b949e; font-weight: 500; padding: 8px 12px; border-bottom: 1px solid #2d333b; font-size: 11px; text-transform: uppercase; letter-spacing: 0.5px; }
  td { padding: 8px 12px; border-bottom: 1px solid #1c2128; }
  tr:hover { background: #1c2128; }
  .profit { color: #3fb950; font-weight: 600; }
  .progress-bar { width: 100%; height: 20px; background: #1c2128; border-radius: 10px; overflow: hidden; margin-top: 8px; }
  .progress-fill { height: 100%; background: linear-gradient(90deg, #238636, #3fb950); border-radius: 10px; transition: width 0.5s ease; }
  .progress-text { font-size: 11px; color: #8b949e; margin-top: 4px; text-align: right; }
  .refresh-note { font-size: 11px; color: #484f58; text-align: center; padding: 10px; }
  .no-data { color: #484f58; text-align: center; padding: 20px; font-style: italic; }
  .chain-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(200px, 1fr)); gap: 10px; }
  .chain-card { background: #1c2128; border-radius: 8px; padding: 14px; border-left: 3px solid #58a6ff; }
  .chain-card .name { font-weight: 600; color: #c9d1d9; font-size: 14px; }
  .chain-card .profit { font-size: 20px; font-weight: 700; color: #3fb950; margin-top: 4px; }
  .chain-card .trades { font-size: 11px; color: #8b949e; margin-top: 2px; }
</style>
</head>
<body>
<div class="header">
  <h1>ZeroCap Arbitrage Monitor</h1>
  <div class="status"><div class="dot"></div><span class="mode" id="scanner-status">SCANNING</span></div>
</div>

<div class="grid">
  <div class="card">
    <h2>Cumulative Profit</h2>
    <div class="value green" id="cumulative">$0.00</div>
    <div class="sub" id="trade-count">0 trades</div>
  </div>
  <div class="card">
    <h2>Accrued for Transfer</h2>
    <div class="value yellow" id="accrued">$0.00</div>
    <div class="progress-bar"><div class="progress-fill" id="progress" style="width:0%"></div></div>
    <div class="progress-text" id="progress-text">$0.00 / $100.00 threshold</div>
  </div>
  <div class="card">
    <h2>Live Opportunities</h2>
    <div class="value blue" id="live-opps">0</div>
    <div class="sub" id="current-net">Current cycle: $0.00</div>
  </div>
  <div class="card">
    <h2>Uptime</h2>
    <div class="value" id="uptime" style="color:#c9d1d9">0s</div>
    <div class="sub" id="wallet">Wallet: loading...</div>
  </div>
</div>

<div class="section">
  <h2>Profit per Chain</h2>
  <div class="chain-grid" id="chain-grid">
    <div class="no-data">Waiting for first trade...</div>
  </div>
</div>

<div class="section">
  <h2>Trade History (Etherscan-Verified)</h2>
  <table>
    <thead><tr>
      <th>Time</th><th>Chain</th><th>Token</th><th>Buy Venue</th><th>Sell Venue</th>
      <th>Profit</th><th>Gas</th><th>Status</th><th>Etherscan</th>
    </tr></thead>
    <tbody id="trades-body">
      <tr><td colspan="9" class="no-data">Waiting for first trade...</td></tr>
    </tbody>
  </table>
</div>

<div class="refresh-note">Auto-refreshes every 10 seconds</div>

<script>
const BASE = window.location.origin;

function fmt(n) { return '$' + n.toFixed(2); }
function fmtDuration(s) {
  if (s < 60) return s + 's';
  if (s < 3600) return Math.floor(s/60) + 'm ' + (s%60) + 's';
  return Math.floor(s/3600) + 'h ' + Math.floor((s%3600)/60) + 'm';
}

async function refresh() {
  try {
    const [statusRes, tradesRes, chainsRes, transferRes] = await Promise.all([
      fetch(BASE + '/api/live/status'),
      fetch(BASE + '/api/live/trades'),
      fetch(BASE + '/api/live/chains'),
      fetch(BASE + '/api/profit/transfer/status'),
    ]);
    const status = await statusRes.json();
    const trades = await tradesRes.json();
    const chains = await chainsRes.json();
    let transfer = {};
    try { transfer = await transferRes.json(); } catch(e) {}

    // Summary cards
    document.getElementById('cumulative').textContent = fmt(status.cumulative_profit_usd || 0);
    document.getElementById('accrued').textContent = fmt(status.accrued_for_transfer_usd || 0);
    document.getElementById('live-opps').textContent = status.live_opportunities || 0;
    document.getElementById('current-net').textContent = 'Current cycle: ' + fmt(status.current_cycle_net_usd || 0);
    document.getElementById('uptime').textContent = fmtDuration(status.uptime_secs || 0);
    document.getElementById('trade-count').textContent = (trades.total || 0) + ' trades';

    const threshold = (transfer.min_usd || 100);
    const accrued = status.accrued_for_transfer_usd || 0;
    const pct = Math.min((accrued / threshold) * 100, 100);
    document.getElementById('progress').style.width = pct + '%';
    document.getElementById('progress-text').textContent = fmt(accrued) + ' / ' + fmt(threshold) + ' threshold';

    if (transfer.destinations && transfer.destinations.length > 0) {
      document.getElementById('wallet').textContent = 'Wallet: ' + transfer.destinations[0].address.slice(0,6) + '...' + transfer.destinations[0].address.slice(-4);
    }

    // Chain grid
    const cg = document.getElementById('chain-grid');
    if (chains.chains && chains.chains.length > 0) {
      cg.innerHTML = chains.chains
        .sort((a,b) => b.total_profit_usd - a.total_profit_usd)
        .map(c => `<div class="chain-card">
          <div class="name">${c.chain}</div>
          <div class="profit">${fmt(c.total_profit_usd)}</div>
          <div class="trades">${c.trade_count} trades${c.last_trade ? ' &middot; last: ' + c.last_trade : ''}</div>
        </div>`).join('');
    }

    // Trade history table
    const tb = document.getElementById('trades-body');
    if (trades.trades && trades.trades.length > 0) {
      tb.innerHTML = trades.trades.slice(0, 50).map(t => {
        const statusBadge = {
          'Confirmed': '<span style="color:#3fb950;font-weight:600">VERIFIED</span>',
          'Reverted': '<span style="color:#f85149;font-weight:600">REVERTED</span>',
          'Pending': '<span style="color:#d29922">PENDING</span>',
          'QuoteOnly': '<span style="color:#8b949e">QUOTE ONLY</span>',
          'ExecutionFailed': '<span style="color:#f85149">EXEC FAILED</span>',
        }[t.verified] || '<span style="color:#8b949e">-</span>';
        const etherscanLink = t.tx_hash && t.explorer_url
          ? '<a href="' + t.explorer_url + '" target="_blank" style="color:#58a6ff;text-decoration:none">' + t.tx_hash.slice(0,10) + '...</a>'
          : '-';
        return `<tr>
          <td>${t.timestamp}</td>
          <td>${t.chain}</td>
          <td><b>${t.token}</b></td>
          <td>${t.buy_venue}</td>
          <td>${t.sell_venue}</td>
          <td class="profit">${fmt(t.profit_usd)}</td>
          <td>${fmt(t.gas_cost_usd || 0)}</td>
          <td>${statusBadge}</td>
          <td>${etherscanLink}</td>
        </tr>`;
      }).join('');
    }
  } catch(e) {
    console.error('refresh error:', e);
  }
}

refresh();
setInterval(refresh, 10000);
</script>
</body>
</html>
"##;

async fn scan_token(
    State(state): State<AppState>,
    Json(req): Json<serde_json::Value>,
) -> Result<Json<RadarScanResponse>, (StatusCode, String)> {
    let token = req["token"]
        .as_str()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "token required".to_string()))?;
    let addr = req["token_address"].as_str();
    state
        .scanner
        .scan_token(token, addr)
        .await
        .map(Json)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Scan failed: {}", e),
            )
        })
}

async fn get_all_prices(
    State(state): State<AppState>,
    Json(req): Json<serde_json::Value>,
) -> Result<Json<AllPricesResponse>, (StatusCode, String)> {
    let token = req["token"]
        .as_str()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "token required".to_string()))?;
    let addr = req["token_address"].as_str();
    state
        .scanner
        .scan_all_prices(token, addr)
        .await
        .map(Json)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Price scan failed: {}", e),
            )
        })
}

async fn get_all_opportunities(
    State(state): State<AppState>,
    Json(req): Json<serde_json::Value>,
) -> Result<Json<AllOpportunitiesResponse>, (StatusCode, String)> {
    let token = req["token"]
        .as_str()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "token required".to_string()))?;
    let addr = req["token_address"].as_str();
    state
        .scanner
        .scan_all_strategies(token, addr)
        .await
        .map(Json)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Strategy scan failed: {}", e),
            )
        })
}

// â”€â”€â”€ Velora API Handlers â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

async fn get_velora_price(
    State(state): State<AppState>,
    Json(req): Json<serde_json::Value>,
) -> Result<Json<VeloraPriceResponse>, (StatusCode, String)> {
    let chain_id = req["chain_id"]
        .as_u64()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "chain_id required".to_string()))?;
    let src = req["src_token"]
        .as_str()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "src_token required".to_string()))?;
    let dst = req["dest_token"]
        .as_str()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "dest_token required".to_string()))?;
    let sd = req["src_decimals"].as_u64().unwrap_or(18) as u8;
    let dd = req["dest_decimals"].as_u64().unwrap_or(18) as u8;
    let amt = req["amount"]
        .as_str()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "amount required".to_string()))?;
    let side = req["side"].as_str().unwrap_or("SELL");

    let resp = state
        .velora
        .get_price(chain_id, src, dst, sd, dd, amt, side)
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                format!("Velora price error: {}", e),
            )
        })?;

    let routes: Vec<VeloraRoute> = resp
        .price_route
        .best_route
        .iter()
        .flat_map(|seg| {
            seg.swaps.iter().map(move |s| VeloraRoute {
                src_token: s.src_token.clone(),
                src_decimals: resp.price_route.src_decimals,
                dest_token: s.dest_token.clone(),
                dest_decimals: resp.price_route.dest_decimals,
                src_amount: s.src_amount.clone(),
                dest_amount: s.dest_amount.clone(),
                percentage: s.percent,
                exchange: s.exchange.clone(),
            })
        })
        .collect();

    Ok(Json(VeloraPriceResponse {
        src_token: resp.price_route.src_token,
        dest_token: resp.price_route.dest_token,
        src_amount: resp.price_route.src_amount,
        dest_amount: resp.price_route.dest_amount,
        price_impact: 0.0,
        routes,
        gas_cost_usd: resp.price_route.gas_cost_usd.parse().unwrap_or(0.0),
        contract_address: resp.price_route.contract_address,
        token_transfer_proxy: resp.price_route.token_transfer_proxy,
        version: resp.price_route.version,
    }))
}

async fn get_velora_swap(
    State(state): State<AppState>,
    Json(req): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let chain_id = req["chain_id"]
        .as_u64()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "chain_id required".to_string()))?;
    let src = req["src_token"]
        .as_str()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "src_token required".to_string()))?;
    let dst = req["dest_token"]
        .as_str()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "dest_token required".to_string()))?;
    let sd = req["src_decimals"].as_u64().unwrap_or(18) as u8;
    let dd = req["dest_decimals"].as_u64().unwrap_or(18) as u8;
    let amt = req["amount"]
        .as_str()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "amount required".to_string()))?;
    let side = req["side"].as_str().unwrap_or("SELL");
    let ua = req["user_address"].as_str();
    let slip = req["slippage"].as_u64().map(|v| v as u32);

    let resp = state
        .velora
        .get_swap(chain_id, src, dst, sd, dd, amt, side, ua, slip)
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                format!("Velora /swap error: {}", e),
            )
        })?;

    Ok(Json(serde_json::json!({
        "priceRoute": {
            "srcToken": resp.price_route.src_token,
            "destToken": resp.price_route.dest_token,
            "srcAmount": resp.price_route.src_amount,
            "destAmount": resp.price_route.dest_amount,
            "gasCostUSD": resp.price_route.gas_cost_usd,
            "contractAddress": resp.price_route.contract_address,
            "tokenTransferProxy": resp.price_route.token_transfer_proxy,
            "version": resp.price_route.version,
        },
        "txParams": {
            "from": resp.tx_params.from,
            "to": resp.tx_params.to,
            "value": resp.tx_params.value,
            "data": resp.tx_params.data,
            "gasPrice": resp.tx_params.gas_price,
            "chainId": resp.tx_params.chain_id,
        }
    })))
}

async fn build_velora_tx(
    State(state): State<AppState>,
    Json(req): Json<serde_json::Value>,
) -> Result<Json<VeloraTxResponse>, (StatusCode, String)> {
    let chain_id = req["chain_id"]
        .as_u64()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "chain_id required".to_string()))?;
    let src = req["src_token"]
        .as_str()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "src_token required".to_string()))?;
    let dst = req["dest_token"]
        .as_str()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "dest_token required".to_string()))?;
    let sd = req["src_decimals"].as_u64().unwrap_or(18) as u8;
    let dd = req["dest_decimals"].as_u64().unwrap_or(18) as u8;
    let sa = req["src_amount"]
        .as_str()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "src_amount required".to_string()))?;
    let da = req["dest_amount"]
        .as_str()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "dest_amount required".to_string()))?;
    let slip = req["slippage"].as_f64().unwrap_or(0.5);
    let ua = req["user_address"]
        .as_str()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "user_address required".to_string()))?;
    let rc = req["receiver"].as_str();
    let pr = req["price_route"].clone();

    state
        .velora
        .build_transaction(chain_id, src, dst, sd, dd, sa, da, slip, ua, rc, &pr)
        .await
        .map(|r| {
            Json(VeloraTxResponse {
                from: r.from,
                to: r.to,
                value: r.value,
                data: r.data,
                gas_price: r.gas_price,
                gas: r.gas,
                chain_id: r.chain_id,
            })
        })
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("Velora tx error: {}", e)))
}

async fn submit_delta_order(
    State(state): State<AppState>,
    Json(req): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let chain_id = req["chain_id"]
        .as_u64()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "chain_id required".to_string()))?;
    let src = req["src_token"]
        .as_str()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "src_token required".to_string()))?;
    let dst = req["dest_token"]
        .as_str()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "dest_token required".to_string()))?;
    let amt = req["amount"]
        .as_str()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "amount required".to_string()))?;
    let ua = req["user_address"]
        .as_str()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "user_address required".to_string()))?;

    state
        .velora
        .submit_delta_order(chain_id, src, dst, amt, ua)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("Delta order error: {}", e)))
}

// â”€â”€â”€ Pimlico (ERC-4337) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/// GET /api/pimlico/status/{chain_id} â€” bundler configuration + live entry points.
async fn pimlico_status(
    State(state): State<AppState>,
    Path(chain_id): Path<u64>,
) -> Json<serde_json::Value> {
    let mut out = serde_json::json!({
        "configured": state.pimlico.is_configured(),
        "chain_id": chain_id,
        "entry_point_v06": PimlicoClient::ENTRY_POINT_V06,
        "entry_point_v07": PimlicoClient::ENTRY_POINT_V07,
        "entry_point_v08": PimlicoClient::ENTRY_POINT_V08,
    });
    if state.pimlico.is_configured() {
        match state.pimlico.supported_entry_points(chain_id).await {
            Ok(entry_points) => out["entry_points"] = serde_json::json!(entry_points),
            Err(e) => out["entry_points_error"] = serde_json::json!(e),
        }
    }
    Json(out)
}

/// POST /api/pimlico/rpc/{chain_id} â€” JSON-RPC proxy to the Pimlico bundler/paymaster.
/// The browser's permissionless.js clients use this as their transport so the
/// PIMLICO_API_KEY never reaches the frontend.
async fn pimlico_rpc(
    State(state): State<AppState>,
    Path(chain_id): Path<u64>,
    Json(req): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let method = req.get("method").and_then(|m| m.as_str()).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "JSON-RPC `method` required".to_string(),
        )
    })?;
    let params = req
        .get("params")
        .cloned()
        .unwrap_or_else(|| serde_json::json!([]));
    let id = req
        .get("id")
        .cloned()
        .unwrap_or_else(|| serde_json::json!(1));

    let result = state
        .pimlico
        .rpc(chain_id, method, params)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, e))?;

    Ok(Json(
        serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result }),
    ))
}

/// Shared body for estimate/sponsor/send: `{ chain_id, user_operation, entry_point? }`.
fn pimlico_op_args(
    req: &serde_json::Value,
) -> Result<(u64, serde_json::Value, String), (StatusCode, String)> {
    let chain_id = req["chain_id"]
        .as_u64()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "chain_id required".to_string()))?;
    let user_operation = req
        .get("user_operation")
        .cloned()
        .filter(|v| v.is_object())
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                "`user_operation` object required (flat/unpacked ERC-4337 v0.7 layout)".to_string(),
            )
        })?;
    let entry_point = req
        .get("entry_point")
        .and_then(|v| v.as_str())
        .unwrap_or(PimlicoClient::ENTRY_POINT_V07)
        .to_string();
    Ok((chain_id, user_operation, entry_point))
}

/// POST /api/pimlico/estimate â€” `eth_estimateUserOperationGas` passthrough.
async fn pimlico_estimate(
    State(state): State<AppState>,
    Json(req): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let (chain_id, user_operation, entry_point) = pimlico_op_args(&req)?;
    state
        .pimlico
        .estimate_user_operation_gas(chain_id, &user_operation, &entry_point)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::BAD_GATEWAY, e))
}

/// POST /api/pimlico/sponsor â€” `pm_sponsorUserOperation` passthrough.
async fn pimlico_sponsor(
    State(state): State<AppState>,
    Json(req): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let (chain_id, user_operation, entry_point) = pimlico_op_args(&req)?;
    state
        .pimlico
        .sponsor_user_operation(chain_id, &user_operation, &entry_point)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::BAD_GATEWAY, e))
}

/// POST /api/pimlico/send â€” `eth_sendUserOperation`: real bundler submission.
async fn pimlico_send(
    State(state): State<AppState>,
    Json(req): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let (chain_id, user_operation, entry_point) = pimlico_op_args(&req)?;
    let user_op_hash = state
        .pimlico
        .send_user_operation(chain_id, &user_operation, &entry_point)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, e))?;
    info!(chain_id, %user_op_hash, "UserOperation accepted by Pimlico bundler");
    Ok(Json(serde_json::json!({
        "user_op_hash": user_op_hash,
        "entry_point": entry_point,
        "chain_id": chain_id,
    })))
}

/// GET /api/pimlico/receipt/{chain_id}/{hash} â€” `eth_getUserOperationReceipt`.
async fn pimlico_receipt(
    State(state): State<AppState>,
    Path((chain_id, hash)): Path<(u64, String)>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    state
        .pimlico
        .user_operation_receipt(chain_id, &hash, PimlicoClient::ENTRY_POINT_V07)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::BAD_GATEWAY, e))
}

/// GET /api/pimlico/status-op/{chain_id}/{hash} â€” `pimlico_getUserOperationStatus`.
async fn pimlico_op_status(
    State(state): State<AppState>,
    Path((chain_id, hash)): Path<(u64, String)>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    state
        .pimlico
        .user_operation_status(chain_id, &hash)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::BAD_GATEWAY, e))
}

/// GET /api/pimlico/gas-price/{chain_id} â€” `pimlico_getUserOperationGasPrice`.
async fn pimlico_gas_price(
    State(state): State<AppState>,
    Path(chain_id): Path<u64>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    state
        .pimlico
        .gas_price(chain_id)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::BAD_GATEWAY, e))
}

// â”€â”€â”€ Execution â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/// Shared Pimlico branch for `/api/execute` and `/api/execute/advanced`:
/// submits a client-signed ERC-4337 UserOperation to the Pimlico bundler for real.
async fn execute_pimlico_user_op(
    state: &AppState,
    chain_id: Option<u64>,
    user_operation: Option<&UserOperation>,
    strategy: &str,
    execution_mode: &str,
) -> Result<Json<ExecuteResult>, (StatusCode, String)> {
    let chain_id = chain_id.ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "gas_strategy \"Pimlico\" requires `chain_id` in the request body".to_string(),
        )
    })?;
    let op = user_operation.ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "gas_strategy \"Pimlico\" requires a signed `user_operation`: build and sign it \
             client-side (see frontend/src/lib/pimlico.ts, which uses permissionless.js) or \
             call POST /api/pimlico/send directly"
                .to_string(),
        )
    })?;
    let op_value = serde_json::to_value(op).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to serialize user_operation: {}", e),
        )
    })?;

    let user_op_hash = state
        .pimlico
        .send_user_operation(chain_id, &op_value, PimlicoClient::ENTRY_POINT_V07)
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                format!("Pimlico bundler rejected the UserOperation: {}", e),
            )
        })?;

    info!(chain_id, %user_op_hash, "UserOperation accepted by Pimlico bundler");
    Ok(Json(ExecuteResult {
        status: "submitted".to_string(),
        message: format!(
            "UserOperation {} accepted by the Pimlico bundler on chain {} (EntryPoint v0.7, \
             ERC-4337). It is queued for inclusion, not mined yet; poll \
             GET /api/pimlico/receipt/{}/{} for the bundled transaction.",
            user_op_hash, chain_id, chain_id, user_op_hash
        ),
        strategy: strategy.to_string(),
        execution_mode: execution_mode.to_string(),
        tx_hash: Some(user_op_hash),
        estimated_profit_usd: None,
        gas_cost_usd: None,
    }))
}

async fn execute_arbitrage(
    State(state): State<AppState>,
    Json(req): Json<ExecuteArbitrageRequest>,
) -> Result<Json<ExecuteResult>, (StatusCode, String)> {
    // Real path: ERC-4337 via Pimlico â€” only possible when the client supplies a
    // signed UserOperation (the backend deliberately holds no signing key for EOA txs).
    if let GasStrategy::Pimlico = req.gas_strategy {
        return execute_pimlico_user_op(
            &state,
            req.chain_id,
            req.user_operation.as_ref(),
            "simple",
            req.flash_loan_source.as_str(),
        )
        .await;
    }

    // Previously answered `status: "simulated"` with a fabricated 0x... hash and invented
    // profit/gas figures, which made a no-op look like a completed trade (D-01/D-03).
    warn!(
        opportunity_id = %req.opportunity_id,
        "Rejecting /api/execute: execution is not implemented for this gas strategy"
    );
    Err((
        StatusCode::NOT_IMPLEMENTED,
        NOT_IMPLEMENTED_EXECUTION.to_string(),
    ))
}

async fn execute_advanced(
    State(state): State<AppState>,
    Json(req): Json<AdvancedExecuteRequest>,
) -> Result<Json<ExecuteResult>, (StatusCode, String)> {
    // Same Pimlico branch as /api/execute; everything else keeps the honest 501
    // (the previous implementation returned a fabricated hash and a profit figure
    // selected by strategy name, without touching any chain â€” D-01/D-03).
    if let GasStrategy::Pimlico = req.gas_strategy {
        return execute_pimlico_user_op(
            &state,
            req.chain_id,
            req.user_operation.as_ref(),
            &req.strategy,
            &format!("{:?}", req.execution_mode),
        )
        .await;
    }

    warn!(
        strategy = %req.strategy,
        "Rejecting /api/execute/advanced: execution is not implemented for this gas strategy"
    );
    Err((
        StatusCode::NOT_IMPLEMENTED,
        NOT_IMPLEMENTED_EXECUTION.to_string(),
    ))
}

// â”€â”€â”€ Comprehensive Scan â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

async fn scan_comprehensive(
    State(state): State<AppState>,
    Json(req): Json<serde_json::Value>,
) -> Result<Json<ComprehensiveScanResponse>, (StatusCode, String)> {
    let tokens: Vec<crate::radar_scanner::TokenInfo> =
        serde_json::from_value(req["tokens"].clone())
            .map_err(|e| (StatusCode::BAD_REQUEST, format!("Invalid tokens: {}", e)))?;
    state
        .scanner
        .comprehensive_scan(&tokens)
        .await
        .map(Json)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Comprehensive scan failed: {}", e),
            )
        })
}

// â”€â”€â”€ Bot Control â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

async fn get_bot_config(State(state): State<AppState>) -> Json<BotConfig> {
    Json(state.bot_config.read().clone())
}

async fn update_bot_config(
    State(state): State<AppState>,
    Json(config): Json<BotConfig>,
) -> Json<BotConfig> {
    *state.bot_config.write() = config.clone();
    info!("Bot config updated: {:?}", config.mode);
    Json(config)
}

async fn start_bot(State(state): State<AppState>) -> Json<BotStatus> {
    let config = state.bot_config.read().clone();
    let mut status = state.bot_status.write();
    *status = Some(BotStatus {
        running: true,
        config,
        total_trades: status.as_ref().map_or(0, |s| s.total_trades),
        successful_trades: status.as_ref().map_or(0, |s| s.successful_trades),
        failed_trades: status.as_ref().map_or(0, |s| s.failed_trades),
        total_profit_usd: status.as_ref().map_or(0.0, |s| s.total_profit_usd),
        uptime_secs: 0,
        current_opportunity: None,
        last_execution: None,
        logs: Vec::new(),
    });
    info!("Bot started");
    Json(status.clone().unwrap())
}

async fn stop_bot(State(state): State<AppState>) -> Json<serde_json::Value> {
    *state.bot_status.write() = None;
    info!("Bot stopped");
    Json(serde_json::json!({"status": "stopped"}))
}

async fn get_bot_status(State(state): State<AppState>) -> Json<serde_json::Value> {
    let status = state.bot_status.read();
    match status.as_ref() {
        Some(s) => Json(serde_json::json!(s)),
        None => Json(serde_json::json!({"running": false, "message": "Bot is stopped"})),
    }
}

async fn get_bot_logs(State(state): State<AppState>) -> Json<Vec<BotLogEntry>> {
    let status = state.bot_status.read();
    Json(status.as_ref().map_or(Vec::new(), |s| s.logs.clone()))
}

// â”€â”€â”€ LLM Integration â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

async fn get_llm_config(State(state): State<AppState>) -> Json<serde_json::Value> {
    let cfg = state.llm_config.read();
    match cfg.as_ref() {
        Some(c) => Json(serde_json::json!(c)),
        None => Json(serde_json::json!({"configured": false})),
    }
}

async fn update_llm_config(
    State(state): State<AppState>,
    Json(config): Json<LLMConfig>,
) -> Json<serde_json::Value> {
    *state.llm_config.write() = Some(config.clone());
    info!("LLM config updated: {}", config.provider.as_str());
    Json(serde_json::json!({"configured": true, "provider": config.provider.as_str()}))
}

async fn get_llm_advice(
    State(state): State<AppState>,
    Json(req): Json<LLMAdviceRequest>,
) -> Json<LLMAdviceResponse> {
    let llm_cfg = state.llm_config.read().clone();
    match llm_cfg {
        Some(_cfg) => {
            // A "configured" LLM previously produced hard-coded advice, including invented
            // statistics ("Historical success rate: 87%") and a fabricated
            // recommend_execute: true trading recommendation. Until a real provider call
            // exists, say so rather than appearing to advise a trade (D-09).
            Json(LLMAdviceResponse {
                advice: "LLM advisor is not implemented: no provider (OpenAI/Anthropic/etc.) call is wired up.".to_string(),
                confidence: "none".to_string(),
                recommend_execute: false,
                reasoning: vec![
                    "This endpoint previously returned hard-coded advice containing invented statistics.".to_string(),
                ],
                risk_factors: vec![
                    "No LLM advisor is implemented - do not treat any output from this route as advice".to_string(),
                ],
            })
        }
        None => Json(LLMAdviceResponse {
            advice: "LLM not configured. Please configure your API key in settings.".to_string(),
            confidence: "none".to_string(),
            recommend_execute: false,
            reasoning: vec![],
            risk_factors: vec!["No LLM advisor configured".to_string()],
        }),
    }
}

// â”€â”€â”€ Liquidity Data â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

async fn get_liquidity_data(State(state): State<AppState>) -> Json<LiquidityMapResponse> {
    // Build liquidity data from the live scanner's opportunity cache.
    let opps = state.scanner.latest_opportunities();
    let mut data_points = Vec::new();
    let mut chain_totals: std::collections::BTreeMap<u64, (String, f64, std::collections::HashSet<String>, usize)> =
        std::collections::BTreeMap::new();

    for opp in &opps {
        if let Some(buy_dex) = &opp.buy_dex {
            data_points.push(LiquidityDataPoint {
                chain_id: opp.chain_id,
                chain_name: opp.chain_name.clone(),
                dex_name: buy_dex.clone(),
                token: opp.token.clone(),
                token_address: opp.token_address.clone(),
                liquidity_usd: opp.liquidity_usd,
                price_usd: opp.buy_price,
                volume_24h_usd: 0.0, // not available from scanner
                pool_address: String::new(),
            });
        }
        let entry = chain_totals
            .entry(opp.chain_id)
            .or_insert_with(|| (opp.chain_name.clone(), 0.0, std::collections::HashSet::new(), 0));
        entry.1 += opp.liquidity_usd;
        entry.2.insert(opp.token.clone());
        entry.3 += 1;
    }

    let mut by_chain: Vec<LiquidityChainSummary> = chain_totals
        .iter()
        .map(|(chain_id, (name, liq, tokens, _count))| LiquidityChainSummary {
            chain_id: *chain_id,
            chain_name: name.clone(),
            total_liquidity_usd: *liq,
            dex_count: crate::chains::get_dexes_for_chain(*chain_id).len(),
            token_count: tokens.len(),
            percentage: 0.0,
        })
        .collect();

    let total: f64 = by_chain.iter().map(|c| c.total_liquidity_usd).sum();
    for c in &mut by_chain {
        c.percentage = if total > 0.0 {
            (c.total_liquidity_usd / total) * 100.0
        } else {
            0.0
        };
    }

    Json(LiquidityMapResponse {
        total_liquidity_usd: total,
        by_chain,
        by_dex: Vec::new(),
        data_points,
    })
}

// â”€â”€â”€ Bubble Chart â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

async fn get_bubble_data(State(state): State<AppState>) -> Json<Vec<BubbleData>> {
    // Build bubble chart from live scanner opportunities — no fabricated data.
    let opps = state.scanner.latest_opportunities();
    let mut bubbles = Vec::new();

    for opp in &opps {
        bubbles.push(BubbleData {
            token: opp.token.clone(),
            symbol: opp.token.clone(),
            price_usd: opp.buy_price,
            liquidity_usd: opp.liquidity_usd,
            market_cap_usd: 0.0, // not available from on-chain scanner
            chain_name: opp.chain_name.clone(),
            chain_id: opp.chain_id,
            has_opportunity: opp.profit_breakdown.is_profitable,
            opportunity_types: vec![opp.arbitrage_type.clone()],
            best_spread_pct: opp.spread_pct,
            volume_24h_usd: 0.0,
            price_change_24h_pct: 0.0,
            dexes_available: crate::chains::get_dexes_for_chain(opp.chain_id)
                .iter()
                .map(|d| d.name.clone())
                .collect(),
            bubble_size: (opp.liquidity_usd / 1_000.0).sqrt().min(100.0),
        });
    }

    Json(bubbles)
}

// â”€â”€â”€ Dashboard â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

async fn get_dashboard(State(state): State<AppState>) -> Json<DashboardData> {
    let _llm = state.llm_config.read().clone();
    let bot = state.bot_status.read().clone();

    let bubble_data = get_bubble_data(State(state.clone())).await;
    let liquidity_data = get_liquidity_data(State(state.clone())).await;

    // Read live opportunities from the continuous scanner if available.
    let live_opps = state.scanner.latest_opportunities();
    let profitable_count = live_opps.iter().filter(|o| o.profit_breakdown.is_profitable).count();
    let total_net: f64 = live_opps.iter().map(|o| o.profit_breakdown.net_profit_usd).sum();

    Json(DashboardData {
        bubbles: bubble_data.0,
        liquidity_map: liquidity_data.0,
        opportunities: live_opps.clone(),
        bot_status: bot,
        scan_timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        total_profit_24h_usd: total_net.max(0.0),
        total_opportunities_found: profitable_count,
    })
}

// â”€â”€â”€ Paper Trading / Backtesting â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

async fn paper_trading_start(State(state): State<AppState>) -> Json<serde_json::Value> {
    let mut pt = state.paper_trader.write();
    pt.start();
    info!("Paper trading started");
    Json(serde_json::json!({"status": "started", "balance_usd": pt.balance_usd}))
}

async fn paper_trading_stop(State(state): State<AppState>) -> Json<BacktestResult> {
    let mut pt = state.paper_trader.write();
    let result = pt.stop();
    Json(result)
}

async fn paper_trading_status(State(state): State<AppState>) -> Json<BacktestResult> {
    let pt = state.paper_trader.read();
    Json(pt.get_metrics())
}

async fn paper_simulate_trade(
    State(state): State<AppState>,
    Json(opp): Json<OpportunityDetail>,
) -> Json<serde_json::Value> {
    let mut pt = state.paper_trader.write();
    let trade = pt.simulate_trade(&opp);
    Json(serde_json::json!({
        "trade": trade,
        "balance_usd": pt.balance_usd,
        "total_trades": pt.total_trades,
        "win_rate_pct": pt.win_rate_pct,
    }))
}

async fn paper_run_backtest(State(state): State<AppState>) -> Json<BacktestResult> {
    let mut pt = state.paper_trader.write();
    pt.reset();
    pt.start();
    // Simulate some backtest trades
    for _ in 0..50 {
        let opp = OpportunityDetail {
            id: uuid::Uuid::new_v4().to_string(),
            token: "WETH".into(),
            token_address: "0x...".into(),
            arbitrage_type: ArbitrageType::Simple,
            chain_name: "Ethereum".into(),
            chain_id: 1,
            buy_dex: Some("Uniswap V3".into()),
            sell_dex: Some("Curve".into()),
            buy_price: 3445.0,
            sell_price: 3460.0 + rand::random::<f64>() * 10.0,
            spread_pct: 0.5 + rand::random::<f64>() * 2.0,
            profit_breakdown: {
                let gross = 100.0 + rand::random::<f64>() * 200.0;
                let gas = 10.0 + rand::random::<f64>() * 5.0;
                let fl = gross * 0.0003;
                let slip = gross * 0.005;
                let velora = gross * 0.001;
                let total_cost = gas + fl + slip + velora;
                let net = gross - total_cost;
                let notional = 10_000.0;
                NetProfitBreakdown {
                    gross_profit_usd: gross,
                    costs: CostBreakdown {
                        gas_estimated_usd: gas,
                        flash_loan_fee_usd: fl,
                        slippage_estimated_usd: slip,
                        bridge_fee_usd: None,
                        velora_fee_usd: velora,
                        total_cost_usd: total_cost,
                    },
                    net_profit_usd: net,
                    net_profit_pct: (net / notional) * 100.0,
                    roi_pct: (net / notional) * 100.0,
                    is_profitable: net > 0.0,
                }
            },
            flash_loan_recommendation: None,
            execution_steps: vec![],
            confidence_score: 0.8,
            liquidity_usd: 1_000_000.0,
            timestamp: chrono::Utc::now().timestamp() as u64,
        };
        pt.simulate_trade(&opp);
    }
    let result = pt.stop();
    Json(result)
}

async fn paper_trading_reset(State(state): State<AppState>) -> Json<serde_json::Value> {
    let mut pt = state.paper_trader.write();
    pt.reset();
    Json(serde_json::json!({"status": "reset", "balance_usd": pt.balance_usd}))
}

// â”€â”€â”€ Portfolio Manager â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

async fn get_portfolio_config(State(state): State<AppState>) -> Json<PortfolioConfig> {
    Json(state.portfolio_manager.read().config.clone())
}

async fn update_portfolio_config(
    State(state): State<AppState>,
    Json(config): Json<PortfolioConfig>,
) -> Json<PortfolioConfig> {
    let mut pm = state.portfolio_manager.write();
    pm.config = config.clone();
    Json(config)
}

async fn get_portfolio_status(
    State(state): State<AppState>,
) -> Json<crate::portfolio_manager::PortfolioStatus> {
    Json(state.portfolio_manager.read().get_status())
}

// â”€â”€â”€ MEV Guard â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

async fn get_mev_config(State(state): State<AppState>) -> Json<MevGuardConfig> {
    Json(state.mev_guard.read().config.clone())
}

async fn update_mev_config(
    State(state): State<AppState>,
    Json(config): Json<MevGuardConfig>,
) -> Json<MevGuardConfig> {
    let mut mg = state.mev_guard.write();
    mg.config = config.clone();
    Json(config)
}

async fn mev_analyze(
    State(state): State<AppState>,
    Json(req): Json<serde_json::Value>,
) -> Result<Json<crate::mev_guard::MevDetectionResult>, (StatusCode, String)> {
    let chain_id = req["chain_id"].as_u64().unwrap_or(1);
    let tx_data = req["tx_data"].as_str().unwrap_or("");
    state
        .mev_guard
        .read()
        .analyze_pending(chain_id, tx_data)
        .map(Json)
        .map_err(|e| (StatusCode::NOT_IMPLEMENTED, e))
}

// â”€â”€â”€ Alerts â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

async fn get_alerts_config(State(state): State<AppState>) -> Json<Vec<AlertConfig>> {
    Json(state.alert_manager.read().configs.clone())
}

async fn update_alerts_config(
    State(state): State<AppState>,
    Json(configs): Json<Vec<AlertConfig>>,
) -> Json<Vec<AlertConfig>> {
    let mut am = state.alert_manager.write();
    am.configs = configs.clone();
    Json(configs)
}

async fn get_alerts_history(
    State(state): State<AppState>,
) -> Json<Vec<crate::alert_manager::AlertMessage>> {
    Json(state.alert_manager.read().get_history(50))
}

async fn test_alert(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let mut am = state.alert_manager.write();
    let failures = am.send_alert(
        AlertEvent::BotStarted,
        "Test Alert",
        "This is a test alert from Zero-Cap Arbitrage",
    );

    // Previously this always answered {"status":"sent"} even though no channel ever
    // delivered anything (D-04). Report the real outcome so alerting is never assumed
    // to work when it does not.
    if failures.is_empty() {
        Ok(Json(serde_json::json!({ "status": "delivered" })))
    } else {
        Err((
            StatusCode::NOT_IMPLEMENTED,
            format!(
                "No alert was delivered: delivery is not implemented. {} channel(s) failed: {}",
                failures.len(),
                failures.join("; ")
            ),
        ))
    }
}

// â”€â”€â”€ Rules Engine â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

async fn get_rules(State(state): State<AppState>) -> Json<Vec<ExecutionRule>> {
    Json(state.rules_engine.read().rules.clone())
}

async fn update_rules(
    State(state): State<AppState>,
    Json(rules): Json<Vec<ExecutionRule>>,
) -> Json<Vec<ExecutionRule>> {
    let mut re = state.rules_engine.write();
    re.rules = rules.clone();
    Json(rules)
}

async fn evaluate_rules(
    State(state): State<AppState>,
    Json(opp): Json<OpportunityDetail>,
) -> Json<serde_json::Value> {
    let (execute, results) = state.rules_engine.read().should_execute(&opp);
    Json(serde_json::json!({
        "should_execute": execute,
        "results": results,
    }))
}

// â”€â”€â”€ Profit Splitter â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

async fn get_splitter_config(State(state): State<AppState>) -> Json<SplitterConfig> {
    Json(state.profit_splitter.read().config.clone())
}

async fn update_splitter_config(
    State(state): State<AppState>,
    Json(config): Json<SplitterConfig>,
) -> Json<SplitterConfig> {
    let mut ps = state.profit_splitter.write();
    ps.config = config.clone();
    Json(config)
}

async fn calculate_split(
    State(state): State<AppState>,
    Json(req): Json<serde_json::Value>,
) -> Json<crate::profit_splitter::ProfitSplitResult> {
    let total = req["total_profit_usd"].as_f64().unwrap_or(0.0);
    Json(state.profit_splitter.read().calculate_split(total))
}

// â”€â”€â”€ Gas Bidder â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

async fn get_gas_config(State(state): State<AppState>) -> Json<GasBidConfig> {
    Json(state.gas_bidder.read().config.clone())
}

async fn update_gas_config(
    State(state): State<AppState>,
    Json(config): Json<GasBidConfig>,
) -> Json<GasBidConfig> {
    let mut gb = state.gas_bidder.write();
    gb.config = config.clone();
    Json(config)
}

async fn recommend_gas(
    State(state): State<AppState>,
    Json(req): Json<serde_json::Value>,
) -> Json<crate::gas_bidder::GasRecommendation> {
    let chain_id = req["chain_id"].as_u64().unwrap_or(1);
    let profit = req["profit_usd"].as_f64().unwrap_or(0.0);
    let spread = req["spread_pct"].as_f64().unwrap_or(0.0);
    Json(
        state
            .gas_bidder
            .read()
            .recommend_gas(chain_id, profit, spread),
    )
}

// â”€â”€â”€ Chain / DEX Info â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

async fn list_chains() -> Json<Vec<ChainConfig>> {
    Json(crate::chains::get_chains().clone())
}

async fn list_dexes(Path(chain_id): Path<u64>) -> Json<Vec<DexConfig>> {
    Json(crate::chains::get_dexes_for_chain(chain_id))
}

// â”€â”€â”€ RPC pool health (point 6) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/// Per-chain health for the multi-endpoint RPC pool.
///
/// Exposes what the pool actually knows â€” which endpoints are cooling or
/// quarantined and how fast the best one is â€” rather than a single opaque URL.
async fn rpc_health(State(state): State<AppState>) -> Json<serde_json::Value> {
    let chains = crate::chains::get_chains();
    let per_chain: Vec<serde_json::Value> = chains
        .iter()
        .map(|c| {
            let h = state.rpc_pool.chain_health(c.id, &c.name);
            serde_json::json!({
                "chain_id": h.chain_id,
                "chain_name": h.chain_name,
                "total_endpoints": h.total_endpoints,
                "available_endpoints": h.available_endpoints,
                "cooling": h.cooling,
                "quarantined": h.quarantined,
                "best_latency_ms": if h.best_latency_ms < 0.0 { serde_json::Value::Null } else { serde_json::json!(h.best_latency_ms) },
            })
        })
        .collect();

    let total: usize = per_chain
        .iter()
        .map(|v| v["total_endpoints"].as_u64().unwrap_or(0) as usize)
        .sum();
    let available: usize = per_chain
        .iter()
        .map(|v| v["available_endpoints"].as_u64().unwrap_or(0) as usize)
        .sum();

    Json(serde_json::json!({
        "total_endpoints": total,
        "available_endpoints": available,
        "chains": per_chain,
    }))
}

// â”€â”€â”€ Profit transfer (point 7) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

async fn profit_transfer_config(State(state): State<AppState>) -> Json<serde_json::Value> {
    let cfg = state.profit_transfer.config().await;
    let errs = cfg.validate();
    Json(serde_json::json!({
        "mode": if cfg.auto { "AUTO" } else { "MANUAL" },
        "min_usd": cfg.min_usd,
        "max_usd": cfg.max_usd,
        "interval_secs": cfg.interval_secs,
        "accrued_usd": state.profit_transfer.accrued_usd().await,
        "destinations": cfg.destinations.iter().map(|d| serde_json::json!({
            "address": d.address.as_str(),
            "label": d.label,
            "share_pct": d.share_pct,
            "enabled": d.enabled,
        })).collect::<Vec<_>>(),
        "total_share_pct": cfg.total_share_pct(),
        "config_errors": errs,
    }))
}

async fn profit_transfer_preview(
    State(state): State<AppState>,
    Json(req): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let amount = req["amount_usd"].as_f64().ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "amount_usd must be a number".into(),
        )
    })?;
    let p = state.profit_transfer.preview(amount).await;
    Ok(Json(
        serde_json::to_value(p).unwrap_or(serde_json::Value::Null),
    ))
}

/// MANUAL withdrawal trigger. Available regardless of AUTO/MANUAL mode.
async fn profit_transfer_manual(
    State(state): State<AppState>,
    Json(req): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    // Default to the full accrued balance when no amount is given.
    let amount = match req["amount_usd"].as_f64() {
        Some(a) => a,
        None => state.profit_transfer.accrued_usd().await,
    };
    let r = state.profit_transfer.transfer(amount).await;
    // A refusal is not a server fault: return 200 with the structured reason
    // so the client can show *why* the transfer was declined. Only a malformed
    // request is a 400.
    let body = serde_json::to_value(&r).unwrap_or(serde_json::Value::Null);
    if r.ok {
        Ok(Json(body))
    } else {
        Ok(Json(serde_json::json!({
            "ok": false,
            "error": body.get("reason").cloned().unwrap_or(serde_json::Value::Null),
            "result": body,
        })))
    }
}

async fn profit_transfer_history(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "transfers": state.profit_transfer.history().await }))
}
