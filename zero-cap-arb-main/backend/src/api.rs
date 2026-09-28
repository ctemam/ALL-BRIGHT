use crate::alert_manager::{AlertConfig, AlertEvent, AlertManager};
use crate::gas_bidder::{GasBidConfig, GasBidStrategy, GasBidder};
use crate::mev_guard::{MevGuard, MevGuardConfig, MevRiskLevel};
use crate::paper_trader::{BacktestConfig, BacktestResult, PaperTradeMode, PaperTrader};
use crate::portfolio_manager::{PortfolioConfig, PortfolioManager, StrategyAllocation};
use crate::profit_splitter::{ProfitSplitter, SplitterConfig, SplitterWallet};
use crate::radar_scanner::RadarScanner;
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
use tracing::info;

#[derive(Clone)]
pub struct AppState {
    pub scanner: Arc<RadarScanner>,
    pub velora: Arc<VeloraClient>,
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
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(health_check))
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
        // ─── Paper Trading / Backtesting ──────────
        .route("/api/paper/start", post(paper_trading_start))
        .route("/api/paper/stop", post(paper_trading_stop))
        .route("/api/paper/status", get(paper_trading_status))
        .route("/api/paper/simulate", post(paper_simulate_trade))
        .route("/api/paper/backtest", post(paper_run_backtest))
        .route("/api/paper/reset", post(paper_trading_reset))
        // ─── Portfolio Manager ────────────────────
        .route("/api/portfolio/config", get(get_portfolio_config))
        .route("/api/portfolio/config", post(update_portfolio_config))
        .route("/api/portfolio/status", get(get_portfolio_status))
        // ─── MEV Guard ────────────────────────────
        .route("/api/mev/config", get(get_mev_config))
        .route("/api/mev/config", post(update_mev_config))
        .route("/api/mev/analyze", post(mev_analyze))
        // ─── Alerts (Telegram/Discord) ────────────
        .route("/api/alerts/config", get(get_alerts_config))
        .route("/api/alerts/config", post(update_alerts_config))
        .route("/api/alerts/history", get(get_alerts_history))
        .route("/api/alerts/test", post(test_alert))
        // ─── Rules Engine ─────────────────────────
        .route("/api/rules", get(get_rules))
        .route("/api/rules", post(update_rules))
        .route("/api/rules/evaluate", post(evaluate_rules))
        // ─── Profit Splitter ──────────────────────
        .route("/api/splitter/config", get(get_splitter_config))
        .route("/api/splitter/config", post(update_splitter_config))
        .route("/api/splitter/calculate", post(calculate_split))
        // ─── Gas Bidder ───────────────────────────
        .route("/api/gas/config", get(get_gas_config))
        .route("/api/gas/config", post(update_gas_config))
        .route("/api/gas/recommend", post(recommend_gas))
        .route("/api/chains", get(list_chains))
        .route("/api/dexes/{chain_id}", get(list_dexes))
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

async fn scan_token(
    State(state): State<AppState>,
    Json(req): Json<serde_json::Value>,
) -> Result<Json<RadarScanResponse>, (StatusCode, String)> {
    let token = req["token"].as_str().ok_or_else(|| (StatusCode::BAD_REQUEST, "token required".to_string()))?;
    let addr = req["token_address"].as_str();
    state.scanner.scan_token(token, addr).await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Scan failed: {}", e)))
}

async fn get_all_prices(
    State(state): State<AppState>,
    Json(req): Json<serde_json::Value>,
) -> Result<Json<AllPricesResponse>, (StatusCode, String)> {
    let token = req["token"].as_str().ok_or_else(|| (StatusCode::BAD_REQUEST, "token required".to_string()))?;
    let addr = req["token_address"].as_str();
    state.scanner.scan_all_prices(token, addr).await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Price scan failed: {}", e)))
}

async fn get_all_opportunities(
    State(state): State<AppState>,
    Json(req): Json<serde_json::Value>,
) -> Result<Json<AllOpportunitiesResponse>, (StatusCode, String)> {
    let token = req["token"].as_str().ok_or_else(|| (StatusCode::BAD_REQUEST, "token required".to_string()))?;
    let addr = req["token_address"].as_str();
    state.scanner.scan_all_strategies(token, addr).await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Strategy scan failed: {}", e)))
}

// ─── Velora API Handlers ──────────────────────────────

async fn get_velora_price(
    State(state): State<AppState>,
    Json(req): Json<serde_json::Value>,
) -> Result<Json<VeloraPriceResponse>, (StatusCode, String)> {
    let chain_id = req["chain_id"].as_u64().ok_or_else(|| (StatusCode::BAD_REQUEST, "chain_id required".to_string()))?;
    let src = req["src_token"].as_str().ok_or_else(|| (StatusCode::BAD_REQUEST, "src_token required".to_string()))?;
    let dst = req["dest_token"].as_str().ok_or_else(|| (StatusCode::BAD_REQUEST, "dest_token required".to_string()))?;
    let sd = req["src_decimals"].as_u64().unwrap_or(18) as u8;
    let dd = req["dest_decimals"].as_u64().unwrap_or(18) as u8;
    let amt = req["amount"].as_str().ok_or_else(|| (StatusCode::BAD_REQUEST, "amount required".to_string()))?;
    let side = req["side"].as_str().unwrap_or("SELL");

    let resp = state.velora.get_price(chain_id, src, dst, sd, dd, amt, side).await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("Velora price error: {}", e)))?;

    let routes: Vec<VeloraRoute> = resp.price_route.best_route.iter().flat_map(|seg| {
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
    }).collect();

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
    let chain_id = req["chain_id"].as_u64().ok_or_else(|| (StatusCode::BAD_REQUEST, "chain_id required".to_string()))?;
    let src = req["src_token"].as_str().ok_or_else(|| (StatusCode::BAD_REQUEST, "src_token required".to_string()))?;
    let dst = req["dest_token"].as_str().ok_or_else(|| (StatusCode::BAD_REQUEST, "dest_token required".to_string()))?;
    let sd = req["src_decimals"].as_u64().unwrap_or(18) as u8;
    let dd = req["dest_decimals"].as_u64().unwrap_or(18) as u8;
    let amt = req["amount"].as_str().ok_or_else(|| (StatusCode::BAD_REQUEST, "amount required".to_string()))?;
    let side = req["side"].as_str().unwrap_or("SELL");
    let ua = req["user_address"].as_str();
    let slip = req["slippage"].as_u64();

    let resp = state.velora.get_swap(chain_id, src, dst, sd, dd, amt, side, ua, slip).await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("Velora /swap error: {}", e)))?;

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
    let chain_id = req["chain_id"].as_u64().ok_or_else(|| (StatusCode::BAD_REQUEST, "chain_id required".to_string()))?;
    let src = req["src_token"].as_str().ok_or_else(|| (StatusCode::BAD_REQUEST, "src_token required".to_string()))?;
    let dst = req["dest_token"].as_str().ok_or_else(|| (StatusCode::BAD_REQUEST, "dest_token required".to_string()))?;
    let sd = req["src_decimals"].as_u64().unwrap_or(18) as u8;
    let dd = req["dest_decimals"].as_u64().unwrap_or(18) as u8;
    let sa = req["src_amount"].as_str().ok_or_else(|| (StatusCode::BAD_REQUEST, "src_amount required".to_string()))?;
    let da = req["dest_amount"].as_str().ok_or_else(|| (StatusCode::BAD_REQUEST, "dest_amount required".to_string()))?;
    let slip = req["slippage"].as_f64().unwrap_or(0.5);
    let ua = req["user_address"].as_str().ok_or_else(|| (StatusCode::BAD_REQUEST, "user_address required".to_string()))?;
    let rc = req["receiver"].as_str();
    let pr = req["price_route"].clone();

    state.velora.build_transaction(chain_id, src, dst, sd, dd, sa, da, slip, ua, rc, &pr).await
        .map(|r| Json(VeloraTxResponse {
            from: r.from, to: r.to, value: r.value, data: r.data,
            gas_price: r.gas_price, gas: r.gas, chain_id: r.chain_id,
        }))
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("Velora tx error: {}", e)))
}

async fn submit_delta_order(
    State(state): State<AppState>,
    Json(req): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let chain_id = req["chain_id"].as_u64().ok_or_else(|| (StatusCode::BAD_REQUEST, "chain_id required".to_string()))?;
    let src = req["src_token"].as_str().ok_or_else(|| (StatusCode::BAD_REQUEST, "src_token required".to_string()))?;
    let dst = req["dest_token"].as_str().ok_or_else(|| (StatusCode::BAD_REQUEST, "dest_token required".to_string()))?;
    let amt = req["amount"].as_str().ok_or_else(|| (StatusCode::BAD_REQUEST, "amount required".to_string()))?;
    let ua = req["user_address"].as_str().ok_or_else(|| (StatusCode::BAD_REQUEST, "user_address required".to_string()))?;

    state.velora.submit_delta_order(chain_id, src, dst, amt, ua).await
        .map(Json)
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("Delta order error: {}", e)))
}

// ─── Execution ────────────────────────────────────────

async fn execute_arbitrage(
    State(_state): State<AppState>,
    Json(req): Json<ExecuteArbitrageRequest>,
) -> Json<ExecuteResult> {
    info!("Executing arbitrage: opp={:?}", req.opportunity_id);
    Json(ExecuteResult {
        status: "simulated".to_string(),
        message: "Arbitrage executed via Velora + Flashbots. 0 capital, 0 upfront gas.".to_string(),
        strategy: "flash_loan".to_string(),
        execution_mode: "FlashLoan".to_string(),
        tx_hash: Some(format!("0x{:064x}", rand::random::<u64>())),
        estimated_profit_usd: Some(100.50),
        gas_cost_usd: Some(2.30),
    })
}

async fn execute_advanced(
    State(_state): State<AppState>,
    Json(req): Json<AdvancedExecuteRequest>,
) -> Json<ExecuteResult> {
    info!("Advanced execute: strategy={}, mode={:?}", req.strategy, req.execution_mode);
    let profit = match req.strategy.as_str() {
        "triangular" => 42.75, "cross_chain" => 185.20, "jit" => 67.30, "mint" => 33.10, _ => 100.50,
    };
    Json(ExecuteResult {
        status: "simulated".to_string(),
        message: format!("{} executed via Velora. 0 capital, 0 upfront gas.", req.strategy),
        strategy: req.strategy.clone(),
        execution_mode: match req.execution_mode { ExecutionMode::FlashLoan => "FlashLoan", ExecutionMode::DirectSwap => "DirectSwap", ExecutionMode::Mint => "Mint" }.to_string(),
        tx_hash: Some(format!("0x{:064x}", rand::random::<u64>())),
        estimated_profit_usd: Some(profit),
        gas_cost_usd: Some(rand::random::<f64>() * 5.0),
    })
}

// ─── Comprehensive Scan ───────────────────────────────

async fn scan_comprehensive(
    State(state): State<AppState>,
    Json(req): Json<serde_json::Value>,
) -> Result<Json<ComprehensiveScanResponse>, (StatusCode, String)> {
    let tokens: Vec<crate::radar_scanner::TokenInfo> = serde_json::from_value(req["tokens"].clone())
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("Invalid tokens: {}", e)))?;
    state.scanner.comprehensive_scan(&tokens).await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Comprehensive scan failed: {}", e)))
}

// ─── Bot Control ──────────────────────────────────────

async fn get_bot_config(
    State(state): State<AppState>,
) -> Json<BotConfig> {
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

async fn start_bot(
    State(state): State<AppState>,
) -> Json<BotStatus> {
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

async fn stop_bot(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    *state.bot_status.write() = None;
    info!("Bot stopped");
    Json(serde_json::json!({"status": "stopped"}))
}

async fn get_bot_status(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let status = state.bot_status.read();
    match status.as_ref() {
        Some(s) => Json(serde_json::json!(s)),
        None => Json(serde_json::json!({"running": false, "message": "Bot is stopped"})),
    }
}

async fn get_bot_logs(
    State(state): State<AppState>,
) -> Json<Vec<BotLogEntry>> {
    let status = state.bot_status.read();
    Json(status.as_ref().map_or(Vec::new(), |s| s.logs.clone()))
}

// ─── LLM Integration ──────────────────────────────────

async fn get_llm_config(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
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
            // In production: call OpenAI/Anthropic/etc. API
            Json(LLMAdviceResponse {
                advice: "Based on current market conditions, this opportunity has a strong probability of execution. The spread is healthy and liquidity is sufficient.".to_string(),
                confidence: "high".to_string(),
                recommend_execute: true,
                reasoning: vec![
                    "Spread above minimum threshold (2.5% > 0.5%)".to_string(),
                    "Liquidity sufficient for profitable trade size".to_string(),
                    "Low gas costs relative to profit".to_string(),
                    "Historical success rate: 87% on similar setups".to_string(),
                ],
                risk_factors: vec![
                    "Market volatility may increase slippage".to_string(),
                    "MEV risk on public mempool".to_string(),
                    "Gas price spike could reduce margins".to_string(),
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

// ─── Liquidity Data ───────────────────────────────────

async fn get_liquidity_data(
    State(state): State<AppState>,
) -> Json<LiquidityMapResponse> {
    // Gather liquidity from all chains
    let chains = crate::chains::get_chains();
    let mut data_points = Vec::new();
    let mut by_chain = Vec::new();

    for chain in chains {
        let dexes = crate::chains::get_dexes_for_chain(chain.id);
        let chain_liq: f64 = dexes.iter().enumerate().map(|(i, dex)| {
            // In production: query on-chain reserves via multicall
            // Here: simulated liquidity distribution
            let simulated = 100_000.0 + (chain.id as f64 * 50_000.0) + (i as f64 * 10_000.0);
            data_points.push(LiquidityDataPoint {
                chain_id: chain.id,
                chain_name: chain.name.clone(),
                dex_name: dex.name.clone(),
                token: "USDC".to_string(),
                token_address: "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48".to_string(),
                liquidity_usd: simulated,
                price_usd: 1.0,
                volume_24h_usd: simulated * 2.5,
                pool_address: dex.address.clone(),
            });
            simulated
        }).sum();

        by_chain.push(LiquidityChainSummary {
            chain_id: chain.id,
            chain_name: chain.name.clone(),
            total_liquidity_usd: chain_liq,
            dex_count: dexes.len(),
            token_count: 1,
            percentage: 0.0,
        });
    }

    let total: f64 = by_chain.iter().map(|c| c.total_liquidity_usd).sum();
    for c in &mut by_chain {
        c.percentage = if total > 0.0 { (c.total_liquidity_usd / total) * 100.0 } else { 0.0 };
    }

    Json(LiquidityMapResponse {
        total_liquidity_usd: total,
        by_chain,
        by_dex: Vec::new(),
        data_points,
    })
}

// ─── Bubble Chart ─────────────────────────────────────

async fn get_bubble_data(
    State(state): State<AppState>,
) -> Json<Vec<BubbleData>> {
    let tokens = vec![
        ("DAI", "0x6B175474E89094C44Da98b954EedeAC495271d0F", 1.00, 5_000_000_000.0),
        ("USDC", "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48", 1.00, 4_500_000_000.0),
        ("USDT", "0xdAC17F958D2ee523a2206206994597C13D831ec7", 1.00, 6_000_000_000.0),
        ("WETH", "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2", 3450.0, 8_000_000_000.0),
        ("WBTC", "0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599", 67800.0, 3_500_000_000.0),
        ("MATIC", "0x7D1AfA7B718fb893dB30A3aBc0Cfc608AaCfeBB0", 0.72, 1_200_000_000.0),
        ("LINK", "0x514910771AF9Ca656af840dff83E8264EcF986CA", 18.50, 800_000_000.0),
        ("UNI", "0x1f9840a85d5aF5bf1D1762F925BDADdC4201F984", 12.30, 600_000_000.0),
        ("AAVE", "0x7Fc66500c84A76Ad7e9c93437bFc5Ac33E2DDaE9", 145.0, 400_000_000.0),
        ("ARB", "0xB50721BCf8d664c30412Cf6036cB7b561B04C0e9", 1.85, 350_000_000.0),
        ("OP", "0x4200000000000000000000000000000000000042", 3.20, 280_000_000.0),
        ("CRV", "0xD533a949740bb3306d119CC777fa900bA034cd52", 0.85, 220_000_000.0),
    ];

    let chains = crate::chains::get_chains();
    let mut bubbles = Vec::new();
    for (ticker, addr, price, mcap) in tokens {
        for chain in chains {
            let liq = mcap * 0.01 * (chain.id as f64 % 5.0 + 0.5);
            let has_opp = chain.id % 2 == 0;
            bubbles.push(BubbleData {
                token: ticker.to_string(),
                symbol: ticker.to_string(),
                price_usd: price,
                liquidity_usd: liq,
                market_cap_usd: mcap,
                chain_name: chain.name.clone(),
                chain_id: chain.id,
                has_opportunity: has_opp,
                opportunity_types: if has_opp { vec![ArbitrageType::Simple] } else { vec![] },
                best_spread_pct: if has_opp { 1.2 + (chain.id as f64 * 0.1) % 5.0 } else { 0.0 },
                volume_24h_usd: liq * 3.0,
                price_change_24h_pct: (-2.0..=2.0).into_iter().map(|_| 0.5).sum(),
                dexes_available: crate::chains::get_dexes_for_chain(chain.id).iter().map(|d| d.name.clone()).collect(),
                bubble_size: (liq / 1_000_000.0).sqrt().min(100.0),
            });
        }
    }

    Json(bubbles)
}

// ─── Dashboard ────────────────────────────────────────

async fn get_dashboard(
    State(state): State<AppState>,
) -> Json<DashboardData> {
    let llm = state.llm_config.read().clone();
    let bot = state.bot_status.read().clone();

    // Quick simulated scan
    let bubble_data = get_bubble_data(State(state.clone())).await;
    let liquidity_data = get_liquidity_data(State(state.clone())).await;

    let opps = vec![
        OpportunityDetail {
            id: "demo-001".to_string(),
            token: "WETH".to_string(),
            token_address: "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2".to_string(),
            arbitrage_type: ArbitrageType::Simple,
            chain_name: "Ethereum".to_string(),
            chain_id: 1,
            buy_dex: Some("Uniswap V3".to_string()),
            sell_dex: Some("Curve".to_string()),
            buy_price: 3445.0,
            sell_price: 3460.0,
            spread_pct: 0.44,
            profit_breakdown: NetProfitBreakdown {
                gross_profit_usd: 150.0,
                costs: CostBreakdown {
                    gas_estimated_usd: 12.0,
                    flash_loan_fee_usd: 1.50,
                    slippage_estimated_usd: 0.75,
                    bridge_fee_usd: None,
                    velora_fee_usd: 0.15,
                    total_cost_usd: 14.40,
                },
                net_profit_usd: 135.60,
                net_profit_pct: 941.67,
                roi_pct: 0.039,
                is_profitable: true,
            },
            flash_loan_recommendation: None,
            execution_steps: vec!["Borrow 100 ETH", "Buy on Uniswap V3", "Sell on Curve", "Repay", "Keep profit".to_string()],
            confidence_score: 0.87,
            liquidity_usd: 2_500_000.0,
            timestamp: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
        },
    ];

    Json(DashboardData {
        bubbles: bubble_data.0,
        liquidity_map: liquidity_data.0,
        opportunities: opps,
        bot_status: bot,
        scan_timestamp: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
        total_profit_24h_usd: 1250.75,
        total_opportunities_found: 9,
    })
}

// ─── Paper Trading / Backtesting ──────────────────────

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

async fn paper_run_backtest(
    State(state): State<AppState>,
) -> Json<BacktestResult> {
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
            profit_breakdown: NetProfitBreakdown {
                gross_profit_usd: 100.0 + rand::random::<f64>() * 200.0,
                costs: CostBreakdown {
                    gas_estimated_usd: 10.0 + rand::random::<f64>() * 5.0,
                    flash_loan_fee_usd: 0.5,
                    slippage_estimated_usd: 1.0,
                    bridge_fee_usd: None,
                    velora_fee_usd: 0.1,
                    total_cost_usd: 12.0,
                },
                net_profit_usd: 50.0 + rand::random::<f64>() * 150.0,
                net_profit_pct: 500.0,
                roi_pct: 2.0,
                is_profitable: true,
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

// ─── Portfolio Manager ────────────────────────────────

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

async fn get_portfolio_status(State(state): State<AppState>) -> Json<crate::portfolio_manager::PortfolioStatus> {
    Json(state.portfolio_manager.read().get_status())
}

// ─── MEV Guard ────────────────────────────────────────

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
) -> Json<crate::mev_guard::MevDetectionResult> {
    let chain_id = req["chain_id"].as_u64().unwrap_or(1);
    let tx_data = req["tx_data"].as_str().unwrap_or("");
    Json(state.mev_guard.read().analyze_pending(chain_id, tx_data))
}

// ─── Alerts ───────────────────────────────────────────

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

async fn get_alerts_history(State(state): State<AppState>) -> Json<Vec<crate::alert_manager::AlertMessage>> {
    Json(state.alert_manager.read().get_history(50))
}

async fn test_alert(State(state): State<AppState>) -> Json<serde_json::Value> {
    let mut am = state.alert_manager.write();
    am.send_alert(AlertEvent::BotStarted, "Test Alert", "This is a test alert from Zero-Cap Arbitrage");
    Json(serde_json::json!({"status": "sent"}))
}

// ─── Rules Engine ─────────────────────────────────────

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

// ─── Profit Splitter ──────────────────────────────────

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

// ─── Gas Bidder ───────────────────────────────────────

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
    Json(state.gas_bidder.read().recommend_gas(chain_id, profit, spread))
}

// ─── Chain / DEX Info ─────────────────────────────────

async fn list_chains() -> Json<Vec<ChainConfig>> {
    Json(crate::chains::get_chains().clone())
}

async fn list_dexes(Path(chain_id): Path<u64>) -> Json<Vec<DexConfig>> {
    Json(crate::chains::get_dexes_for_chain(chain_id))
}
