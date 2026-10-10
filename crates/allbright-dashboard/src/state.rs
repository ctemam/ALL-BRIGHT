//! Dashboard event index — the dashboard's own bounded, persistent store
//! of economically-meaningful events (AGENTS §38.12 lineage: every row in
//! this index carries its source event type and timestamp; nothing here
//! is fabricated).
//!
//! The ingest journal is BSONL and multi-GB — too large to scan per
//! request. The dashboard therefore maintains its own compact JSONL index
//! (`journal/dashboard-index.jsonl`), appended live from the bus fan-in
//! and replayed on startup. Historical depth = "since index start";
//! surfaces label it honestly.

use allbright_core::events::*;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::io::Write;
use std::path::PathBuf;

pub const INDEX_FILE: &str = "journal/dashboard-index.jsonl";
/// Bounded memory — far above any realistic daily event rate for the
/// row types indexed (opps, execs, rejections, failures, heals, scans).
const MAX_ROWS: usize = 200_000;

/// The 12 Master Build strategy variants (AGENTS §7) — read, never
/// invented. `kind` maps live OppKind signals onto the registry.
pub const STRATEGY_VARIANTS: &[(&str, &str)] = &[
    ("P01", "Cross-DEX Arbitrage"),
    ("P02", "Multi-Hop Cyclic Arbitrage"),
    ("P03", "CEX-DEX Divergence"),
    ("P04", "Stablecoin / Correlated-Asset Arbitrage"),
    ("P05", "State-Change Backrun"),
    ("P06", "Long-Tail Liquidity Arbitrage"),
    ("P07", "Concentrated-Liquidity / Fee-Tier Optimization"),
    ("P08", "Protocol Liquidation Arbitrage"),
    ("P09", "Short-Horizon Divergence Prediction"),
    ("P10", "Statistical Convergence"),
    ("P11", "Liquidity-Imbalance / Flow Prediction"),
    ("P12", "Adaptive Strategy Portfolio Allocation"),
];

/// Map a live signal onto the strategy registry. AtomicDexArb routes
/// touching stable pools are P04; ≥3 legs = P02; else P01.
pub fn variant_of(kind: &OppKind, route: &[SwapLeg]) -> &'static str {
    match kind {
        OppKind::CexDexDivergence | OppKind::AggQuoteDivergence => "P03",
        OppKind::Backrun => "P05",
        OppKind::AtomicDexArb => {
            if route.iter().any(|l| l.dex == DexKind::Stable) {
                "P04"
            } else if route.len() >= 3 {
                "P02"
            } else {
                "P01"
            }
        }
    }
}

/// Resolve strategy attribution: the wire `strategy` field is authoritative;
/// fall back to the route-shape heuristic only for records predating it.
fn strat_of<'a>(explicit: &'a str, kind: &OppKind, route: &[SwapLeg]) -> String {
    if explicit.is_empty() {
        variant_of(kind, route).to_string()
    } else {
        explicit.to_string()
    }
}

/// One normalized index row — compact, self-describing, journaled as
/// JSONL. `t` is the row type tag.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "t")]
pub enum IdxRow {
    /// OpportunitySignal — detected candidate (expected economics).
    #[serde(rename = "opp")]
    Opp {
        ts: u64,
        chain: String,
        variant: String,
        expected_usd: f64,
        legs: usize,
        input_token: String,
        input_amount: f64,
    },
    /// ExecutionResult — submission/simulation outcome (realized economics).
    #[serde(rename = "exec")]
    Exec {
        ts: u64,
        chain: String,
        /// Strategy attribution — empty on pre-strategy records.
        #[serde(default)]
        variant: String,
        status: String,
        tx_hash: Option<String>,
        realized_usd: f64,
        gas_used: u64,
        detail: String,
    },
    /// ExecutionRequest — the decision boundary (execution authorized).
    #[serde(rename = "xreq")]
    ExecReq {
        ts: u64,
        chain: String,
        variant: String,
        expected_usd: f64,
        executor: String,
        dry_run: bool,
    },
    /// RejectedCandidate — funnel rejection evidence.
    #[serde(rename = "rej")]
    Rej {
        ts: u64,
        chain: String,
        variant: String,
        reason: String,
        gross_usd: f64,
        threshold_usd: f64,
    },
    /// Classified failure (self-diagnosis feed).
    #[serde(rename = "fail")]
    Fail {
        ts: u64,
        component: String,
        class: String,
        chain: Option<String>,
        stage: String,
        detail: String,
    },
    /// Sentinel heal record.
    #[serde(rename = "heal")]
    Heal {
        ts: u64,
        target: String,
        action: String,
        success: bool,
        reason: String,
    },
    /// RPC scout sweep summary.
    #[serde(rename = "scan")]
    Scan {
        ts: u64,
        qualified: usize,
        total: usize,
        providers: usize,
    },
}

impl IdxRow {
    pub fn ts(&self) -> u64 {
        match self {
            IdxRow::Opp { ts, .. }
            | IdxRow::Exec { ts, .. }
            | IdxRow::ExecReq { ts, .. }
            | IdxRow::Rej { ts, .. }
            | IdxRow::Fail { ts, .. }
            | IdxRow::Heal { ts, .. }
            | IdxRow::Scan { ts, .. } => *ts,
        }
    }
}

/// Convert a live bus event into an index row (None = not indexed —
/// telemetry/tick floods stay out of the index by design).
pub fn row_of(ev: &Event) -> Option<IdxRow> {
    Some(match ev {
        Event::Opportunity(o) => IdxRow::Opp {
            ts: o.ts_ms,
            chain: o.chain.name().to_string(),
            variant: strat_of(&o.strategy, &o.kind, &o.route),
            expected_usd: o.expected_profit_usd,
            legs: o.route.len(),
            input_token: o.input_token.clone(),
            input_amount: o.input_amount,
        },
        Event::ExecResult(r) => IdxRow::Exec {
            ts: r.ts_ms,
            chain: r.chain.name().to_string(),
            variant: r.strategy.clone(),
            status: format!("{:?}", r.status),
            tx_hash: r.tx_hash.clone(),
            realized_usd: r.realized_profit_usd,
            gas_used: r.gas_used,
            detail: r.detail.clone(),
        },
        Event::ExecRequest(q) => IdxRow::ExecReq {
            ts: q.ts_ms,
            chain: q.signal.chain.name().to_string(),
            variant: strat_of(&q.signal.strategy, &q.signal.kind, &q.signal.route),
            expected_usd: q.signal.expected_profit_usd,
            executor: q.executor_address.clone(),
            dry_run: q.dry_run,
        },
        Event::Rejected(r) => IdxRow::Rej {
            ts: r.ts_ms,
            chain: r.chain.name().to_string(),
            variant: strat_of(&r.strategy, &r.kind, &r.route),
            reason: format!("{:?}", r.reason),
            gross_usd: r.gross_usd,
            threshold_usd: r.threshold_usd,
        },
        Event::Failure(f) => IdxRow::Fail {
            ts: f.ts_ms,
            component: format!("{:?}", f.component),
            class: format!("{:?}", f.class),
            chain: f.chain.map(|c| c.name().to_string()),
            stage: f.stage.clone(),
            detail: f.detail.clone(),
        },
        Event::Heal(h) => IdxRow::Heal {
            ts: h.ts_ms,
            target: h.target.clone(),
            action: h.action.clone(),
            success: h.success,
            reason: h.reason.clone(),
        },
        Event::RpcScan(s) => IdxRow::Scan {
            ts: s.updated_ms,
            qualified: s.qualified,
            total: s.total,
            providers: s.providers,
        },
        _ => return None,
    })
}

/// Bounded in-memory index + append-only JSONL spill.
pub struct EventIndex {
    rows: VecDeque<IdxRow>,
    file: Option<std::fs::File>,
    /// Index epoch — honest "data since" marker for the UI.
    pub index_since_ms: u64,
}

impl Default for EventIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl EventIndex {
    pub fn new() -> Self {
        let path =
            PathBuf::from(std::env::var("DASHBOARD_INDEX").unwrap_or_else(|_| INDEX_FILE.into()));
        let mut rows = VecDeque::with_capacity(1024);
        let mut index_since = now_ms();
        if let Ok(content) = std::fs::read_to_string(&path) {
            for line in content.lines() {
                if let Ok(r) = serde_json::from_str::<IdxRow>(line) {
                    index_since = index_since.min(r.ts());
                    if rows.len() >= MAX_ROWS {
                        rows.pop_front();
                    }
                    rows.push_back(r);
                }
            }
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .ok();
        EventIndex {
            rows,
            file,
            index_since_ms: index_since,
        }
    }

    /// Index a live event; persists when relevant.
    pub fn push(&mut self, ev: &Event) {
        let Some(row) = row_of(ev) else { return };
        if let Some(f) = self.file.as_mut() {
            let _ = serde_json::to_writer(&mut *f, &row);
            let _ = f.write_all(b"\n");
            let _ = f.flush();
        }
        if self.rows.len() >= MAX_ROWS {
            self.rows.pop_front();
        }
        self.index_since_ms = self.index_since_ms.min(row.ts());
        self.rows.push_back(row);
    }

    pub fn rows(&self) -> &VecDeque<IdxRow> {
        &self.rows
    }

    /// Filtered view used by the API layer.
    pub fn in_range(&self, from: u64, to: u64) -> impl Iterator<Item = &IdxRow> {
        self.rows
            .iter()
            .filter(move |r| r.ts() >= from && (to == 0 || r.ts() <= to))
    }
}
