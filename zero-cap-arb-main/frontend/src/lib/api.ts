const API_BASE = process.env.NEXT_PUBLIC_API_URL ?? 'http://localhost:3001';

export interface ChainConfig {
  id: number;
  name: string;
  rpc_url: string;
  rpc_urls: string[];
  native_currency: string;
  explorer_url: string;
}

export interface DexConfig {
  name: string;
  address: string;
  chain_id: number;
}

// ─── Token Price ──────────────────────────────────────

export interface TokenPrice {
  token: string;
  token_address: string;
  chain_id: number;
  chain_name: string;
  dex_name: string;
  price_usd: number;
  liquidity_usd: number;
  timestamp: number;
}

export interface ChainSummary {
  chain_id: number;
  chain_name: string;
  dex_count: number;
  min_price: number;
  max_price: number;
  avg_price: number;
  spread_pct: number;
}

export interface AllPricesResponse {
  token: string;
  token_address: string;
  prices: TokenPrice[];
  chain_summary: ChainSummary[];
  scan_time_ms: number;
}

// ─── Simple Arbitrage ─────────────────────────────────

export interface ArbitrageOpportunity {
  id: string;
  token_symbol: string;
  token_address: string;
  buy_chain_id: number;
  buy_chain_name: string;
  buy_dex: string;
  buy_price_usd: number;
  sell_chain_id: number;
  sell_chain_name: string;
  sell_dex: string;
  sell_price_usd: number;
  spread_pct: number;
  estimated_profit_usd: number;
  liquidity_usd: number;
  timestamp: number;
}

export interface RadarScanResponse {
  token: string;
  opportunities: ArbitrageOpportunity[];
  scan_time_ms: number;
}

// ─── Advanced Arbitrage Types ─────────────────────────

export interface TriangularLeg {
  from_token: string;
  to_token: string;
  dex: string;
  rate: number;
  expected_output: string;
}

export interface TriangularOpportunity {
  id: string;
  chain_id: number;
  chain_name: string;
  legs: TriangularLeg[];
  start_token: string;
  end_token: string;
  net_profit_pct: number;
  estimated_profit_usd: number;
}

export interface CrossChainOpportunity {
  id: string;
  token: string;
  buy_chain_id: number;
  buy_chain_name: string;
  buy_price: number;
  sell_chain_id: number;
  sell_chain_name: string;
  sell_price: number;
  bridge_fee_usd: number;
  net_profit_pct: number;
  estimated_profit_usd: number;
}

export interface MintOpportunity {
  id: string;
  token: string;
  mint_platform: string;
  mint_cost_usd: number;
  market_price_usd: number;
  spread_pct: number;
  estimated_profit_usd: number;
}

export interface JitLiquidityOpportunity {
  id: string;
  token: string;
  pool: string;
  dex: string;
  chain_id: number;
  chain_name: string;
  expected_fee_usd: number;
  capital_required_usd: number;
}

export interface AllOpportunitiesResponse {
  token: string;
  simple_arbitrages: ArbitrageOpportunity[];
  triangular_arbitrages: TriangularOpportunity[];
  cross_chain_arbitrages: CrossChainOpportunity[];
  mint_opportunities: MintOpportunity[];
  jit_opportunities: JitLiquidityOpportunity[];
  scan_time_ms: number;
}

// ─── NEW: Comprehensive Scan ──────────────────────────

export interface CostBreakdown {
  gas_estimated_usd: number;
  flash_loan_fee_usd: number;
  slippage_estimated_usd: number;
  bridge_fee_usd: number | null;
  velora_fee_usd: number;
  total_cost_usd: number;
}

export interface NetProfitBreakdown {
  gross_profit_usd: number;
  costs: CostBreakdown;
  net_profit_usd: number;
  net_profit_pct: number;
  roi_pct: number;
  is_profitable: boolean;
}

export type ArbitrageType = 'Simple' | 'Triangular' | 'CrossChain' | 'Mint' | 'JitLiquidity';

export interface FlashLoanRecommendation {
  source: string;
  fee_pct: number;
  fee_usd: number;
  reason: string;
}

export interface RecommendedFlashLoan {
  primary: FlashLoanRecommendation;
  alternatives: FlashLoanRecommendation[];
}

export interface OpportunityDetail {
  id: string;
  token: string;
  token_address: string;
  arbitrage_type: ArbitrageType;
  chain_name: string;
  chain_id: number;
  buy_dex: string | null;
  sell_dex: string | null;
  buy_price: number;
  sell_price: number;
  spread_pct: number;
  profit_breakdown: NetProfitBreakdown;
  flash_loan_recommendation: RecommendedFlashLoan | null;
  execution_steps: string[];
  confidence_score: number;
  liquidity_usd: number;
  timestamp: number;
}

export interface ComprehensiveScanResponse {
  opportunities: OpportunityDetail[];
  total_opportunities: number;
  profitable_count: number;
  total_net_profit_usd: number;
  total_gas_estimated_usd: number;
  scan_time_ms: number;
  tokens_scanned: string[];
  chains_scanned: string[];
  dexes_scanned: string[];
}

// ─── NEW: Bot Control ─────────────────────────────────

export type BotMode = 'Manual' | 'Auto' | 'SemiAuto';
export type FlashLoanSource = 'AaveV3' | 'RadiantV2' | 'Spark';
export type GasStrategy = 'Flashbots' | 'Pimlico' | 'ZeroDev';

export interface LLMConfig {
  provider: string;
  api_key: string;
  model: string;
  temperature: number;
  max_tokens: number;
  system_prompt: string;
}

export interface BotConfig {
  mode: BotMode;
  min_net_profit_usd: number;
  max_gas_price_gwei: number | null;
  enabled_strategies: ArbitrageType[];
  flash_loan_sources: FlashLoanSource[];
  gas_strategy: GasStrategy;
  max_slippage_pct: number;
  auto_restart: boolean;
  llm_advisor: boolean;
  llm_config: LLMConfig | null;
  scan_interval_secs: number;
  max_concurrent_tx: number;
  chains_enabled: number[];
  dexes_enabled: string[];
}

export interface BotLogEntry {
  timestamp: number;
  level: string;
  message: string;
  opportunity_id: string | null;
  profit_usd: number | null;
}

export interface BotStatus {
  running: boolean;
  config: BotConfig;
  total_trades: number;
  successful_trades: number;
  failed_trades: number;
  total_profit_usd: number;
  uptime_secs: number;
  current_opportunity: OpportunityDetail | null;
  last_execution: number | null;
  logs: BotLogEntry[];
}

// ─── NEW: LLM Advice ──────────────────────────────────

export interface LLMAdviceRequest {
  opportunity: OpportunityDetail;
  market_context: string;
  user_question: string;
}

export interface LLMAdviceResponse {
  advice: string;
  confidence: string;
  recommend_execute: boolean;
  reasoning: string[];
  risk_factors: string[];
}

// ─── NEW: Liquidity Map ──────────────────────────────

export interface LiquidityDataPoint {
  chain_id: number;
  chain_name: string;
  dex_name: string;
  token: string;
  token_address: string;
  liquidity_usd: number;
  price_usd: number;
  volume_24h_usd: number;
  pool_address: string;
}

export interface LiquidityChainSummary {
  chain_id: number;
  chain_name: string;
  total_liquidity_usd: number;
  dex_count: number;
  token_count: number;
  percentage: number;
}

export interface LiquidityDexSummary {
  dex_name: string;
  chain_id: number;
  total_liquidity_usd: number;
  percentage: number;
}

export interface LiquidityMapResponse {
  total_liquidity_usd: number;
  by_chain: LiquidityChainSummary[];
  by_dex: LiquidityDexSummary[];
  data_points: LiquidityDataPoint[];
}

// ─── NEW: Bubble Chart ────────────────────────────────

export interface BubbleData {
  token: string;
  symbol: string;
  price_usd: number;
  liquidity_usd: number;
  market_cap_usd: number;
  chain_name: string;
  chain_id: number;
  has_opportunity: boolean;
  opportunity_types: ArbitrageType[];
  best_spread_pct: number;
  volume_24h_usd: number;
  price_change_24h_pct: number;
  dexes_available: string[];
  bubble_size: number;
}

// ─── NEW: Dashboard ───────────────────────────────────

export interface DashboardData {
  bubbles: BubbleData[];
  liquidity_map: LiquidityMapResponse;
  opportunities: OpportunityDetail[];
  bot_status: BotStatus | null;
  scan_timestamp: number;
  total_profit_24h_usd: number;
  total_opportunities_found: number;
}

// ─── Velora ──────────────────────────────────────────

export interface VeloraRoute {
  src_token: string;
  dest_token: string;
  src_amount: string;
  dest_amount: string;
  percentage: number;
  exchange: string;
}

export interface VeloraPriceResponse {
  src_token: string;
  dest_token: string;
  src_amount: string;
  dest_amount: string;
  price_impact: number;
  routes: VeloraRoute[];
  gas_cost_usd: number;
  contract_address: string;
  token_transfer_proxy: string;
  version: string;
}

export interface VeloraTxResponse {
  from: string;
  to: string;
  value: string;
  data: string;
  gas_price: string;
  gas: string;
  chain_id: number;
}

export interface ExecuteResult {
  status: string;
  message: string;
  strategy: string;
  execution_mode: string;
  tx_hash?: string;
  estimated_profit_usd?: number;
  gas_cost_usd?: number;
}

export interface HealthResponse {
  status: string;
  chains_connected: string[];
  uptime_secs: number;
}

// ─── Paper Trading / Backtesting ────────────────────

export interface PaperTrade {
  id: string;
  opportunity: OpportunityDetail;
  entry_time: number;
  exit_time: number | null;
  result: 'Win' | 'Loss' | 'BreakEven' | null;
  profit_usd: number;
  roi_pct: number;
  gas_used_usd: number;
  flash_loan_fee_usd: number;
  status: 'Simulated' | 'Executed' | 'Failed';
  notes: string;
}

export interface BacktestResult {
  trades: PaperTrade[];
  start_time: number;
  end_time: number;
  initial_balance: number;
  final_balance: number;
  total_return_pct: number;
  total_trades: number;
  wins: number;
  losses: number;
  win_rate_pct: number;
  largest_win_usd: number;
  largest_loss_usd: number;
  max_drawdown_pct: number;
  profit_factor: number;
  sharpe_ratio: number;
  avg_profit_per_trade: number;
  best_strategy: string;
  monthly_returns: { month: string; return_pct: number; trades: number; profit_usd: number }[];
  equity_curve: { timestamp: number; balance: number }[];
}

// ─── Portfolio Manager ─────────────────────────────

export interface StrategyAllocation {
  strategy: ArbitrageType;
  weight_pct: number;
  max_concurrent: number;
  min_profit_usd: number;
  max_daily_trades: number;
  daily_trades: number;
}

export interface PortfolioConfig {
  strategies: StrategyAllocation[];
  total_balance_usd: number;
  risk_per_trade_pct: number;
  max_daily_loss_usd: number;
  daily_loss: number;
  max_open_positions: number;
  open_positions: number;
}

export interface PortfolioStatus {
  config: PortfolioConfig;
  total_pnl_usd: number;
  total_pnl_pct: number;
  daily_pnl_usd: number;
  open_trades: number;
  today_trades: number;
  win_rate_pct: number;
  strategy_breakdown: {
    strategy: string;
    total_trades: number;
    wins: number;
    losses: number;
    pnl_usd: number;
    win_rate_pct: number;
    allocation_pct: number;
  }[];
}

// ─── MEV Guard ─────────────────────────────────────

export type MevRiskLevel = 'Safe' | 'LowRisk' | 'MediumRisk' | 'HighRisk' | 'Critical';

export interface MevGuardConfig {
  enabled: boolean;
  block_sandwich: boolean;
  block_frontrun: boolean;
  block_backrun: boolean;
  max_risk_level: MevRiskLevel;
  use_flashbots: boolean;
  use_private_mempool: boolean;
  delay_seconds: number;
  honeypot_check: boolean;
}

export interface MevDetectionResult {
  risk_level: MevRiskLevel;
  score: number;
  sandwich_probability: number;
  frontrun_probability: number;
  backrun_probability: number;
  unchecked_enabled: boolean;
  detected_bots: string[];
  pending_tx_count: number;
  recommended_action: string;
}

// ─── Alerts (Telegram/Discord) ─────────────────────

export type AlertChannel = 'Telegram' | 'Discord' | 'Webhook';
export type AlertEventType = 'TradeExecuted' | 'OpportunityFound' | 'ProfitTaken' | 'LossTriggered' | 'ErrorOccurred' | 'MevDetected' | 'BotStarted' | 'BotStopped' | 'DailySummary';

export interface AlertConfig {
  channel: AlertChannel;
  webhook_url: string;
  events: AlertEventType[];
  min_profit_usd: number;
  enabled: boolean;
  notify_on_error: boolean;
  daily_summary: boolean;
}

export interface AlertMessage {
  channel: AlertChannel;
  event: AlertEventType;
  title: string;
  body: string;
  timestamp: number;
  delivered: boolean;
}

// ─── Rules Engine ─────────────────────────────────

export type RuleOperator = 'GreaterThan' | 'LessThan' | 'Equals' | 'Between' | 'Contains';
export type RuleField = 'SpreadPct' | 'NetProfitUsd' | 'ConfidenceScore' | 'LiquidityUsd' | 'GasCostUsd' | 'ChainId' | 'ArbitrageType' | 'TokenSymbol';
export type RuleAction = 'Execute' | 'Skip' | 'LogOnly' | 'NotifyMe' | 'AskApproval';

export interface ExecutionRule {
  id: string;
  name: string;
  enabled: boolean;
  field: RuleField;
  operator: RuleOperator;
  value: string;
  action: RuleAction;
  priority: number;
}

export interface RuleEvaluationResult {
  rule_id: string;
  rule_name: string;
  matched: boolean;
  action: RuleAction;
  reason: string;
}

// ─── Profit Splitter ──────────────────────────────

export interface SplitterConfig {
  wallets: SplitterWallet[];
  enabled: boolean;
  min_split_profit_usd: number;
}

export interface SplitterWallet {
  address: string;
  label: string;
  share_pct: number;
  enabled: boolean;
}

export interface SplitEntry {
  address: string;
  label: string;
  amount_usd: number;
  percentage: number;
}

export interface ProfitSplitResult {
  total_profit_usd: number;
  splits: SplitEntry[];
  executed: boolean;
}

// ─── Gas Bidder ──────────────────────────────────

export type GasBidStrategy = 'Fixed' | 'Adaptive' | 'Priority' | 'MEVProtected';

export interface GasBidConfig {
  strategy: GasBidStrategy;
  max_gas_price_gwei: number;
  min_gas_price_gwei: number;
  priority_pct: number;
  adaptive_enabled: boolean;
}

export interface GasRecommendation {
  suggested_gwei: number;
  strategy: string;
  estimated_cost_usd: number;
  confidence: string;
  reasoning: string[];
}

// ─── API Client ───────────────────────────────────────

async function fetchApi<T>(endpoint: string, options?: RequestInit): Promise<T> {
  const res = await fetch(`${API_BASE}${endpoint}`, {
    headers: { 'Content-Type': 'application/json' },
    ...options,
  });
  if (!res.ok) {
    const err = await res.text();
    throw new Error(`API error ${res.status}: ${err}`);
  }
  return res.json();
}

export const api = {
  health: () => fetchApi<HealthResponse>('/api/health'),

  scanToken: (token: string, tokenAddress?: string) =>
    fetchApi<RadarScanResponse>('/api/scan', {
      method: 'POST',
      body: JSON.stringify({ token, token_address: tokenAddress }),
    }),

  getAllPrices: (token: string, tokenAddress?: string) =>
    fetchApi<AllPricesResponse>('/api/all-prices', {
      method: 'POST',
      body: JSON.stringify({ token, token_address: tokenAddress }),
    }),

  getAllOpportunities: (token: string, tokenAddress?: string) =>
    fetchApi<AllOpportunitiesResponse>('/api/all-opportunities', {
      method: 'POST',
      body: JSON.stringify({ token, token_address: tokenAddress }),
    }),

  // ─── NEW: Comprehensive Scan ────────────────────────

  comprehensiveScan: (tokens: { symbol: string; address: string }[]) =>
    fetchApi<ComprehensiveScanResponse>('/api/scan/comprehensive', {
      method: 'POST',
      body: JSON.stringify({ tokens }),
    }),

  // ─── NEW: Bot Control ───────────────────────────────

  getBotConfig: () => fetchApi<BotConfig>('/api/bot/config'),
  updateBotConfig: (config: BotConfig) =>
    fetchApi<BotConfig>('/api/bot/config', {
      method: 'POST',
      body: JSON.stringify(config),
    }),
  startBot: () => fetchApi<BotStatus>('/api/bot/start', { method: 'POST' }),
  stopBot: () => fetchApi<{ status: string }>('/api/bot/stop', { method: 'POST' }),
  getBotStatus: () => fetchApi<BotStatus | { running: false; message: string }>('/api/bot/status'),
  getBotLogs: () => fetchApi<BotLogEntry[]>('/api/bot/logs'),

  // ─── NEW: LLM ──────────────────────────────────────

  getLlmConfig: () => fetchApi<LLMConfig | { configured: false }>('/api/llm/config'),
  updateLlmConfig: (config: LLMConfig) =>
    fetchApi<{ configured: boolean; provider: string }>('/api/llm/config', {
      method: 'POST',
      body: JSON.stringify(config),
    }),
  getLlmAdvice: (req: LLMAdviceRequest) =>
    fetchApi<LLMAdviceResponse>('/api/llm/advise', {
      method: 'POST',
      body: JSON.stringify(req),
    }),

  // ─── NEW: Liquidity & Bubbles ──────────────────────

  getLiquidityData: () =>
    fetchApi<LiquidityMapResponse>('/api/liquidity', { method: 'POST' }),
  getBubbleData: () => fetchApi<BubbleData[]>('/api/bubbles'),
  getDashboard: () => fetchApi<DashboardData>('/api/dashboard'),

  // ─── Velora ─────────────────────────────────────────

  getVeloraPrice: (params: {
    chain_id: number; src_token: string; dest_token: string;
    src_decimals: number; dest_decimals: number;
    amount: string; side: string;
  }) =>
    fetchApi<VeloraPriceResponse>('/api/velora/price', {
      method: 'POST', body: JSON.stringify(params),
    }),

  getVeloraSwap: (params: {
    chain_id: number; src_token: string; dest_token: string;
    src_decimals: number; dest_decimals: number;
    amount: string; side: string;
    user_address?: string; slippage?: number;
  }) =>
    fetchApi<Record<string, unknown>>('/api/velora/swap', {
      method: 'POST', body: JSON.stringify(params),
    }),

  buildVeloraTx: (params: {
    chain_id: number; src_token: string; dest_token: string;
    src_decimals: number; dest_decimals: number;
    src_amount: string; dest_amount: string;
    slippage: number; user_address: string;
    price_route?: Record<string, unknown>;
  }) =>
    fetchApi<VeloraTxResponse>('/api/velora/build-tx', {
      method: 'POST', body: JSON.stringify(params),
    }),

  submitDeltaOrder: (params: {
    chain_id: number; src_token: string; dest_token: string;
    amount: string; user_address: string;
  }) =>
    fetchApi<Record<string, unknown>>('/api/velora/delta', {
      method: 'POST', body: JSON.stringify(params),
    }),

  execute: (params: {
    strategy: string; execution_mode: string;
    flash_loan_source?: string; gas_strategy: string;
    user_address: string; token?: string; token_address?: string;
    amount?: string; chain_id?: number; opportunity_id?: string;
  }) =>
    fetchApi<ExecuteResult>('/api/execute/advanced', {
      method: 'POST', body: JSON.stringify(params),
    }),

  getChains: () => fetchApi<ChainConfig[]>('/api/chains'),
  getDexes: (chainId: number) => fetchApi<DexConfig[]>(`/api/dexes/${chainId}`),

  // ─── Paper Trading / Backtesting ──────────────────────

  paperTradingStart: () =>
    fetchApi<{ status: string; balance_usd: number }>('/api/paper/start', { method: 'POST' }),
  paperTradingStop: () =>
    fetchApi<BacktestResult>('/api/paper/stop', { method: 'POST' }),
  paperTradingStatus: () =>
    fetchApi<BacktestResult>('/api/paper/status'),
  paperSimulateTrade: (opportunity: OpportunityDetail) =>
    fetchApi<{ trade: PaperTrade; balance_usd: number; total_trades: number; win_rate_pct: number }>(
      '/api/paper/simulate', { method: 'POST', body: JSON.stringify(opportunity) }
    ),
  paperRunBacktest: () =>
    fetchApi<BacktestResult>('/api/paper/backtest', { method: 'POST' }),
  paperReset: () =>
    fetchApi<{ status: string; balance_usd: number }>('/api/paper/reset', { method: 'POST' }),

  // ─── Portfolio Manager ─────────────────────────────

  getPortfolioConfig: () => fetchApi<PortfolioConfig>('/api/portfolio/config'),
  updatePortfolioConfig: (config: PortfolioConfig) =>
    fetchApi<PortfolioConfig>('/api/portfolio/config', { method: 'POST', body: JSON.stringify(config) }),
  getPortfolioStatus: () =>
    fetchApi<PortfolioStatus>('/api/portfolio/status'),

  // ─── MEV Guard ────────────────────────────────────

  getMevConfig: () => fetchApi<MevGuardConfig>('/api/mev/config'),
  updateMevConfig: (config: MevGuardConfig) =>
    fetchApi<MevGuardConfig>('/api/mev/config', { method: 'POST', body: JSON.stringify(config) }),
  mevAnalyze: (chainId: number, txData?: string) =>
    fetchApi<MevDetectionResult>('/api/mev/analyze', {
      method: 'POST', body: JSON.stringify({ chain_id: chainId, tx_data: txData }),
    }),

  // ─── Alerts (Telegram/Discord) ─────────────────────

  getAlertsConfig: () => fetchApi<AlertConfig[]>('/api/alerts/config'),
  updateAlertsConfig: (configs: AlertConfig[]) =>
    fetchApi<AlertConfig[]>('/api/alerts/config', { method: 'POST', body: JSON.stringify(configs) }),
  getAlertsHistory: () => fetchApi<AlertMessage[]>('/api/alerts/history'),
  testAlert: () => fetchApi<{ status: string }>('/api/alerts/test', { method: 'POST' }),

  // ─── Rules Engine ─────────────────────────────────

  getRules: () => fetchApi<ExecutionRule[]>('/api/rules'),
  updateRules: (rules: ExecutionRule[]) =>
    fetchApi<ExecutionRule[]>('/api/rules', { method: 'POST', body: JSON.stringify(rules) }),
  evaluateRules: (opportunity: OpportunityDetail) =>
    fetchApi<{ should_execute: boolean; results: RuleEvaluationResult[] }>('/api/rules/evaluate', {
      method: 'POST', body: JSON.stringify(opportunity),
    }),

  // ─── Profit Splitter ──────────────────────────────

  getSplitterConfig: () => fetchApi<SplitterConfig>('/api/splitter/config'),
  updateSplitterConfig: (config: SplitterConfig) =>
    fetchApi<SplitterConfig>('/api/splitter/config', { method: 'POST', body: JSON.stringify(config) }),
  calculateSplit: (totalProfitUsd: number) =>
    fetchApi<ProfitSplitResult>('/api/splitter/calculate', {
      method: 'POST', body: JSON.stringify({ total_profit_usd: totalProfitUsd }),
    }),

  // ─── Gas Bidder ───────────────────────────────────

  getGasConfig: () => fetchApi<GasBidConfig>('/api/gas/config'),
  updateGasConfig: (config: GasBidConfig) =>
    fetchApi<GasBidConfig>('/api/gas/config', { method: 'POST', body: JSON.stringify(config) }),
  recommendGas: (chainId: number, profitUsd: number, spreadPct: number) =>
    fetchApi<GasRecommendation>('/api/gas/recommend', {
      method: 'POST', body: JSON.stringify({ chain_id: chainId, profit_usd: profitUsd, spread_pct: spreadPct }),
    }),
};
