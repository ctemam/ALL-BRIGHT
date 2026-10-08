//! Source-Origin Diagnostic Registry — the self-diagnosis layer.
//!
//! Every major component carries an explicit architectural lineage
//! (`LINEAGE_REGISTRY`), and every operational failure is emitted as a
//! `FailureEvent` classified by *class* (what failed), *component* (where
//! it failed), and *origin* (which source the root cause belongs to).
//!
//! Lineage ≠ blame: `lineage` names the architecture a module descends
//! from; `origin` names where the defect actually lives. A Rusty-Sando-
//! derived module can fail from an Allbright integration defect — the
//! Velodrome sAMM phantom-profit incident was exactly that.
//!
//! Diagnostic order per AGENTS.md: WHAT failed → WHERE (component) →
//! WHICH lineage owns it → diagnosed ORIGIN → root cause → fix → verify.

use serde::{Deserialize, Serialize};

use crate::events::Chain;

/// Architectural source a component descends from, or the diagnosed
/// origin of a defect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceLineage {
    /// Event-driven collector/processor architecture (Paradigm Artemis).
    Artemis,
    /// EVM state simulation / searcher engineering (Rusty-Sando + revm).
    RustySando,
    /// Private execution, relay/bundle concepts (Flashbots / MEV-Share).
    Flashbots,
    /// Allbright-original integration, strategy and specialization layer.
    Allbright,
    /// External infrastructure: RPC, network, builders, dependencies,
    /// chain conditions, deployment environment.
    Environment,
}

/// Every diagnosable subsystem. `module` strings match the concrete
/// file/module so failure events can be traced to code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Component {
    Ingest,
    CexStream,
    EvmStream,
    VeloraStream,
    Recorder,
    Simulation,
    Solver,
    Executor,
    BuilderRelay,
    Calldata,
    AccountAbstraction,
    Research,
    Dashboard,
    Core,
    Contract,
    Bus,
    /// Self-diagnosis/remediation authority (AGENTS §34).
    Sentinel,
    /// RPC discovery + qualification worker (AGENTS §36/§37).
    RpcScout,
}

/// Machine-readable registry: component → owning lineage → module.
pub static LINEAGE_REGISTRY: &[LineageEntry] = &[
    LineageEntry { component: Component::Ingest,        lineage: SourceLineage::Artemis,     module: "allbright-ingest/main.rs",          note: "collector orchestration" },
    LineageEntry { component: Component::CexStream,     lineage: SourceLineage::Allbright,   module: "allbright-ingest/cex_stream.rs",    note: "Binance market-data integration" },
    LineageEntry { component: Component::EvmStream,     lineage: SourceLineage::Artemis,     module: "allbright-ingest/evm_stream.rs",    note: "per-chain collector; Multicall3 reads" },
    LineageEntry { component: Component::VeloraStream,  lineage: SourceLineage::Allbright,   module: "allbright-ingest/velora_stream.rs", note: "aggregator quote feed" },
    LineageEntry { component: Component::Recorder,      lineage: SourceLineage::Artemis,     module: "allbright-ingest/recorder.rs",      note: "append-only journal" },
    LineageEntry { component: Component::Simulation,    lineage: SourceLineage::RustySando,  module: "allbright-simulation/main.rs",      note: "pool cache + route evaluation" },
    LineageEntry { component: Component::Solver,        lineage: SourceLineage::Allbright,   module: "allbright-solver",                  note: "graph + funnel + profit model" },
    LineageEntry { component: Component::Executor,      lineage: SourceLineage::Flashbots,   module: "allbright-executor/main.rs",        note: "execution orchestration" },
    LineageEntry { component: Component::BuilderRelay,  lineage: SourceLineage::Flashbots,   module: "allbright-executor/relay.rs",       note: "per-chain relay adapters" },
    LineageEntry { component: Component::Calldata,      lineage: SourceLineage::Allbright,   module: "allbright-executor/calldata.rs",    note: "SwapLeg encoding" },
    LineageEntry { component: Component::AccountAbstraction, lineage: SourceLineage::Allbright, module: "allbright-executor/aa.rs",       note: "ERC-4337/Pimlico ops path" },
    LineageEntry { component: Component::Research,      lineage: SourceLineage::Allbright,   module: "allbright-research",                note: "incremental journal scanner" },
    LineageEntry { component: Component::Dashboard,     lineage: SourceLineage::Artemis,     module: "allbright-dashboard",               note: "telemetry surfaces" },
    LineageEntry { component: Component::Core,          lineage: SourceLineage::Allbright,   module: "allbright-core",                    note: "types/AMM math/config" },
    LineageEntry { component: Component::Contract,      lineage: SourceLineage::Allbright,   module: "contracts/AllbrightExecutor.sol",   note: "on-chain profit authority" },
    LineageEntry { component: Component::Bus,           lineage: SourceLineage::Artemis,     module: "allbright-core/bus.rs",             note: "BSON-over-WS envelope" },
    LineageEntry { component: Component::Sentinel,      lineage: SourceLineage::Allbright,   module: "allbright-sentinel",                note: "self-diagnosis & bounded remediation" },
    LineageEntry { component: Component::RpcScout,      lineage: SourceLineage::Allbright,   module: "allbright-rpc-scout",               note: "RPC discovery/qualification + visibility contract" },
];

#[derive(Debug, Clone, Copy)]
pub struct LineageEntry {
    pub component: Component,
    pub lineage: SourceLineage,
    pub module: &'static str,
    pub note: &'static str,
}

/// Look up the owning lineage of a component.
pub fn lineage_of(component: Component) -> SourceLineage {
    LINEAGE_REGISTRY
        .iter()
        .find(|e| e.component == component)
        .map(|e| e.lineage)
        .unwrap_or(SourceLineage::Allbright)
}

/// Failure taxonomy (SKILL.md §20). Never emit a bare "failed" — always
/// classify. The class narrows diagnosis; it does not assign blame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureClass {
    StaleState,
    InsufficientLiquidity,
    LowNetProfit,
    SimulationRevert,
    TokenRisk,
    RpcTimeout,
    StreamDrop,
    /// RPC blindness classes (AGENTS §32.5) — never collapsed into
    /// "no opportunity".
    RpcBlindness,
    RpcQuotaExhaustion,
    RpcRateLimit,
    RpcStaleState,
    RpcMethodUnavailable,
    RpcProviderFailure,
    RpcNetworkPartition,
    RpcDataInconsistency,
    RpcLatencyExpired,
    /// Visibility diagnostic codes (AGENTS §37.12) — graded blindness,
    /// never collapsed into "no opportunity".
    RpcOpportunityBlind,
    RpcPoolVisibilityLoss,
    RpcPairVisibilityLoss,
    RpcDexVisibilityLoss,
    RpcEventVisibilityLoss,
    RpcStateVisibilityLoss,
    RpcHistoricalVisibilityLoss,
    RpcSimulationDataBlind,
    RpcProviderDivergence,
    RpcStaleMarketState,
    RpcQuotaVisibilityLoss,
    RpcMethodVisibilityFailure,
    RpcChainVisibilityFailure,
    MetadataMismatch,
    BuilderRejected,
    NotIncluded,
    OnchainRevert,
    ProfitBelowThreshold,
    ConfigurationError,
    PortConflict,
    OrphanedProcess,
    MathArtifact,
    Unknown,
}

impl FailureClass {
    /// First-pass origin hypothesis. The emitter may override — a class
    /// describes the symptom, `origin` records the diagnosed source.
    pub fn default_origin(self) -> SourceLineage {
        match self {
            Self::RpcTimeout | Self::StreamDrop | Self::BuilderRejected | Self::NotIncluded => {
                SourceLineage::Environment
            }
            Self::RpcBlindness | Self::RpcQuotaExhaustion | Self::RpcRateLimit
            | Self::RpcStaleState | Self::RpcMethodUnavailable
            | Self::RpcProviderFailure | Self::RpcNetworkPartition
            | Self::RpcDataInconsistency | Self::RpcLatencyExpired
            | Self::RpcOpportunityBlind | Self::RpcPoolVisibilityLoss
            | Self::RpcPairVisibilityLoss | Self::RpcDexVisibilityLoss
            | Self::RpcEventVisibilityLoss | Self::RpcStateVisibilityLoss
            | Self::RpcHistoricalVisibilityLoss | Self::RpcSimulationDataBlind
            | Self::RpcProviderDivergence | Self::RpcStaleMarketState
            | Self::RpcQuotaVisibilityLoss | Self::RpcMethodVisibilityFailure
            | Self::RpcChainVisibilityFailure => {
                SourceLineage::Environment
            }
            Self::PortConflict | Self::OrphanedProcess | Self::ConfigurationError => {
                SourceLineage::Environment
            }
            _ => SourceLineage::Allbright,
        }
    }
}

/// A classified failure with lineage attribution — the unit of the
/// Source-Origin Diagnostic Pipeline. Journal records it, the dashboard
/// surfaces it, research aggregates it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureEvent {
    /// Where the failure surfaced.
    pub component: Component,
    /// Architecture the failing module descends from.
    pub lineage: SourceLineage,
    /// What kind of failure.
    pub class: FailureClass,
    /// Diagnosed root-source — may differ from `lineage` (integration
    /// defect inside a derived module is `Allbright`, not the lineage).
    pub origin: SourceLineage,
    /// Pipeline stage, e.g. "evm.newHeads", "pool.verify", "relay.submit".
    pub stage: String,
    pub chain: Option<Chain>,
    pub detail: String,
    pub ts_ms: u64,
}

impl FailureEvent {
    /// Construct with default origin = class hypothesis. Override
    /// `origin` after construction when diagnosis is known.
    pub fn new(component: Component, class: FailureClass, stage: &str, detail: String) -> Self {
        FailureEvent {
            component,
            lineage: lineage_of(component),
            class,
            origin: class.default_origin(),
            stage: stage.to_string(),
            chain: None,
            detail,
            ts_ms: crate::events::now_ms(),
        }
    }

    pub fn on_chain(mut self, chain: Chain) -> Self {
        self.chain = Some(chain);
        self
    }

    pub fn with_origin(mut self, origin: SourceLineage) -> Self {
        self.origin = origin;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_covers_all_components() {
        // Every Component variant must have a lineage entry.
        for c in [
            Component::Ingest, Component::CexStream, Component::EvmStream,
            Component::VeloraStream, Component::Recorder, Component::Simulation,
            Component::Solver, Component::Executor, Component::BuilderRelay,
            Component::Calldata, Component::AccountAbstraction, Component::Research,
            Component::Dashboard, Component::Core, Component::Contract, Component::Bus,
            Component::Sentinel, Component::RpcScout,
        ] {
            assert!(LINEAGE_REGISTRY.iter().any(|e| e.component == c), "{c:?} unmapped");
        }
    }

    #[test]
    fn rpc_timeout_is_environment() {
        let f = FailureEvent::new(Component::EvmStream, FailureClass::RpcTimeout, "evm.heads", "pong missed".into());
        assert_eq!(f.lineage, SourceLineage::Artemis); // module ownership
        assert_eq!(f.origin, SourceLineage::Environment); // diagnosed source
    }
}
