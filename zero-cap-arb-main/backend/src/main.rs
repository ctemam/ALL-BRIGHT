mod alert_manager;
mod api;
mod chains;
mod config;
mod gas_bidder;
mod mev_guard;
mod multicall;
mod paper_trader;
mod pimlico_client;
mod portfolio_manager;
mod profit_splitter;
mod profit_transfer;
mod radar_scanner;
mod rpc_catalog;
mod rpc_pool;
mod rules_engine;
mod types;
mod velora_client;
mod websocket;

use alert_manager::{AlertConfig, AlertEvent, AlertManager};
use api::{build_router, AppState};
use gas_bidder::{GasBidConfig, GasBidStrategy, GasBidder};
use mev_guard::{MevGuard, MevGuardConfig, MevRiskLevel};
use paper_trader::{PaperTradeMode, PaperTrader};
use portfolio_manager::{PortfolioConfig, PortfolioManager, StrategyAllocation};
use profit_splitter::{ProfitSplitter, SplitterConfig, SplitterWallet};
use radar_scanner::RadarScanner;
use rules_engine::{ExecutionRule, RuleAction, RuleField, RuleOperator, RulesEngine};
use std::sync::Arc;
use std::time::Instant;
use tokio::signal;
use tower_http::cors::CorsLayer;
use tracing::info;
use tracing_subscriber::EnvFilter;
use types::{ArbitrageType, BotConfig, BotMode, FlashLoanSource, GasStrategy};
use velora_client::VeloraClient;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    dotenvy::dotenv().ok();

    info!("Starting Zero-Capital Arbitrage Backend (Velora engine)...");

    let chains = chains::get_chains();
    info!("Loaded {} chains", chains.len());
    for chain in chains {
        let dex_count = chains::get_dexes_for_chain(chain.id).len();
        info!(
            "  Chain {}: {} (id: {}, {} DEXes)",
            chain.id, chain.name, chain.id, dex_count
        );
    }

    let state = AppState {
        scanner: Arc::new(RadarScanner::new()),
        velora: Arc::new(VeloraClient::new(std::env::var("VELORA_API_KEY").ok())),
        pimlico: Arc::new(pimlico_client::PimlicoClient::new(
            std::env::var("PIMLICO_API_KEY").ok(),
        )),
        start_time: Instant::now(),
        bot_status: Arc::new(parking_lot::RwLock::new(None)),
        bot_config: Arc::new(parking_lot::RwLock::new(BotConfig {
            mode: BotMode::Manual,
            min_net_profit_usd: 10.0,
            max_gas_price_gwei: Some(100.0),
            enabled_strategies: vec![
                ArbitrageType::Simple,
                ArbitrageType::Triangular,
                ArbitrageType::CrossChain,
            ],
            flash_loan_sources: vec![
                FlashLoanSource::Spark,
                FlashLoanSource::AaveV3,
                FlashLoanSource::RadiantV2,
            ],
            gas_strategy: GasStrategy::Flashbots,
            max_slippage_pct: 1.0,
            auto_restart: true,
            llm_advisor: false,
            llm_config: None,
            scan_interval_secs: 30,
            max_concurrent_tx: 3,
            chains_enabled: vec![1, 42161, 10, 137, 56, 43114, 8453, 42220, 100, 59144],
            dexes_enabled: vec![],
        })),
        llm_config: Arc::new(parking_lot::RwLock::new(None)),
        paper_trader: Arc::new(parking_lot::RwLock::new(PaperTrader::new(
            PaperTradeMode::LiveSimulation,
            10_000.0,
        ))),
        portfolio_manager: Arc::new(parking_lot::RwLock::new(PortfolioManager::new(
            PortfolioConfig {
                strategies: vec![
                    StrategyAllocation {
                        strategy: ArbitrageType::Simple,
                        weight_pct: 40.0,
                        max_concurrent: 2,
                        min_profit_usd: 10.0,
                        max_daily_trades: 50,
                        daily_trades: 0,
                    },
                    StrategyAllocation {
                        strategy: ArbitrageType::Triangular,
                        weight_pct: 25.0,
                        max_concurrent: 2,
                        min_profit_usd: 20.0,
                        max_daily_trades: 30,
                        daily_trades: 0,
                    },
                    StrategyAllocation {
                        strategy: ArbitrageType::CrossChain,
                        weight_pct: 20.0,
                        max_concurrent: 1,
                        min_profit_usd: 50.0,
                        max_daily_trades: 20,
                        daily_trades: 0,
                    },
                    StrategyAllocation {
                        strategy: ArbitrageType::Mint,
                        weight_pct: 10.0,
                        max_concurrent: 1,
                        min_profit_usd: 5.0,
                        max_daily_trades: 40,
                        daily_trades: 0,
                    },
                    StrategyAllocation {
                        strategy: ArbitrageType::JitLiquidity,
                        weight_pct: 5.0,
                        max_concurrent: 1,
                        min_profit_usd: 100.0,
                        max_daily_trades: 10,
                        daily_trades: 0,
                    },
                ],
                total_balance_usd: 10_000.0,
                risk_per_trade_pct: 2.0,
                max_daily_loss_usd: 500.0,
                daily_loss: 0.0,
                max_open_positions: 5,
                open_positions: 0,
            },
        ))),
        mev_guard: Arc::new(parking_lot::RwLock::new(MevGuard::new(MevGuardConfig {
            enabled: true,
            block_sandwich: true,
            block_frontrun: true,
            block_backrun: true,
            max_risk_level: MevRiskLevel::MediumRisk,
            use_flashbots: true,
            use_private_mempool: true,
            delay_seconds: 0,
            honeypot_check: true,
        }))),
        alert_manager: Arc::new(parking_lot::RwLock::new(AlertManager::new(vec![]))),
        rules_engine: Arc::new(parking_lot::RwLock::new(RulesEngine::new(vec![
            ExecutionRule {
                id: "rule-001".into(),
                name: "Min Spread 1%".into(),
                enabled: true,
                field: RuleField::SpreadPct,
                operator: RuleOperator::GreaterThan,
                value: "1.0".into(),
                action: RuleAction::Execute,
                priority: 1,
            },
            ExecutionRule {
                id: "rule-002".into(),
                name: "Min Profit $20".into(),
                enabled: true,
                field: RuleField::NetProfitUsd,
                operator: RuleOperator::GreaterThan,
                value: "20.0".into(),
                action: RuleAction::Execute,
                priority: 2,
            },
            ExecutionRule {
                id: "rule-003".into(),
                name: "Min Confidence 0.5".into(),
                enabled: true,
                field: RuleField::ConfidenceScore,
                operator: RuleOperator::GreaterThan,
                value: "0.5".into(),
                action: RuleAction::Execute,
                priority: 3,
            },
        ]))),
        profit_splitter: Arc::new(parking_lot::RwLock::new(ProfitSplitter::new(
            SplitterConfig {
                wallets: vec![
                    SplitterWallet {
                        address: "0xYourWallet".into(),
                        label: "Your Wallet".into(),
                        share_pct: 70.0,
                        enabled: true,
                    },
                    SplitterWallet {
                        address: "0xReserve".into(),
                        label: "Reserve Fund".into(),
                        share_pct: 20.0,
                        enabled: true,
                    },
                    SplitterWallet {
                        address: "0xCharity".into(),
                        label: "Dev Fund".into(),
                        share_pct: 10.0,
                        enabled: true,
                    },
                ],
                enabled: true,
                min_split_profit_usd: 10.0,
            },
        ))),
        gas_bidder: Arc::new(parking_lot::RwLock::new(GasBidder::new(GasBidConfig {
            strategy: GasBidStrategy::Adaptive,
            max_gas_price_gwei: 100.0,
            min_gas_price_gwei: 1.0,
            priority_pct: 10.0,
            adaptive_enabled: true,
        }))),
        profit_transfer: Arc::new(profit_transfer::ProfitTransferService::new(
            profit_transfer::ProfitTransferConfig::from_env(),
        )),
        rpc_pool: {
            // Register every built-in endpoint for every chain. The pool
            // dedupes and drops non-HTTP entries, so a stale placeholder in
            // the catalogue cannot break startup.
            let pool = Arc::new(rpc_pool::RpcPool::new(
                crate::config::ServerConfig::from_env().rpc_max_concurrency,
            ));
            for chain in chains::get_chains() {
                let mut urls = rpc_catalog::all_default_endpoints(chain.id);
                // Operator-supplied endpoints are appended so they take part in
                // the same health tracking and failover.
                urls.extend(env_rpc_urls(chain.id));
                pool.register(chain.id, &urls);
            }
            info!(
                "RPC pool registered {} endpoints across {} chains",
                pool.total_endpoints(),
                chains::get_chains().len()
            );
            pool
        },
    };

    let app = build_router(state.clone()).layer(CorsLayer::permissive());

    // Start the AUTO profit sweep. Defaults to AUTO when PROFIT_WALLET is
    // set and PROFIT_TRANSFER_MODE is not explicitly MANUAL.
    {
        let cfg = state.profit_transfer.config().await;
        info!(
            auto = cfg.auto,
            min_usd = cfg.min_usd,
            destinations = cfg.destinations.len(),
            interval_secs = cfg.interval_secs,
            "profit transfer config loaded"
        );
    }
    state.profit_transfer.spawn_auto_sweep();

    // Start the continuous scanner. It refreshes native/USD rates from on-chain
    // pools, scans all tokens across all chains, and reports profitable
    // opportunities to the dashboard. When ZCA_AUTO_EXECUTE=true, it also
    // attempts to execute via Velora routing.
    let scan_interval = std::env::var("ZCA_SCAN_INTERVAL_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(30);
    state.scanner.spawn_continuous_scanner(
        scan_interval,
        state.velora.clone(),
        state.profit_transfer.clone(),
    );

    let cfg = config::ServerConfig::from_env();
    info!(
        mode = %cfg.mode,
        bind = %cfg.bind_address(),
        region = %cfg.region,
        profit_transfer = %cfg.profit_transfer_mode,
    );

    let listener = tokio::net::TcpListener::bind(cfg.bind_address())
        .await
        .unwrap_or_else(|e| panic!("failed to bind {}: {}", cfg.bind_address(), e));
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap();

    info!("Server shut down");
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };
    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("...")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => info!("Received Ctrl+C"),
        _ = terminate => info!("Received SIGTERM"),
    }
}

/// Extra RPC endpoints for a chain supplied via environment variables.
///
/// Recognises both `CHAIN_RPC_URL="a,b"` and numbered `CHAIN_RPC_URL_1`,
/// `CHAIN_RPC_URL_2`, ... variants, matching `chains::read_rpc_urls`.
fn env_rpc_urls(chain_id: u64) -> Vec<String> {
    let key = match chain_id {
        1 => "ETH_RPC_URL",
        42161 => "ARB_RPC_URL",
        10 => "OP_RPC_URL",
        137 => "POLY_RPC_URL",
        56 => "BSC_RPC_URL",
        43114 => "AVAX_RPC_URL",
        8453 => "BASE_RPC_URL",
        42220 => "CELO_RPC_URL",
        100 => "GNOSIS_RPC_URL",
        59144 => "LINEA_RPC_URL",
        _ => return Vec::new(),
    };

    let mut out: Vec<String> = std::env::var(key)
        .ok()
        .map(|v| {
            v.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();

    for i in 1..=8 {
        if let Ok(v) = std::env::var(format!("{key}_{i}")) {
            let t = v.trim();
            if !t.is_empty() {
                out.push(t.to_string());
            }
        }
    }
    out
}
