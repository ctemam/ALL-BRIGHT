//! Typed event model — the single wire contract between all Allbright
//! processes. Serialized as BSON over localhost WebSocket.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Chain {
    Eth,
    Arb,
    Op,
    Poly,
    Bsc,
    Avax,
    Base,
    Linea,
    Scroll,
    Blast,
}

impl Chain {
    pub const ALL: [Chain; 10] = [
        Chain::Eth, Chain::Arb, Chain::Op, Chain::Poly, Chain::Bsc,
        Chain::Avax, Chain::Base, Chain::Linea, Chain::Scroll, Chain::Blast,
    ];

    pub fn chain_id(self) -> u64 {
        match self {
            Chain::Eth => 1,
            Chain::Bsc => 56,
            Chain::Poly => 137,
            Chain::Arb => 42161,
            Chain::Op => 10,
            Chain::Avax => 43114,
            Chain::Base => 8453,
            Chain::Linea => 59144,
            Chain::Scroll => 534352,
            Chain::Blast => 81457,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Chain::Eth => "eth",
            Chain::Arb => "arb",
            Chain::Op => "op",
            Chain::Poly => "poly",
            Chain::Bsc => "bsc",
            Chain::Avax => "avax",
            Chain::Base => "base",
            Chain::Linea => "linea",
            Chain::Scroll => "scroll",
            Chain::Blast => "blast",
        }
    }

    /// Binance USDT symbol for the chain's gas asset — single canonical
    /// mapping (MATIC was delisted; Polygon uses POL post-migration).
    pub fn native_usdt_symbol(self) -> &'static str {
        match self {
            Chain::Eth | Chain::Arb | Chain::Op | Chain::Base
            | Chain::Linea | Chain::Scroll | Chain::Blast => "ETHUSDT",
            Chain::Bsc => "BNBUSDT",
            Chain::Poly => "POLUSDT",
            Chain::Avax => "AVAXUSDT",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DexKind {
    V2,
    V3,
    Stable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Token {
    pub symbol: String,
    pub address: String,
    pub decimals: u8,
}

/// Point-in-time state of a liquidity pool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolState {
    pub chain: Chain,
    pub dex: DexKind,
    pub venue: String, // e.g. "pancakeswap_v2", "biswap"
    pub address: String,
    pub token0: Token,
    pub token1: Token,
    /// V2: (reserve0, reserve1). V3: reserve fields unused.
    pub reserve0: f64,
    pub reserve1: f64,
    /// V3: sqrt_price_x96 and in-range liquidity (f64 signal-level).
    pub sqrt_price_x96: Option<f64>,
    pub liquidity: Option<f64>,
    /// Fee in bps (V2 typically 25-30, V3 from pool tier).
    pub fee_bps: u32,
    pub block: u64,
    pub ts_ms: u64,
}

impl PoolState {
    /// Spot price of token0 in terms of token1 (human units).
    /// V2/Stable reserves are already decimal-normalized; V3's √P is a
    /// raw-unit ratio and needs the decimal adjustment.
    pub fn spot_price(&self) -> f64 {
        match self.dex {
            DexKind::V2 => {
                if self.reserve0 > 0.0 {
                    self.reserve1 / self.reserve0
                } else {
                    0.0
                }
            }
            DexKind::V3 => self
                .sqrt_price_x96
                .map(|s| {
                    // P_raw is raw-token1 per raw-token0; per human token0:
                    // ×10^(d0−d1).
                    let dec_adj =
                        10f64.powi(self.token0.decimals as i32 - self.token1.decimals as i32);
                    (s / 2f64.powi(96)).powi(2) * dec_adj
                })
                .unwrap_or(0.0),
            DexKind::Stable => {
                // Marginal price −dy/dx of k = x³y + xy³.
                let (x, y) = (self.reserve0, self.reserve1);
                if x > 0.0 && y > 0.0 {
                    let num = 3.0 * x * x * y + y.powi(3);
                    let den = x.powi(3) + 3.0 * x * y * y;
                    num / den
                } else {
                    0.0
                }
            }
        }
    }
}

/// Binance bookTicker signal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CexTick {
    pub venue: String, // "binance"
    pub symbol: String, // e.g. "BTCUSDT"
    pub bid: f64,
    pub ask: f64,
    pub ts_ms: u64,
}

/// Velora (ex-ParaSwap) aggregated quote.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DexQuote {
    pub chain: Chain,
    pub src_token: String,
    pub dst_token: String,
    pub src_amount: f64,
    pub dst_amount: f64,
    pub ts_ms: u64,
}

/// A single swap leg inside a route.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwapLeg {
    pub venue: String,
    pub pool_address: String,
    pub token_in: String,
    pub token_out: String,
    pub dex: DexKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpportunitySignal {
    pub chain: Chain,
    pub kind: OppKind,
    /// Strategy ID (P01..P05) — attribution.
    #[serde(default)]
    pub strategy: String,
    pub route: Vec<SwapLeg>,
    pub input_amount: f64,
    pub input_token: String,
    pub expected_profit_usd: f64,
    pub min_profit_usd: f64,
    /// EV-class submission (GAP-37): positive-net candidate below the
    /// standard floor, attempted only where revert cost ~0 (sponsored
    /// path) and the simgate dry-run is the authority. Telemetry must
    /// distinguish these from standard-floor submissions.
    #[serde(default)]
    pub ev_mode: bool,
    /// ABI-encoded AllbrightExecutor call, ready for submission.
    pub calldata: Option<String>,
    pub ts_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OppKind {
    AtomicDexArb,
    CexDexDivergence,
    AggQuoteDivergence,
    Backrun,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionRequest {
    pub signal: OpportunitySignal,
    pub executor_address: String,
    pub dry_run: bool,
    pub ts_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    pub chain: Chain,
    /// Strategy ID (P01..P05) — echoed from the triggering signal.
    #[serde(default)]
    pub strategy: String,
    pub status: ExecStatus,
    pub tx_hash: Option<String>,
    pub realized_profit_usd: f64,
    pub gas_used: u64,
    pub detail: String,
    pub ts_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecStatus {
    Simulated,
    Submitted,
    Included,
    Reverted,
    Failed,
}

/// Observability frame — the opportunity funnel.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TelemetryFrame {
    pub seen: u64,
    pub simulated: u64,
    pub profitable: u64,
    pub submitted: u64,
    pub included: u64,
    pub settled: u64,
    /// Per-stage p50 latency in ms (decode/sim/solve/submit).
    pub lat_decode_ms: f64,
    pub lat_sim_ms: f64,
    pub lat_solve_ms: f64,
    pub lat_submit_ms: f64,
    pub uptime_secs: u64,
    pub ts_ms: u64,
}

/// Research statistics snapshot served by allbright-research.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResearchStats {
    pub journal_events: u64,
    pub opportunities: u64,
    pub executions: u64,
    pub avg_expected_profit_usd: f64,
    pub per_chain_opps: Vec<(String, u64)>,
    /// Classified failures observed, by `component:class:origin` key.
    #[serde(default)]
    pub failures: Vec<(String, u64)>,
    /// IOO: independently observed spread events (opportunity universe).
    #[serde(default)]
    pub observed_universe: u64,
    /// Strategy rejections, by reason (e.g. "below_strategy_threshold").
    #[serde(default)]
    pub rejections: Vec<(String, u64)>,
    /// Challenger verdicts where counterfactual net > 0.
    #[serde(default)]
    pub challenged: u64,
    /// Sum of counterfactual net USD across challenged rejections.
    #[serde(default)]
    pub rescued_usd: f64,
    /// P06: pools discovered via factory events (verified / total).
    #[serde(default)]
    pub pools_discovered: u64,
    #[serde(default)]
    pub pools_discovered_verified: u64,
    pub ts_ms: u64,
}

/// Solver → simulation: evaluate a candidate route against current pool
/// state at a given block.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimRequest {
    pub req_id: u64,
    pub chain: Chain,
    pub route: Vec<SwapLeg>,
    pub input_amount: f64,
    pub input_token: String,
    /// Strategy ID (P01..P05) — attribution; absent on pre-strategy records.
    #[serde(default)]
    pub strategy: String,
    pub block: u64,
    pub ts_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimResult {
    pub req_id: u64,
    pub chain: Chain,
    pub ok: bool,
    pub output_amount: f64,
    pub output_token: String,
    pub gas_est: u64,
    pub elapsed_us: u64,
    pub detail: String,
    pub ts_ms: u64,
}

/// Opportunity lifecycle — every opportunity carries how far it got.
/// This is what makes "zero opportunity" distinguishable from "zero
/// QUALIFIED opportunity" (AGENTS.md §30 + challenger constitution).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OppState {
    Observed,
    Candidate,
    Qualified,
    Simulated,
    EconomicallyPositive,
    RiskApproved,
    Executed,
    Confirmed,
    Profitable,
    Unprofitable,
}

/// Why a candidate stopped — the reason code stays on the record so
/// counterfactual analysis can revisit it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectReason {
    BelowStrategyThreshold,
    InsufficientLiquidity,
    SimulationFailed,
    NegativeEdge,
    NoUsdPricing,
    ExecutionUnavailable,
    StaleState,
    RiskRejected,
    /// Route touches a pool convicted as bait (fake reserves → on-chain
    /// revert). Emitted when a conviction lands between sim dispatch
    /// and result — the in-flight candidate is dropped, never executed.
    BaitPool,
    /// Simulated output exceeds what the route's pools physically
    /// hold — a reserve-bound violation proves the input state was
    /// corrupt/incoherent (the nine-figure phantom class), so the
    /// candidate is rejected as a math artifact, never executed.
    MathArtifact,
}

/// Independent Opportunity Observatory record — a price discrepancy the
/// MARKET showed, measured without the strategy's profit threshold.
/// The observatory never decides whether to trade.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservedOpportunity {
    pub chain: Chain,
    /// Pair key: sorted token addresses "tokenA/tokenB".
    pub pair: String,
    /// Venue with the lower price and venue with the higher price.
    pub buy_venue: String,
    pub sell_venue: String,
    /// Gross price discrepancy in percent (pre-cost, pre-threshold).
    pub spread_pct: f64,
    /// Signal-level depth estimate on the shallow side (USD).
    pub est_depth_usd: f64,
    pub ts_ms: u64,
}

/// A strategy candidate that was stopped — auditable evidence for the
/// challenger. `state` is the furthest lifecycle stage reached.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RejectedCandidate {
    pub chain: Chain,
    pub kind: OppKind,
    /// Strategy ID (P01..P05) — per-strategy rejection attribution.
    #[serde(default)]
    pub strategy: String,
    pub req_id: u64,
    pub route: Vec<SwapLeg>,
    pub input_amount: f64,
    pub input_token: String,
    pub state: OppState,
    pub reason: RejectReason,
    /// Observed gross edge in USD (0 if unknown, e.g. sim revert).
    pub gross_usd: f64,
    pub threshold_usd: f64,
    pub gas_est: u64,
    pub ts_ms: u64,
}

/// Challenger verdict: counterfactual economics of a rejected candidate.
/// `challenged = true` means the decision is refuted by evidence — the
/// threshold needs recalibration review (not blind lowering).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChallengeVerdict {
    pub chain: Chain,
    pub req_id: u64,
    pub reason: RejectReason,
    /// Net of the challenger's full cost model (gas, flash premium,
    /// slippage buffer) at challenge time.
    pub counterfactual_net_usd: f64,
    pub challenged: bool,
    pub detail: String,
    pub ts_ms: u64,
}

/// P06: a newly deployed pool discovered via a factory event.
/// OBSERVED tier only (Master Build Guide §30) — never auto-admitted to
/// the execution universe; promotion requires the risk lifecycle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredPool {
    pub chain: Chain,
    pub factory: String,
    pub pool: String,
    pub token0: String,
    pub token1: String,
    /// Pool family inferred from the event signature (not trusted —
    /// confirmed by on-chain probes during verification).
    pub dex: DexKind,
    /// True once token0/token1/decimals were re-verified on-chain.
    pub verified: bool,
    pub block: u64,
    pub ts_ms: u64,
}

/// Sentinel remediation record — every autonomous heal action is
/// journaled so the audit trail can replay it (AGENTS §34).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealAction {
    /// Service being remediated, e.g. "allbright-solver".
    pub target: String,
    /// Remediation performed, e.g. "pm2.restart".
    pub action: String,
    /// Triggering condition, e.g. "port_dead:3103".
    pub reason: String,
    /// Post-action verification result.
    pub success: bool,
    /// Restart budget consumed for this target in the window.
    pub restarts_in_window: u32,
    pub ts_ms: u64,
}

/// The observatory's periodic measure of the opportunity universe.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UniverseStats {
    /// Pairs tracked across ≥2 venues on any chain.
    pub observed_pairs: u64,
    /// Distinct pair observations above the noise floor this window.
    pub spreads_above_floor: u64,
    pub max_spread_pct: f64,
    pub ts_ms: u64,
}

/// U1 pre-confirmation trigger: a pending (public-mempool) transaction
/// observed touching a watched pool — emitted by the ingest pending
/// watcher BEFORE the tx lands. Match is `to == pool` OR a watched pool
/// address embedded in the calldata's 32-byte words (router/aggregator
/// paths). Chains with private/sequenced mempools simply emit nothing.
/// Evidence only — the tx may never land; the predicted shift is used
/// to rescan early, never to fabricate state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingSwap {
    pub chain: Chain,
    /// Watched pool the pending tx touches.
    pub pool_address: String,
    /// Pending tx hash — evidence, not finality.
    pub tx_hash: String,
    pub ts_ms: u64,
}

/// Compact freshness heartbeat: pools re-verified UNCHANGED at a pin
/// block during one shard scan. Replaces identical-state re-emission —
/// consumers update "verified-at" block/ts stamps so staleness and
/// coherence checks measure read-time truth, not last-change time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolHeartbeat {
    pub chain: Chain,
    /// Pin block the unchanged state was verified at.
    pub block: u64,
    pub ts_ms: u64,
    /// Pool addresses confirmed unchanged this scan.
    pub pools: Vec<String>,
}

/// The single envelope all processes speak.
/// Lifecycle/operational marker — service boots, journal rollovers,
/// config-level facts. Journaled for restart attribution and audit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemEvent {
    pub component: String,
    pub action: String,
    pub detail: String,
    pub ts_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "t", content = "d")]
pub enum Event {
    CexTick(CexTick),
    DexQuote(DexQuote),
    PoolUpdate(PoolState),
    SimRequest(SimRequest),
    SimResult(SimResult),
    Opportunity(OpportunitySignal),
    ExecRequest(ExecutionRequest),
    ExecResult(ExecutionResult),
    Telemetry(TelemetryFrame),
    Stats(ResearchStats),
    /// Classified failure with source-lineage attribution (§diagnostics).
    Failure(crate::lineage::FailureEvent),
    /// IOO: market-observed discrepancy, threshold-independent.
    Observed(ObservedOpportunity),
    /// Strategy rejection with reason — challenger's input.
    Rejected(RejectedCandidate),
    /// Challenger counterfactual verdict.
    Challenge(ChallengeVerdict),
    /// IOO periodic universe measure.
    Universe(UniverseStats),
    /// P06: factory-event pool discovery (observation tier only).
    DiscoveredPool(DiscoveredPool),
    /// Sentinel self-heal action record (AGENTS §34).
    Heal(HealAction),
    /// RPC qualification sweep summary — journaled evidence of the
    /// discovery/qualification universe (AGENTS §36/§37).
    RpcScan(crate::rpc_registry::RegistrySummary),
    /// Lifecycle/operational marker (boot, journal rotate, …).
    System(SystemEvent),
    /// Verified-unchanged pool set for one scan (post-dedup freshness).
    PoolHeartbeat(PoolHeartbeat),
    /// U1: mempool tx predicted to move a watched pool (pre-confirmation).
    PendingSwap(PendingSwap),
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
