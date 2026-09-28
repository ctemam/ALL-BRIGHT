use serde::{Deserialize, Serialize};

// ─── Chain & DEX ──────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainConfig {
    pub id: u64,
    pub name: String,
    pub rpc_url: String,
    pub rpc_urls: Vec<String>,
    pub native_currency: String,
    pub explorer_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DexConfig {
    pub name: String,
    pub address: String,
    pub chain_id: u64,
    pub router_abi: Option<String>,
}

// ─── Price Data ───────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenPrice {
    pub token: String,
    pub token_address: String,
    pub chain_id: u64,
    pub chain_name: String,
    pub dex_name: String,
    pub price_usd: f64,
    pub liquidity_usd: f64,
    pub timestamp: u64,
}

/// All prices response – shows every DEX price across every chain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AllPricesResponse {
    pub token: String,
    pub token_address: String,
    pub prices: Vec<TokenPrice>,
    pub chain_summary: Vec<ChainSummary>,
    pub scan_time_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainSummary {
    pub chain_id: u64,
    pub chain_name: String,
    pub dex_count: usize,
    pub min_price: f64,
    pub max_price: f64,
    pub avg_price: f64,
    pub spread_pct: f64,
}

// ─── Simple Arbitrage (Buy on X, sell on Y) ──────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbitrageOpportunity {
    pub id: String,
    pub token_symbol: String,
    pub token_address: String,
    pub buy_chain_id: u64,
    pub buy_chain_name: String,
    pub buy_dex: String,
    pub buy_price_usd: f64,
    pub sell_chain_id: u64,
    pub sell_chain_name: String,
    pub sell_dex: String,
    pub sell_price_usd: f64,
    pub spread_pct: f64,
    pub estimated_profit_usd: f64,
    pub liquidity_usd: f64,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RadarScanResponse {
    pub token: String,
    pub opportunities: Vec<ArbitrageOpportunity>,
    pub scan_time_ms: u64,
}

// ─── Advanced Arbitrage Strategies ────────────────────

/// Every arbitrage strategy the engine can execute
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ArbitrageStrategy {
    /// Flash loan: borrow, buy cheap, sell expensive, repay
    FlashLoanArbitrage,
    /// Direct swap: user provides capital, find best route
    DirectSwap,
    /// Mint arbitrage: mint synthetic (e.g. DAI) cheaper than market
    MintArbitrage,
    /// Triangular: A → B → C → A on same chain
    TriangularArbitrage,
    /// Cross-chain: bridge asset across chains
    CrossChainArbitrage,
    /// JIT liquidity / MEV
    JitLiquidity,
}

impl ArbitrageStrategy {
    pub fn as_str(&self) -> &'static str {
        match self {
            ArbitrageStrategy::FlashLoanArbitrage => "Flash Loan Arbitrage",
            ArbitrageStrategy::DirectSwap => "Direct Swap",
            ArbitrageStrategy::MintArbitrage => "Mint Arbitrage",
            ArbitrageStrategy::TriangularArbitrage => "Triangular Arbitrage",
            ArbitrageStrategy::CrossChainArbitrage => "Cross-Chain Arbitrage",
            ArbitrageStrategy::JitLiquidity => "JIT Liquidity / MEV",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            ArbitrageStrategy::FlashLoanArbitrage => "Borrow via flash loan, buy on cheap DEX, sell on expensive DEX, repay loan + keep profit. 0 capital required.",
            ArbitrageStrategy::DirectSwap => "User provides the capital. We find the single best route via ParaSwap split routing across all DEXes.",
            ArbitrageStrategy::MintArbitrage => "Mint DAI on Spark/Maker when the mint cost is below market price. Sell DAI on DEX for profit.",
            ArbitrageStrategy::TriangularArbitrage => "Cycle through 3 tokens (e.g. DAI → ETH → USDC → DAI) on a single chain where price inconsistencies create profit.",
            ArbitrageStrategy::CrossChainArbitrage => "Bridge tokens across chains using Stargate/Across. Buy on chain A, bridge, sell on chain B where price is higher.",
            ArbitrageStrategy::JitLiquidity => "Provide just-in-time liquidity to an upcoming large swap. Capture the spread as the LP fee.",
        }
    }

    /// Whether this strategy needs 0 upfront capital
    pub fn zero_capital(&self) -> bool {
        match self {
            ArbitrageStrategy::FlashLoanArbitrage => true,
            ArbitrageStrategy::MintArbitrage => true,
            _ => false,
        }
    }

    pub fn needs_user_capital(&self) -> bool {
        !self.zero_capital()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriangularLeg {
    pub from_token: String,
    pub to_token: String,
    pub dex: String,
    pub rate: f64,
    pub expected_output: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriangularOpportunity {
    pub id: String,
    pub chain_id: u64,
    pub chain_name: String,
    pub legs: Vec<TriangularLeg>,
    pub start_token: String,
    pub end_token: String,
    pub net_profit_pct: f64,
    pub estimated_profit_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossChainOpportunity {
    pub id: String,
    pub token: String,
    pub buy_chain_id: u64,
    pub buy_chain_name: String,
    pub buy_price: f64,
    pub sell_chain_id: u64,
    pub sell_chain_name: String,
    pub sell_price: f64,
    pub bridge_fee_usd: f64,
    pub net_profit_pct: f64,
    pub estimated_profit_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MintOpportunity {
    pub id: String,
    pub token: String,
    pub mint_platform: String,
    pub mint_cost_usd: f64,
    pub market_price_usd: f64,
    pub spread_pct: f64,
    pub estimated_profit_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JitLiquidityOpportunity {
    pub id: String,
    pub token: String,
    pub pool: String,
    pub dex: String,
    pub chain_id: u64,
    pub chain_name: String,
    pub expected_fee_usd: f64,
    pub capital_required_usd: f64,
}

/// Simple execute request (backward compat)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteArbitrageRequest {
    pub opportunity_id: String,
    pub flash_loan_source: FlashLoanSource,
    pub gas_strategy: GasStrategy,
    pub user_address: String,
}

/// Unified view of all opportunities across all strategies
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AllOpportunitiesResponse {
    pub token: String,
    pub simple_arbitrages: Vec<ArbitrageOpportunity>,
    pub triangular_arbitrages: Vec<TriangularOpportunity>,
    pub cross_chain_arbitrages: Vec<CrossChainOpportunity>,
    pub mint_opportunities: Vec<MintOpportunity>,
    pub jit_opportunities: Vec<JitLiquidityOpportunity>,
    pub scan_time_ms: u64,
}

// ─── Execution ────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FlashLoanSource {
    AaveV3,
    RadiantV2,
    Spark,
}

impl FlashLoanSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            FlashLoanSource::AaveV3 => "Aave V3",
            FlashLoanSource::RadiantV2 => "Radiant V2",
            FlashLoanSource::Spark => "Spark Protocol",
        }
    }
    pub fn fee_pct(&self, token: &str) -> f64 {
        match self {
            FlashLoanSource::Spark => if token == "DAI" { 0.0 } else { 0.05 },
            FlashLoanSource::AaveV3 => 0.05,
            FlashLoanSource::RadiantV2 => 0.03,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GasStrategy {
    Flashbots,
    Pimlico,
    ZeroDev,
}

impl GasStrategy {
    pub fn as_str(&self) -> &'static str {
        match self {
            GasStrategy::Flashbots => "Flashbots / MEV-Share",
            GasStrategy::Pimlico => "Pimlico (ERC-4337)",
            GasStrategy::ZeroDev => "ZeroDev (ERC-4337)",
        }
    }
}

/// Execution mode
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExecutionMode {
    FlashLoan,
    DirectSwap,
    Mint,
}

/// Master execution request for any strategy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdvancedExecuteRequest {
    pub strategy: String, // "flash_loan" | "direct_swap" | "mint" | "triangular" | "cross_chain" | "jit"
    pub execution_mode: ExecutionMode,
    pub flash_loan_source: Option<FlashLoanSource>,
    pub gas_strategy: GasStrategy,
    pub user_address: String,
    pub token: Option<String>,
    pub token_address: Option<String>,
    pub amount: Option<String>,
    pub chain_id: Option<u64>,
    pub opportunity_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteResult {
    pub status: String,
    pub message: String,
    pub strategy: String,
    pub execution_mode: String,
    pub tx_hash: Option<String>,
    pub estimated_profit_usd: Option<f64>,
    pub gas_cost_usd: Option<f64>,
}

// ─── Velora (formerly ParaSwap) ──────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VeloraRoute {
    pub src_token: String,
    pub src_decimals: u8,
    pub dest_token: String,
    pub dest_decimals: u8,
    pub src_amount: String,
    pub dest_amount: String,
    pub percentage: f64,
    pub exchange: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VeloraPriceResponse {
    pub src_token: String,
    pub dest_token: String,
    pub src_amount: String,
    pub dest_amount: String,
    pub price_impact: f64,
    pub routes: Vec<VeloraRoute>,
    pub gas_cost_usd: f64,
    pub contract_address: String,
    pub token_transfer_proxy: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VeloraTxResponse {
    pub from: String,
    pub to: String,
    pub value: String,
    pub data: String,
    pub gas_price: String,
    pub gas: String,
    pub chain_id: u64,
}

// ─── Misc ─────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsMessage {
    pub msg_type: String,
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub chains_connected: Vec<String>,
    pub uptime_secs: u64,
}

// ─── Net Profit & Cost Breakdown ─────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostBreakdown {
    pub gas_estimated_usd: f64,
    pub flash_loan_fee_usd: f64,
    pub slippage_estimated_usd: f64,
    pub bridge_fee_usd: Option<f64>,
    pub velora_fee_usd: f64,
    pub total_cost_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetProfitBreakdown {
    pub gross_profit_usd: f64,
    pub costs: CostBreakdown,
    pub net_profit_usd: f64,
    pub net_profit_pct: f64,
    pub roi_pct: f64,
    pub is_profitable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ArbitrageType {
    Simple,       // Buy X on DEX A, sell on DEX B
    Triangular,   // A → B → C → A
    CrossChain,   // Bridge between chains
    Mint,         // Mint DAI cheaper than market
    JitLiquidity, // JIT liquidity / MEV
}

impl ArbitrageType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ArbitrageType::Simple => "Simple",
            ArbitrageType::Triangular => "Triangular",
            ArbitrageType::CrossChain => "Cross-Chain",
            ArbitrageType::Mint => "Mint",
            ArbitrageType::JitLiquidity => "JIT / MEV",
        }
    }
    pub fn description(&self) -> &'static str {
        match self {
            ArbitrageType::Simple => "Buy low on one DEX, sell high on another DEX",
            ArbitrageType::Triangular => "Cycle through 3 tokens exploiting price inconsistencies",
            ArbitrageType::CrossChain => "Buy on chain A, bridge to chain B, sell at higher price",
            ArbitrageType::Mint => "Mint synthetic asset below market price",
            ArbitrageType::JitLiquidity => "Provide just-in-time liquidity to capture LP fees",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlashLoanRecommendation {
    pub source: FlashLoanSource,
    pub fee_pct: f64,
    pub fee_usd: f64,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecommendedFlashLoan {
    pub primary: FlashLoanRecommendation,
    pub alternatives: Vec<FlashLoanRecommendation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpportunityDetail {
    pub id: String,
    pub token: String,
    pub token_address: String,
    pub arbitrage_type: ArbitrageType,
    pub chain_name: String,
    pub chain_id: u64,
    pub buy_dex: Option<String>,
    pub sell_dex: Option<String>,
    pub buy_price: f64,
    pub sell_price: f64,
    pub spread_pct: f64,
    pub profit_breakdown: NetProfitBreakdown,
    pub flash_loan_recommendation: Option<RecommendedFlashLoan>,
    pub execution_steps: Vec<String>,
    pub confidence_score: f64, // 0.0 - 1.0
    pub liquidity_usd: f64,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComprehensiveScanResponse {
    pub opportunities: Vec<OpportunityDetail>,
    pub total_opportunities: usize,
    pub profitable_count: usize,
    pub total_net_profit_usd: f64,
    pub total_gas_estimated_usd: f64,
    pub scan_time_ms: u64,
    pub tokens_scanned: Vec<String>,
    pub chains_scanned: Vec<String>,
    pub dexes_scanned: Vec<String>,
}

// ─── Bot Config ──────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotConfig {
    pub mode: BotMode,
    pub min_net_profit_usd: f64,
    pub max_gas_price_gwei: Option<f64>,
    pub enabled_strategies: Vec<ArbitrageType>,
    pub flash_loan_sources: Vec<FlashLoanSource>,
    pub gas_strategy: GasStrategy,
    pub max_slippage_pct: f64,
    pub auto_restart: bool,
    pub llm_advisor: bool,
    pub llm_config: Option<LLMConfig>,
    pub scan_interval_secs: u64,
    pub max_concurrent_tx: u32,
    pub chains_enabled: Vec<u64>,
    pub dexes_enabled: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BotMode {
    Manual,    // Bot finds opportunities, user reviews & approves
    Auto,      // Bot executes automatically
    SemiAuto,  // Bot auto-executes below threshold, asks above
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotStatus {
    pub running: bool,
    pub config: BotConfig,
    pub total_trades: u64,
    pub successful_trades: u64,
    pub failed_trades: u64,
    pub total_profit_usd: f64,
    pub uptime_secs: u64,
    pub current_opportunity: Option<OpportunityDetail>,
    pub last_execution: Option<u64>,
    pub logs: Vec<BotLogEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotLogEntry {
    pub timestamp: u64,
    pub level: String,
    pub message: String,
    pub opportunity_id: Option<String>,
    pub profit_usd: Option<f64>,
}

// ─── LLM Integration ─────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMConfig {
    pub provider: LLMProvider,
    pub api_key: String,
    pub model: String,
    pub temperature: f64,
    pub max_tokens: u32,
    pub system_prompt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LLMProvider {
    OpenAI,
    Anthropic,
    Groq,
    Ollama,
    DeepSeek,
    Custom,
}

impl LLMProvider {
    pub fn as_str(&self) -> &'static str {
        match self {
            LLMProvider::OpenAI => "OpenAI",
            LLMProvider::Anthropic => "Anthropic",
            LLMProvider::Groq => "Groq",
            LLMProvider::Ollama => "Ollama (Local)",
            LLMProvider::DeepSeek => "DeepSeek",
            LLMProvider::Custom => "Custom",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMAdviceRequest {
    pub opportunity: OpportunityDetail,
    pub market_context: String,
    pub user_question: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMAdviceResponse {
    pub advice: String,
    pub confidence: String,
    pub recommend_execute: bool,
    pub reasoning: Vec<String>,
    pub risk_factors: Vec<String>,
}

// ─── Liquidity Data ──────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiquidityDataPoint {
    pub chain_id: u64,
    pub chain_name: String,
    pub dex_name: String,
    pub token: String,
    pub token_address: String,
    pub liquidity_usd: f64,
    pub price_usd: f64,
    pub volume_24h_usd: f64,
    pub pool_address: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiquidityMapResponse {
    pub total_liquidity_usd: f64,
    pub by_chain: Vec<LiquidityChainSummary>,
    pub by_dex: Vec<LiquidityDexSummary>,
    pub data_points: Vec<LiquidityDataPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiquidityChainSummary {
    pub chain_id: u64,
    pub chain_name: String,
    pub total_liquidity_usd: f64,
    pub dex_count: usize,
    pub token_count: usize,
    pub percentage: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiquidityDexSummary {
    pub dex_name: String,
    pub chain_id: u64,
    pub total_liquidity_usd: f64,
    pub percentage: f64,
}

// ─── Bubble Chart ────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BubbleData {
    pub token: String,
    pub symbol: String,
    pub price_usd: f64,
    pub liquidity_usd: f64,
    pub market_cap_usd: f64,
    pub chain_name: String,
    pub chain_id: u64,
    pub has_opportunity: bool,
    pub opportunity_types: Vec<ArbitrageType>,
    pub best_spread_pct: f64,
    pub volume_24h_usd: f64,
    pub price_change_24h_pct: f64,
    pub dexes_available: Vec<String>,
    pub bubble_size: f64, // calculated: sqrt(liquidity) normalized
}

// ─── Response wrapper for all frontend data ──────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardData {
    pub bubbles: Vec<BubbleData>,
    pub liquidity_map: LiquidityMapResponse,
    pub opportunities: Vec<OpportunityDetail>,
    pub bot_status: Option<BotStatus>,
    pub scan_timestamp: u64,
    pub total_profit_24h_usd: f64,
    pub total_opportunities_found: usize,
}
