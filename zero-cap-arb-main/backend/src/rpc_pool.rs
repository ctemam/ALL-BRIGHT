//! Multi-endpoint RPC pool with health tracking, rotation and per-endpoint
//! cooldown.
//!
//! The scanner previously took `chain.rpc_urls` and used the first URL that
//! responded. With one endpoint per chain that meant a single 429 or a slow
//! node degraded the whole chain, and there was no way to observe it.
//!
//! This module keeps a per-endpoint health record so the scanner can:
//!   * pick the fastest *healthy* endpoint instead of the first one,
//!   * back off endpoints that rate-limit instead of hammering them,
//!   * expose real pool health over the API rather than one opaque URL.
//!
//! Selection is deliberately simple: available beats cooling, then lower
//! observed latency wins, then a round-robin cursor breaks ties. This is a
//! free-public-endpoint pool where the dominant failure mode is HTTP 429, not
//! latency.

use dashmap::DashMap;
use futures_util::future::join_all;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How long an endpoint is excluded after a rate-limit or transport failure.
const COOLDOWN_AFTER_RATE_LIMIT: Duration = Duration::from_secs(60);
const COOLDOWN_AFTER_ERROR: Duration = Duration::from_secs(15);

/// Consecutive successes required before a quarantined endpoint is trusted
/// again. Prevents flapping straight back into a 429.
const RECOVERY_SUCCESSES: u32 = 2;

/// An endpoint that failed this many consecutive times is quarantined even
/// after the per-failure cooldown elapses.
const QUARANTINE_THRESHOLD: u32 = 5;
const QUARANTINE_DURATION: Duration = Duration::from_secs(300);

/// Upper bound on tracked latency samples per endpoint (ring buffer size).
const LATENCY_SAMPLES: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EndpointState {
    /// Never probed, or probed successfully with no failures since.
    Healthy,
    /// Temporarily excluded after a failure.
    Cooling,
    /// Excluded for a long period after repeated failures.
    Quarantined,
}

#[derive(Debug, Clone, Serialize)]
pub struct EndpointHealth {
    pub url: String,
    pub state: EndpointState,
    pub consecutive_failures: u32,
    pub consecutive_successes: u32,
    /// Mean of the most recent `LATENCY_SAMPLES` round trips, in ms.
    /// `-1.0` when the endpoint has not been probed yet.
    pub avg_latency_ms: f64,
    pub last_error: Option<String>,
    pub total_successes: u64,
    pub total_failures: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChainPoolHealth {
    pub chain_id: u64,
    pub chain_name: String,
    pub total_endpoints: usize,
    pub available_endpoints: usize,
    pub cooling: usize,
    pub quarantined: usize,
    /// Best average latency across available endpoints, ms.
    /// `-1.0` when no endpoint has been probed yet.
    pub best_latency_ms: f64,
}

#[derive(Debug)]
pub enum RpcError {
    NoEndpoints(u64),
    Transport(String),
    Http(String, u16),
    RateLimited(String, u16),
    JsonRpc(serde_json::Value),
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RpcError::NoEndpoints(c) => write!(f, "no RPC endpoints registered for chain {c}"),
            RpcError::Transport(e) => write!(f, "transport error: {e}"),
            RpcError::Http(u, c) => write!(f, "HTTP {c} from {u}"),
            RpcError::RateLimited(u, c) => write!(f, "rate limited (HTTP {c}) by {u}"),
            RpcError::JsonRpc(e) => write!(f, "JSON-RPC error: {e}"),
        }
    }
}

impl std::error::Error for RpcError {}

/// Mutable per-endpoint state. Kept internal; exposed as `EndpointHealth`.
struct Endpoint {
    url: String,
    state: EndpointState,
    consecutive_failures: u32,
    consecutive_successes: u32,
    /// Ring of recent latencies (ms).
    latencies_ms: Vec<f64>,
    last_error: Option<String>,
    total_successes: u64,
    total_failures: u64,
    /// Instant until which this endpoint is excluded.
    cooldown_until: Option<Instant>,
}

impl Endpoint {
    fn new(url: String) -> Self {
        Self {
            url,
            state: EndpointState::Healthy,
            consecutive_failures: 0,
            consecutive_successes: 0,
            latencies_ms: Vec::with_capacity(LATENCY_SAMPLES),
            last_error: None,
            total_successes: 0,
            total_failures: 0,
            cooldown_until: None,
        }
    }

    /// `f64::MAX` for unprobed endpoints so they sort last but stay eligible.
    fn score(&self) -> f64 {
        if self.latencies_ms.is_empty() {
            f64::MAX
        } else {
            self.latencies_ms.iter().sum::<f64>() / self.latencies_ms.len() as f64
        }
    }

    fn is_available(&self, now: Instant) -> bool {
        match self.cooldown_until {
            Some(until) => now >= until,
            None => true,
        }
    }

    fn record_success(&mut self, latency: Duration) {
        self.consecutive_failures = 0;
        self.consecutive_successes += 1;
        self.total_successes += 1;
        self.cooldown_until = None;
        self.last_error = None;
        // A quarantined endpoint must win several consecutive times before it
        // is trusted again; one lucky response must not re-enable a 429 node.
        self.state = if self.state == EndpointState::Quarantined
            && self.consecutive_successes < RECOVERY_SUCCESSES
        {
            EndpointState::Quarantined
        } else {
            EndpointState::Healthy
        };

        let ms = latency.as_secs_f64() * 1000.0;
        if self.latencies_ms.len() == LATENCY_SAMPLES {
            self.latencies_ms.remove(0);
        }
        self.latencies_ms.push(ms);
    }

    fn record_failure(&mut self, err: String, rate_limited: bool) {
        self.consecutive_successes = 0;
        self.consecutive_failures += 1;
        self.total_failures += 1;
        self.last_error = Some(err);

        let now = Instant::now();
        if self.consecutive_failures >= QUARANTINE_THRESHOLD {
            self.state = EndpointState::Quarantined;
            self.cooldown_until = Some(now + QUARANTINE_DURATION);
        } else {
            self.state = EndpointState::Cooling;
            let d = if rate_limited {
                COOLDOWN_AFTER_RATE_LIMIT
            } else {
                COOLDOWN_AFTER_ERROR
            };
            self.cooldown_until = Some(now + d);
        }
    }

    fn to_health(&self) -> EndpointHealth {
        EndpointHealth {
            url: self.url.clone(),
            state: self.state,
            consecutive_failures: self.consecutive_failures,
            consecutive_successes: self.consecutive_successes,
            avg_latency_ms: if self.latencies_ms.is_empty() {
                -1.0
            } else {
                self.score()
            },
            last_error: self.last_error.clone(),
            total_successes: self.total_successes,
            total_failures: self.total_failures,
        }
    }
}

/// A pool of RPC endpoints, keyed by chain id.
pub struct RpcPool {
    endpoints: DashMap<u64, Arc<DashMap<usize, Endpoint>>>,
    /// Round-robin cursor per chain, to spread concurrent callers apart.
    cursors: DashMap<u64, AtomicUsize>,
    /// Caps concurrent outbound requests across the whole pool.
    semaphore: Arc<tokio::sync::Semaphore>,
    /// Stable identity counter for endpoints across all chains.
    next_index: AtomicUsize,
    http: reqwest::Client,
}

impl RpcPool {
    /// Build a pool. `max_concurrency` caps simultaneous outbound requests.
    pub fn new(max_concurrency: usize) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(5))
            .pool_max_idle_per_host(8)
            .user_agent(concat!("zero-cap-arb/", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_default();

        Self {
            endpoints: DashMap::new(),
            cursors: DashMap::new(),
            semaphore: Arc::new(tokio::sync::Semaphore::new(max_concurrency.max(1))),
            next_index: AtomicUsize::new(0),
            http,
        }
    }

    /// Register a chain's endpoints, replacing any existing registration.
    /// Blank strings, duplicates and non-HTTP placeholders are dropped.
    pub fn register(&self, chain_id: u64, urls: &[String]) {
        let mut filtered: Vec<String> = Vec::with_capacity(urls.len());
        for u in urls {
            let t = u.trim();
            if t.is_empty() || !t.starts_with("http") {
                continue;
            }
            if !filtered.iter().any(|e| e == t) {
                filtered.push(t.to_string());
            }
        }
        if filtered.is_empty() {
            return;
        }

        let map = Arc::new(DashMap::new());
        for url in filtered {
            let idx = self.next_index.fetch_add(1, Ordering::Relaxed);
            map.insert(idx, Endpoint::new(url));
        }
        self.endpoints.insert(chain_id, map);
        self.cursors
            .entry(chain_id)
            .or_insert_with(|| AtomicUsize::new(0));
    }

    /// Select the best available endpoint for a chain.
    ///
    /// Returns `(index, url)`; the index is needed to report the outcome back
    /// via `record_success` / `record_failure`. If every endpoint is cooling,
    /// the least-bad one is returned rather than failing outright.
    pub fn select(&self, chain_id: u64) -> Option<(usize, String)> {
        let map = self.endpoints.get(&chain_id)?;
        let now = Instant::now();

        let cursor = self
            .cursors
            .get(&chain_id)
            .map(|c| c.fetch_add(1, Ordering::Relaxed))
            .unwrap_or(0);

        // Snapshot out of the DashMap so scoring does not hold a shard lock.
        let mut candidates: Vec<(usize, String, f64, bool)> = map
            .iter()
            .map(|kv| {
                let ep = kv.value();
                (
                    kv.key().to_owned(),
                    ep.url.clone(),
                    ep.score(),
                    !ep.is_available(now),
                )
            })
            .collect();

        if candidates.is_empty() {
            return None;
        }

        // Available beats cooling; within the same availability lower latency
        // wins. The round-robin cursor breaks exact ties so concurrent callers
        // do not all converge on the same endpoint.
        candidates.sort_by(|a, b| {
            a.3.cmp(&b.3)
                .then_with(|| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal))
                .then_with(|| a.0.cmp(&b.0))
                .then_with(|| a.0.wrapping_sub(cursor).cmp(&b.0.wrapping_sub(cursor)))
        });

        let (idx, url, _, cooling) = candidates.remove(0);
        if cooling {
            tracing::debug!(
                chain_id,
                "all RPC endpoints cooling; attempting least-recently-failed"
            );
        }
        Some((idx, url))
    }

    pub fn record_success(&self, chain_id: u64, idx: usize, latency: Duration) {
        if let Some(map) = self.endpoints.get(&chain_id) {
            if let Some(mut ep) = map.get_mut(&idx) {
                ep.record_success(latency);
            }
        }
    }

    /// Record a failed call. `rate_limited` selects the longer cooldown, since
    /// an HTTP 429 needs the provider to recover, not just a retry.
    pub fn record_failure(&self, chain_id: u64, idx: usize, err: String, rate_limited: bool) {
        if let Some(map) = self.endpoints.get(&chain_id) {
            if let Some(mut ep) = map.get_mut(&idx) {
                ep.record_failure(err, rate_limited);
            }
        }
    }

    /// Acquire a concurrency permit, held for the duration of one RPC call.
    /// Bounds total in-flight requests so a wide scan cannot open hundreds of
    /// sockets against rate-limited free endpoints.
    pub async fn acquire(&self) -> tokio::sync::OwnedSemaphorePermit {
        self.semaphore
            .clone()
            .acquire_owned()
            .await
            .expect("RPC semaphore never closes")
    }

    /// Issue a single JSON-RPC call against the best endpoint for `chain_id`.
    ///
    /// Handles endpoint selection, the concurrency permit, latency recording
    /// and failover to another endpoint on transport errors. A single flaky
    /// node must not fail the request when the chain has 25 others.
    pub async fn call(
        &self,
        chain_id: u64,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, RpcError> {
        const MAX_ATTEMPTS: usize = 3;
        let mut last_err = RpcError::NoEndpoints(chain_id);

        for _ in 0..MAX_ATTEMPTS {
            let (idx, url) = match self.select(chain_id) {
                Some(v) => v,
                None => return Err(RpcError::NoEndpoints(chain_id)),
            };

            let _permit = self.acquire().await;
            let payload = serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": method,
                "params": params,
            });

            let started = Instant::now();
            let outcome = self.http.post(&url).json(&payload).send().await;
            let elapsed = started.elapsed();

            match outcome {
                Ok(resp) => {
                    let code = resp.status().as_u16();
                    if code == 429 || code == 503 {
                        self.record_failure(chain_id, idx, format!("HTTP {code}"), true);
                        last_err = RpcError::RateLimited(url, code);
                        continue;
                    }
                    if !resp.status().is_success() {
                        self.record_failure(chain_id, idx, format!("HTTP {code}"), false);
                        last_err = RpcError::Http(url, code);
                        continue;
                    }

                    let body: serde_json::Value = match resp.json().await {
                        Ok(v) => v,
                        Err(e) => {
                            self.record_failure(chain_id, idx, format!("decode: {e}"), false);
                            last_err = RpcError::Transport(format!("decode: {e}"));
                            continue;
                        }
                    };

                    // A JSON-RPC level error is not the endpoint's fault â€” the
                    // request was served â€” so the endpoint stays healthy.
                    if let Some(err) = body.get("error") {
                        self.record_success(chain_id, idx, elapsed);
                        return Err(RpcError::JsonRpc(err.clone()));
                    }

                    self.record_success(chain_id, idx, elapsed);
                    return Ok(body
                        .get("result")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null));
                }
                Err(e) => {
                    self.record_failure(chain_id, idx, e.to_string(), false);
                    last_err = RpcError::Transport(e.to_string());
                }
            }
        }

        Err(last_err)
    }

    /// Probe every endpoint of a chain concurrently with `eth_chainId`.
    ///
    /// Probing is the only way to learn real latency and weed out dead nodes,
    /// so it is exposed rather than hidden behind startup.
    pub async fn probe_chain(&self, chain_id: u64) -> usize {
        let Some(map) = self.endpoints.get(&chain_id) else {
            return 0;
        };
        let targets: Vec<(usize, String)> = map
            .iter()
            .map(|kv| (kv.key().to_owned(), kv.value().url.clone()))
            .collect();
        drop(map);

        let futures = targets
            .iter()
            .map(|(idx, url)| self.probe(chain_id, *idx, url));
        join_all(futures).await.iter().filter(|ok| **ok).count()
    }

    /// Probe a single endpoint and record the outcome.
    pub async fn probe(&self, chain_id: u64, idx: usize, url: &str) -> bool {
        let _permit = self.acquire().await;
        let payload = serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "eth_chainId", "params": []
        });
        let started = Instant::now();
        match self.http.post(url).json(&payload).send().await {
            Ok(resp) if resp.status().is_success() => {
                let ok = matches!(
                    resp.json::<serde_json::Value>().await,
                    Ok(v) if v.get("result").is_some()
                );
                if ok {
                    self.record_success(chain_id, idx, started.elapsed());
                } else {
                    self.record_failure(chain_id, idx, "probe: no result field".into(), false);
                }
                ok
            }
            Ok(resp) => {
                let code = resp.status().as_u16();
                self.record_failure(chain_id, idx, format!("HTTP {code}"), code == 429);
                false
            }
            Err(e) => {
                self.record_failure(chain_id, idx, e.to_string(), false);
                false
            }
        }
    }

    /// Health summary for one chain.
    pub fn chain_health(&self, chain_id: u64, chain_name: &str) -> ChainPoolHealth {
        let (mut available, mut cooling, mut quarantined) = (0, 0, 0);
        let (mut best, mut total) = (f64::MAX, 0);

        if let Some(map) = self.endpoints.get(&chain_id) {
            let now = Instant::now();
            for kv in map.iter() {
                total += 1;
                let ep = kv.value();
                // Count by the endpoint's *state*, not just by whether the
                // cooldown has elapsed. A quarantined endpoint that has won one
                // success is selectable again, but it is still recovering and
                // must still be reported as quarantined rather than healthy.
                match ep.state {
                    EndpointState::Quarantined => quarantined += 1,
                    EndpointState::Cooling if !ep.is_available(now) => cooling += 1,
                    _ => {
                        if ep.is_available(now) {
                            available += 1;
                            if !ep.latencies_ms.is_empty() {
                                best = best.min(ep.score());
                            }
                        } else {
                            cooling += 1;
                        }
                    }
                }
            }
        }

        ChainPoolHealth {
            chain_id,
            chain_name: chain_name.to_string(),
            total_endpoints: total,
            available_endpoints: available,
            cooling,
            quarantined,
            // `best` starts at f64::MAX, which IS finite, so a plain
            // `is_finite()` check would report 1.79e308 as a latency. Compare
            // against the sentinel directly and report -1.0 for "not probed".
            best_latency_ms: if best == f64::MAX { -1.0 } else { best },
        }
    }

    /// Per-endpoint detail for a chain, fastest first.
    pub fn endpoint_health(&self, chain_id: u64) -> Vec<EndpointHealth> {
        let mut out = Vec::new();
        if let Some(map) = self.endpoints.get(&chain_id) {
            for kv in map.iter() {
                out.push(kv.value().to_health());
            }
        }
        out.sort_by(|a, b| {
            let ka = if a.avg_latency_ms < 0.0 {
                f64::MAX
            } else {
                a.avg_latency_ms
            };
            let kb = if b.avg_latency_ms < 0.0 {
                f64::MAX
            } else {
                b.avg_latency_ms
            };
            ka.partial_cmp(&kb).unwrap_or(std::cmp::Ordering::Equal)
        });
        out
    }

    /// Total endpoints registered across every chain.
    pub fn total_endpoints(&self) -> usize {
        self.endpoints.iter().map(|kv| kv.value().len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn urls(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("https://rpc{i}.example/")).collect()
    }

    /// Look up the stable index assigned to a given URL.
    fn index_of(pool: &RpcPool, chain: u64, url: &str) -> usize {
        let map = pool.endpoints.get(&chain).unwrap();
        // Bind the result to a local: chaining off `.iter()` directly would
        // create a temporary that borrows the guard and outlives it.
        let found = map
            .iter()
            .find(|kv| kv.value().url == url)
            .map(|kv| kv.key().to_owned());
        found.expect("endpoint registered")
    }

    #[test]
    fn register_dedupes_and_rejects_non_http() {
        let pool = RpcPool::new(4);
        pool.register(
            1,
            &[
                "https://a.example/".into(),
                "https://a.example/".into(), // duplicate
                "  ".into(),                 // blank
                "0xYourWallet".into(),       // placeholder, not http
                "https://b.example/".into(),
            ],
        );
        assert_eq!(pool.chain_health(1, "Ethereum").total_endpoints, 2);
    }

    #[test]
    fn select_returns_registered_endpoint() {
        let pool = RpcPool::new(4);
        pool.register(1, &urls(3));
        let (idx, url) = pool.select(1).expect("endpoint should be selectable");
        assert!(url.starts_with("https://rpc"));
        assert!(idx < 3);
    }

    #[test]
    fn select_returns_none_for_unknown_chain() {
        let pool = RpcPool::new(4);
        assert!(pool.select(999).is_none());
    }

    #[test]
    fn rate_limit_cools_endpoint_down() {
        let pool = RpcPool::new(4);
        pool.register(1, &urls(1));
        let (idx, _) = pool.select(1).unwrap();
        pool.record_failure(1, idx, "HTTP 429".into(), true);

        let h = pool.chain_health(1, "Ethereum");
        assert_eq!(h.cooling, 1);
        assert_eq!(h.available_endpoints, 0);
    }

    #[test]
    fn repeated_failures_quarantine() {
        let pool = RpcPool::new(4);
        pool.register(1, &urls(1));
        let (idx, _) = pool.select(1).unwrap();
        for _ in 0..QUARANTINE_THRESHOLD {
            pool.record_failure(1, idx, "boom".into(), false);
        }
        assert_eq!(pool.chain_health(1, "Ethereum").quarantined, 1);
    }

    #[test]
    fn quarantined_endpoint_needs_consecutive_recovery() {
        let pool = RpcPool::new(4);
        pool.register(1, &urls(1));
        let (idx, _) = pool.select(1).unwrap();
        for _ in 0..QUARANTINE_THRESHOLD {
            pool.record_failure(1, idx, "boom".into(), false);
        }
        assert_eq!(pool.chain_health(1, "Ethereum").quarantined, 1);

        // One success must NOT clear quarantine.
        pool.record_success(1, idx, Duration::from_millis(10));
        assert_eq!(pool.chain_health(1, "Ethereum").quarantined, 1);

        // A second consecutive success does.
        pool.record_success(1, idx, Duration::from_millis(10));
        assert_eq!(pool.chain_health(1, "Ethereum").available_endpoints, 1);
    }

    #[test]
    fn success_recovers_and_records_latency() {
        let pool = RpcPool::new(4);
        pool.register(1, &urls(1));
        let (idx, _) = pool.select(1).unwrap();

        pool.record_failure(1, idx, "HTTP 429".into(), true);
        assert_eq!(pool.chain_health(1, "Ethereum").available_endpoints, 0);

        pool.record_success(1, idx, Duration::from_millis(42));
        let h = pool.chain_health(1, "Ethereum");
        assert_eq!(h.available_endpoints, 1);
        assert!((h.best_latency_ms - 42.0).abs() < 0.001);
    }

    #[test]
    fn one_bad_endpoint_does_not_hide_the_others() {
        let pool = RpcPool::new(4);
        pool.register(1, &urls(4));
        let (bad, _) = pool.select(1).unwrap();
        pool.record_failure(1, bad, "HTTP 429".into(), true);

        let h = pool.chain_health(1, "Ethereum");
        assert_eq!(h.total_endpoints, 4);
        assert_eq!(h.available_endpoints, 3);
    }

    #[test]
    fn fastest_endpoint_wins_once_probed() {
        let pool = RpcPool::new(4);
        let all = urls(3);
        pool.register(1, &all);

        // Distinct latencies: 300ms, 100ms, 200ms.
        pool.record_success(1, index_of(&pool, 1, &all[0]), Duration::from_millis(300));
        pool.record_success(1, index_of(&pool, 1, &all[1]), Duration::from_millis(100));
        pool.record_success(1, index_of(&pool, 1, &all[2]), Duration::from_millis(200));

        // Selection must land on the 100ms endpoint.
        let (_, chosen) = pool.select(1).unwrap();
        assert_eq!(chosen, all[1]);
    }

    #[test]
    fn available_endpoint_beats_lower_latency_cooling_one() {
        let pool = RpcPool::new(4);
        let all = urls(2);
        pool.register(1, &all);

        // The fastest endpoint rate-limits; the slow one must still be chosen.
        pool.record_success(1, index_of(&pool, 1, &all[0]), Duration::from_millis(5));
        pool.record_success(1, index_of(&pool, 1, &all[1]), Duration::from_millis(900));
        pool.record_failure(1, index_of(&pool, 1, &all[0]), "HTTP 429".into(), true);

        let (_, chosen) = pool.select(1).unwrap();
        assert_eq!(chosen, all[1]);
    }

    #[test]
    fn endpoint_health_sorts_fastest_first_and_keeps_unprobed_last() {
        let pool = RpcPool::new(4);
        let all = urls(3);
        pool.register(1, &all);
        pool.record_success(1, index_of(&pool, 1, &all[0]), Duration::from_millis(50));
        pool.record_success(1, index_of(&pool, 1, &all[2]), Duration::from_millis(10));

        let h = pool.endpoint_health(1);
        assert_eq!(h.len(), 3);
        assert_eq!(h[0].url, all[2]); // 10ms
        assert_eq!(h[1].url, all[0]); // 50ms
        assert_eq!(h[2].avg_latency_ms, -1.0); // unprobed sinks to the bottom
    }

    #[test]
    fn total_endpoints_across_chains() {
        let pool = RpcPool::new(4);
        pool.register(1, &urls(25));
        pool.register(42161, &urls(25));
        assert_eq!(pool.total_endpoints(), 50);
    }

    #[tokio::test]
    async fn semaphore_bounds_concurrency() {
        let pool = RpcPool::new(2);
        let sem = pool.semaphore.clone();
        let a = sem.clone().acquire_owned().await.unwrap();
        let b = sem.clone().acquire_owned().await.unwrap();
        assert_eq!(sem.available_permits(), 0);
        drop(a);
        drop(b);
        assert_eq!(sem.available_permits(), 2);
    }
}
