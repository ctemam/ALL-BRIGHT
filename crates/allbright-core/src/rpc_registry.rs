//! RPC Registry — the machine-readable endpoint intelligence layer
//! (AGENTS §36/§37).
//!
//! Single durable store of every discovered candidate endpoint: provenance,
//! capability matrix (§37.7), visibility class V0–V6 (§37.3), qualification
//! state, latency samples, freshness, safe capacity (90% rule, §36.7) and
//! the Opportunity Visibility Score (§37.10).
//!
//! Ownership: `allbright-rpc-scout` is the sole writer (atomic tmp→rename);
//! sentinel and ingest are readers. The registry is *evidence* — a URL
//! appearing here means nothing without a qualification state earned by
//! live probes from this deployment environment.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use crate::events::Chain;

pub const REGISTRY_FILE: &str = "rpc_registry.json";

/// Visibility ladder (AGENTS §37.3). Only V5/V6 qualifies an endpoint for
/// the corresponding production opportunity/execution workload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum VisibilityClass {
    /// RPC unavailable.
    V0Dead = 0,
    /// Basic JSON-RPC connectivity works.
    V1Connected = 1,
    /// Chain identity and current block available.
    V2ChainVisible = 2,
    /// Contracts, balances, calls and current state accessible.
    V3StateVisible = 3,
    /// DEXs, pools, pairs, liquidity and market state accessible.
    V4MarketVisible = 4,
    /// AllBright can discover and evaluate arbitrage candidates.
    V5OpportunityVisible = 5,
    /// AllBright can obtain everything needed to construct, simulate
    /// and prepare execution.
    V6ExecutionVisible = 6,
}

/// Qualification lifecycle (AGENTS §36.5). `Qualified` is the only state
/// admitted to the production candidate pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum QualState {
    Discovered,
    Live,
    ChainVerified,
    MethodVerified,
    StateVerified,
    Qualified,
    Degraded,
    RateLimited,
    Stale,
    Broken,
    Quarantined,
    /// URL template requiring an API key — counted, never probeable.
    NeedsKey,
}

/// Access class (§36.9): private keyed endpoints carry per-account
/// quota — an order of magnitude more usable capacity than shared
/// public endpoints. The system prefers private capacity for hot-path
/// reads and counts the classes separately on the infra surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AccessClass {
    /// Shared unkeyed endpoint — public quota, heavy rate limits.
    Public,
    /// Keyed/per-account endpoint — private free-tier or paid quota.
    Private,
}

impl Default for AccessClass {
    fn default() -> Self {
        AccessClass::Public
    }
}

/// Classify a URL: credential material in the query or a known keyed
/// host means private; everything else is public. Heuristic only —
/// re-derived on every load so stale classifications never persist.
pub fn access_of(url: &str) -> AccessClass {
    let u = url.to_lowercase();
    for pat in [
        "dkey=", "apikey=", "api_key=", "?key=", "&key=", "portalid=",
        "token=", "projectid=", "project_id=", "api-key=",
    ] {
        if u.contains(pat) {
            return AccessClass::Private;
        }
    }
    for pat in [
        "g.alchemy.com", ".infura.io/", "quiknode.pro",
        ".blastapi.io/", "nodereal.io", ".chainstack.com",
        // Keyed Tenderly = `{slug}.gateway.tenderly.co/{key}` — the
        // keyless public gateway has no path segment, so require the
        // trailing slash or a bare keyless URL would be mislabeled
        // private and admitted to the private-only production pool.
        ".gateway.tenderly.co/",
    ] {
        if u.contains(pat) && !u.contains("public.") {
            return AccessClass::Private;
        }
    }
    AccessClass::Public
}

/// Tri-state per capability (§37.7): nothing is AVAILABLE merely because
/// a provider advertises it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CapState {
    Available,
    Partial,
    Unavailable,
    Unknown,
}

/// Per-endpoint capability matrix (§37.7).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcCapabilities {
    pub chain_id: CapState,
    pub latest_block: CapState,
    pub block_fresh: CapState,
    pub eth_call: CapState,
    pub get_balance: CapState,
    pub get_logs: CapState,
    pub pool_state: CapState,
    pub websocket: CapState,
    pub send_raw_tx: CapState,
    pub archive: CapState,
    pub trace: CapState,
}

impl Default for RpcCapabilities {
    fn default() -> Self {
        RpcCapabilities {
            chain_id: CapState::Unknown,
            latest_block: CapState::Unknown,
            block_fresh: CapState::Unknown,
            eth_call: CapState::Unknown,
            get_balance: CapState::Unknown,
            get_logs: CapState::Unknown,
            pool_state: CapState::Unknown,
            websocket: CapState::Unknown,
            send_raw_tx: CapState::Unknown,
            archive: CapState::Unknown,
            trace: CapState::Unknown,
        }
    }
}

/// One candidate/qualified endpoint — the unit of the registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcEndpointRecord {
    pub url: String,
    pub host: String,
    /// Owning infrastructure, derived from eTLD+1 — provider diversity is
    /// measured on this, not on URL count (§36.2).
    pub provider: String,
    /// Discovery provenance: "env", "chainlist", "pocket", "official", …
    pub source: String,
    pub chain: Chain,
    /// "https" | "wss" | "http"
    pub transport: String,
    pub discovered_ms: u64,
    pub last_probe_ms: u64,
    pub caps: RpcCapabilities,
    pub visibility: VisibilityClass,
    pub state: QualState,
    /// Bounded RTT sample window (latest N).
    pub rtt_ms: Vec<u64>,
    pub probes: u64,
    pub errors: u64,
    pub consecutive_errors: u32,
    pub last_error: Option<String>,
    /// Seconds behind wall-clock at last probe.
    pub head_lag_secs: Option<u64>,
    /// Opportunity Visibility Score, 0..100 (§37.10).
    pub ovs: f64,
    /// Verified sustained request rate; safe capacity = ×0.90 (§36.7).
    pub verified_rps: Option<f64>,
    /// Public vs private keyed endpoint (§36.9) — re-derived from the
    /// URL on load; never trusted from disk.
    #[serde(default)]
    pub access: AccessClass,
}

impl RpcEndpointRecord {
    pub fn new(url: &str, chain: Chain, source: &str) -> Self {
        let host = host_of(url).to_string();
        RpcEndpointRecord {
            url: url.to_string(),
            provider: provider_of(&host),
            host,
            source: source.to_string(),
            chain,
            transport: url.split("://").next().unwrap_or("https").to_string(),
            discovered_ms: crate::events::now_ms(),
            last_probe_ms: 0,
            caps: RpcCapabilities::default(),
            visibility: VisibilityClass::V0Dead,
            state: QualState::Discovered,
            rtt_ms: Vec::new(),
            probes: 0,
            errors: 0,
            consecutive_errors: 0,
            last_error: None,
            head_lag_secs: None,
            ovs: 0.0,
            verified_rps: None,
            access: access_of(url),
        }
    }

    pub fn push_rtt(&mut self, rtt: u64) {
        self.rtt_ms.push(rtt);
        if self.rtt_ms.len() > 32 {
            self.rtt_ms.remove(0);
        }
    }

    pub fn median_rtt(&self) -> Option<u64> {
        if self.rtt_ms.is_empty() {
            return None;
        }
        let mut v = self.rtt_ms.clone();
        v.sort_unstable();
        Some(v[v.len() / 2])
    }

    /// 90% safe-capacity rule (§36.7): the remaining 10% is reserved for
    /// bursts, failover, self-diagnosis and emergency execution — never
    /// ordinary workload.
    pub fn safe_rps(&self) -> Option<f64> {
        self.verified_rps.map(|r| r * 0.90)
    }

    /// Opportunity Visibility Score (§37.10): weighted evidence sum.
    pub fn compute_ovs(&mut self) {
        let mut s = 0.0f64;
        if self.caps.chain_id == CapState::Available { s += 15.0; }
        if self.caps.latest_block == CapState::Available { s += 10.0; }
        if self.caps.block_fresh == CapState::Available { s += 15.0; }
        if self.caps.eth_call == CapState::Available { s += 15.0; }
        if self.caps.get_balance == CapState::Available { s += 5.0; }
        if self.caps.get_logs == CapState::Available { s += 10.0; }
        if self.caps.pool_state == CapState::Available { s += 10.0; }
        // Latency leg: <250ms full, <800ms half, else zero.
        match self.median_rtt() {
            Some(r) if r < 250 => s += 15.0,
            Some(r) if r < 800 => s += 8.0,
            Some(_) => s += 2.0,
            None => {}
        }
        // Reliability leg: success ratio over probes.
        if self.probes > 0 {
            s += 5.0 * (1.0 - self.errors as f64 / self.probes as f64).max(0.0);
        }
        self.ovs = s.min(100.0);
    }
}

/// Host part of a URL — never used for logging secrets; registry file is
/// local-only, not telemetry.
pub fn host_of(url: &str) -> &str {
    url.split("://").nth(1).unwrap_or(url).split('/').next().unwrap_or(url)
}

/// Provider = registrable domain (last two labels). Good enough for
/// diversity accounting; noted limitation on co-hosted ccTLDs.
pub fn provider_of(host: &str) -> String {
    let h = host.split(':').next().unwrap_or(host);
    let labels: Vec<&str> = h.split('.').collect();
    if labels.len() >= 2 {
        format!("{}.{}", labels[labels.len() - 2], labels[labels.len() - 1])
    } else {
        h.to_string()
    }
}

/// The registry document — single-writer (scout), atomic save.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcRegistry {
    pub schema: u32,
    pub updated_ms: u64,
    pub endpoints: Vec<RpcEndpointRecord>,
}

impl Default for RpcRegistry {
    fn default() -> Self {
        RpcRegistry { schema: 1, updated_ms: 0, endpoints: Vec::new() }
    }
}

impl RpcRegistry {
    pub fn load(path: &Path) -> Self {
        let mut reg: Self = std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        // Access class is derived truth, never stored truth — reclassify
        // every record so a hand-edited or stale file can't mislabel.
        for e in &mut reg.endpoints {
            e.access = access_of(&e.url);
        }
        reg
    }

    /// Atomic write — tmp + rename so readers never see a partial file.
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    /// Merge a probed record by (url, chain); preserves discovery history.
    pub fn upsert(&mut self, rec: RpcEndpointRecord) {
        if let Some(e) = self
            .endpoints
            .iter_mut()
            .find(|e| e.url == rec.url && e.chain == rec.chain)
        {
            e.last_probe_ms = rec.last_probe_ms;
            e.caps = rec.caps;
            e.visibility = rec.visibility;
            e.state = rec.state;
            e.rtt_ms = rec.rtt_ms;
            e.probes = e.probes.saturating_add(rec.probes.saturating_sub(1).max(0));
            e.errors = e.errors.saturating_add(rec.errors);
            e.consecutive_errors = rec.consecutive_errors;
            e.last_error = rec.last_error;
            e.head_lag_secs = rec.head_lag_secs;
            e.verified_rps = rec.verified_rps;
            e.access = access_of(&e.url);
            e.compute_ovs();
        } else {
            self.endpoints.push(rec);
        }
        self.updated_ms = crate::events::now_ms();
    }

    pub fn for_chain(&self, chain: Chain) -> impl Iterator<Item = &RpcEndpointRecord> {
        self.endpoints.iter().filter(move |e| e.chain == chain)
    }

    /// Production candidate pool for a chain: QUALIFIED **private**
    /// endpoints only (§36.9 — the platform is based on private keyed
    /// capacity; shared public endpoints are excluded from the
    /// production path even when they qualify). Ordered OVS descending.
    pub fn qualified_urls(&self, chain: Chain) -> Vec<String> {
        let mut v: Vec<&RpcEndpointRecord> = self
            .for_chain(chain)
            .filter(|e| e.state == QualState::Qualified && e.access == AccessClass::Private)
            .collect();
        v.sort_by(|a, b| b.ovs.partial_cmp(&a.ovs).unwrap_or(std::cmp::Ordering::Equal));
        v.into_iter().map(|e| e.url.clone()).collect()
    }

    /// Per-chain access accounting for the infra surface (§36.9):
    /// (total, private, private_qualified). Private quota is what the
    /// 80% budget rule actually trusts.
    pub fn chain_access_counts(&self, chain: Chain) -> (usize, usize, usize) {
        let mut t = (0usize, 0usize, 0usize);
        for e in self.for_chain(chain) {
            t.0 += 1;
            if e.access == AccessClass::Private {
                t.1 += 1;
                if e.state == QualState::Qualified {
                    t.2 += 1;
                }
            }
        }
        t
    }

    /// Fallback tier (§37.1): Degraded **private** endpoints that PROVED
    /// V4 market visibility (pool_state read succeeded) but lost
    /// Qualified on latency or an auxiliary cap. Public endpoints are
    /// excluded here as well (§36.9); economically-blind endpoints
    /// (pool_state Unavailable) are never admitted anywhere. OVS order.
    pub fn fallback_urls(&self, chain: Chain) -> Vec<String> {
        let mut v: Vec<&RpcEndpointRecord> = self
            .for_chain(chain)
            .filter(|e| {
                e.state == QualState::Degraded
                    && e.access == AccessClass::Private
                    && e.caps.pool_state == CapState::Available
            })
            .collect();
        v.sort_by(|a, b| b.ovs.partial_cmp(&a.ovs).unwrap_or(std::cmp::Ordering::Equal));
        v.into_iter().map(|e| e.url.clone()).collect()
    }

    /// WSS-qualified endpoints for a chain (GAP-34): transport is
    /// `wss` or `ws`, `eth_subscribe newHeads` was granted
    /// (`caps.websocket == Available`), and state is `Qualified`.
    /// Ordered OVS descending so the fastest measured endpoint is tried
    /// first. No access-class filter — WSS endpoints come from Chainlist
    /// and derived candidates, not just private keyed infrastructure.
    pub fn wss_qualified_urls(&self, chain: Chain) -> Vec<String> {
        let mut v: Vec<&RpcEndpointRecord> = self
            .for_chain(chain)
            .filter(|e| {
                (e.transport == "wss" || e.transport == "ws")
                    && e.state == QualState::Qualified
                    && e.caps.websocket == CapState::Available
            })
            .collect();
        v.sort_by(|a, b| b.ovs.partial_cmp(&a.ovs).unwrap_or(std::cmp::Ordering::Equal));
        v.into_iter().map(|e| e.url.clone()).collect()
    }

    /// Provider diversity for a chain — a pool of 200 URLs from one
    /// infrastructure owner is NOT independent capacity (§36.2).
    pub fn chain_diversity(&self, chain: Chain) -> (usize, usize) {
        let qualified: Vec<&RpcEndpointRecord> = self
            .for_chain(chain)
            .filter(|e| e.state == QualState::Qualified)
            .collect();
        let providers: std::collections::HashSet<&String> =
            qualified.iter().map(|e| &e.provider).collect();
        (qualified.len(), providers.len())
    }

    /// §36.9 — measured designed capacity: Σ verified_rps across
    /// qualified endpoints. `None` = no measured capacity, which must
    /// surface as unavailable — a budget is never fabricated.
    pub fn chain_capacity_rps(&self, chain: Chain) -> Option<f64> {
        let mut sum = 0.0;
        let mut any = false;
        for e in self
            .for_chain(chain)
            .filter(|e| e.state == QualState::Qualified)
        {
            if let Some(r) = e.verified_rps {
                sum += r;
                any = true;
            }
        }
        any.then_some(sum)
    }

    /// §36.9 — 80% utilization rule: planned workload must never
    /// exceed 80% of measured capacity; the 20% headroom absorbs
    /// bursts, failover and self-diagnosis probes.
    pub fn chain_budget_rps(&self, chain: Chain) -> Option<f64> {
        self.chain_capacity_rps(chain).map(|c| c * 0.80)
    }

    /// Daily request budget = budget_rps × 3600 × 24 (§36.9).
    pub fn chain_budget_day(&self, chain: Chain) -> Option<f64> {
        self.chain_budget_rps(chain).map(|r| r * 86_400.0)
    }

    pub fn summary(&self) -> RegistrySummary {
        let mut by_chain: HashMap<String, ChainStat> = HashMap::new();
        let mut providers = std::collections::HashSet::new();
        let (mut live, mut qualified) = (0usize, 0usize);
        for e in &self.endpoints {
            let st = by_chain.entry(e.chain.name().to_string()).or_default();
            st.discovered += 1;
            match e.state {
                QualState::Qualified => {
                    qualified += 1;
                    st.qualified += 1;
                    providers.insert(e.provider.clone());
                }
                QualState::Live | QualState::ChainVerified | QualState::MethodVerified
                | QualState::StateVerified | QualState::Degraded => {
                    live += 1;
                    st.live += 1;
                }
                _ => {}
            }
        }
        RegistrySummary {
            updated_ms: self.updated_ms,
            total: self.endpoints.len(),
            live,
            qualified,
            providers: providers.len(),
            by_chain,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChainStat {
    pub discovered: usize,
    pub live: usize,
    pub qualified: usize,
}

/// Wire-summary published by the scout after each sweep — journaled via
/// the ingest fan-in so qualification evidence lands in the journal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrySummary {
    pub updated_ms: u64,
    pub total: usize,
    pub live: usize,
    pub qualified: usize,
    /// Distinct qualified providers across all chains (diversity evidence).
    pub providers: usize,
    pub by_chain: HashMap<String, ChainStat>,
}
