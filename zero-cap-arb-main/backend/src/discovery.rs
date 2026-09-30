//! External data feeds for opportunity discovery.
//!
//! Three integrations that expand our search space beyond the hard-coded
//! token/venue list:
//!
//! 1. **DEX Screener** — discovers pools with anomalous spreads across the
//!    full DEX landscape (thousands of pairs vs our 15 tokens).
//! 2. **DeFiLlama** — TVL-based pool ranking so we scan deepest pools first
//!    and dynamically filter out dead pools.
//! 3. **Binance ticker** — CEX benchmark price; any DEX pool that deviates
//!    from Binance spot is a high-confidence arb candidate.

use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

// ───────────────────────────────────────────────────────────────────────
//  Shared types
// ───────────────────────────────────────────────────────────────────────

/// A discovered token that should be fed into the on-chain scanner.
#[derive(Debug, Clone)]
pub struct DiscoveredToken {
    pub symbol: String,
    pub address: String,
    pub chain_id: u64,
    /// Where the discovery came from.
    pub source: DiscoverySource,
    /// Estimated spread or deviation that triggered discovery.
    pub signal_pct: f64,
}

#[derive(Debug, Clone, Copy)]
pub enum DiscoverySource {
    DexScreener,
    CexDeviation,
}

/// CEX benchmark price for a token symbol.
#[derive(Debug, Clone)]
pub struct CexBenchmark {
    pub symbol: String,
    pub price_usd: f64,
    pub timestamp: Instant,
}

/// TVL info for a chain/protocol.
#[derive(Debug, Clone)]
pub struct ProtocolTvl {
    pub name: String,
    pub chain: String,
    pub tvl_usd: f64,
}

// ───────────────────────────────────────────────────────────────────────
//  DEX Screener client
// ───────────────────────────────────────────────────────────────────────

/// DEX Screener free API — https://docs.dexscreener.com/api/reference
/// Rate limit: ~60 req/min on free tier.
struct DexScreenerClient {
    client: Client,
}

// Chain slug mapping for DEX Screener API
fn chain_slug(chain_id: u64) -> Option<&'static str> {
    match chain_id {
        1 => Some("ethereum"),
        42161 => Some("arbitrum"),
        10 => Some("optimism"),
        137 => Some("polygon"),
        56 => Some("bsc"),
        43114 => Some("avalanche"),
        8453 => Some("base"),
        42220 => Some("celo"),
        100 => Some("gnosischain"),
        59144 => Some("linea"),
        _ => None,
    }
}

fn slug_to_chain_id(slug: &str) -> u64 {
    match slug {
        "ethereum" => 1,
        "arbitrum" => 42161,
        "optimism" => 10,
        "polygon" => 137,
        "bsc" => 56,
        "avalanche" => 43114,
        "base" => 8453,
        "celo" => 42220,
        "gnosischain" => 100,
        "linea" => 59144,
        _ => 0,
    }
}

#[derive(Deserialize, Debug)]
struct DexScreenerResponse {
    pairs: Option<Vec<DexScreenerPair>>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct DexScreenerPair {
    chain_id: Option<String>,
    dex_id: Option<String>,
    base_token: Option<DexScreenerToken>,
    quote_token: Option<DexScreenerToken>,
    price_usd: Option<String>,
    liquidity: Option<DexScreenerLiquidity>,
    price_change: Option<DexScreenerPriceChange>,
}

#[derive(Deserialize, Debug)]
struct DexScreenerToken {
    address: Option<String>,
    symbol: Option<String>,
}

#[derive(Deserialize, Debug)]
struct DexScreenerLiquidity {
    usd: Option<f64>,
}

#[derive(Deserialize, Debug)]
struct DexScreenerPriceChange {
    m5: Option<f64>,
    h1: Option<f64>,
}

impl DexScreenerClient {
    fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(10))
                .user_agent("ZeroCapArb/1.0")
                .build()
                .expect("http client"),
        }
    }

    /// Search for tokens showing high 5m price movement (spread signal).
    /// Uses the /dex/search endpoint to find volatile pairs.
    async fn search_boosted_tokens(
        &self,
        chain_id: u64,
    ) -> Vec<DiscoveredToken> {
        let slug = match chain_slug(chain_id) {
            Some(s) => s,
            None => return Vec::new(),
        };

        // Query top-volume pairs on this chain. The /latest/dex/tokens
        // endpoint is per-address, so we use /dex/pairs/{chain} for
        // trending pairs. But the free API doesn't have a "trending"
        // endpoint for a chain. Instead, search for common base tokens
        // and find pairs with high 5m price change.
        let search_terms = ["USDC", "USDT", "WETH", "ETH"];
        let mut discovered = Vec::new();

        for term in &search_terms {
            let url = format!(
                "https://api.dexscreener.com/latest/dex/search?q={}",
                term,
            );
            let resp = match self.client.get(&url).send().await {
                Ok(r) => r,
                Err(e) => {
                    debug!(error = %e, "DEX Screener search failed");
                    continue;
                }
            };

            let data: DexScreenerResponse = match resp.json().await {
                Ok(d) => d,
                Err(e) => {
                    debug!(error = %e, "DEX Screener parse failed");
                    continue;
                }
            };

            if let Some(pairs) = data.pairs {
                for pair in pairs {
                    // Filter to our target chain
                    let pair_chain = pair.chain_id.as_deref().unwrap_or("");
                    if pair_chain != slug {
                        continue;
                    }

                    // Need sufficient liquidity
                    let liq = pair
                        .liquidity
                        .as_ref()
                        .and_then(|l| l.usd)
                        .unwrap_or(0.0);
                    if liq < 50_000.0 {
                        continue;
                    }

                    // High 5-minute price change indicates exploitable
                    // spread between this pool and others.
                    let m5_change = pair
                        .price_change
                        .as_ref()
                        .and_then(|pc| pc.m5)
                        .unwrap_or(0.0)
                        .abs();
                    if m5_change < 0.3 {
                        continue;
                    }

                    if let Some(base) = &pair.base_token {
                        let sym = base.symbol.clone().unwrap_or_default();
                        let addr = base.address.clone().unwrap_or_default();
                        if !addr.is_empty()
                            && addr.starts_with("0x")
                            && !sym.is_empty()
                        {
                            discovered.push(DiscoveredToken {
                                symbol: sym,
                                address: addr,
                                chain_id,
                                source: DiscoverySource::DexScreener,
                                signal_pct: m5_change,
                            });
                        }
                    }
                }
            }

            // Respect rate limits — 300ms between calls
            tokio::time::sleep(Duration::from_millis(300)).await;
        }

        // Deduplicate by address
        discovered.sort_by(|a, b| a.address.cmp(&b.address));
        discovered.dedup_by(|a, b| a.address.eq_ignore_ascii_case(&b.address));
        discovered
    }
}

// ───────────────────────────────────────────────────────────────────────
//  DeFiLlama client
// ───────────────────────────────────────────────────────────────────────

/// DeFiLlama free API — https://defillama.com/docs/api
/// No rate limits, open source.
struct DeFiLlamaClient {
    client: Client,
}

#[derive(Deserialize, Debug)]
struct LlamaProtocol {
    name: Option<String>,
    chain: Option<String>,
    #[serde(rename = "chainTvls")]
    chain_tvls: Option<HashMap<String, f64>>,
    tvl: Option<f64>,
    category: Option<String>,
}

impl DeFiLlamaClient {
    fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(15))
                .user_agent("ZeroCapArb/1.0")
                .build()
                .expect("http client"),
        }
    }

    /// Fetch DEX-category protocol TVLs, filtered to our chains.
    /// Returns a map of (chain_name_lower → total_dex_tvl_usd).
    async fn fetch_chain_dex_tvls(&self) -> HashMap<String, f64> {
        let url = "https://api.llama.fi/protocols";
        let resp = match self.client.get(url).send().await {
            Ok(r) => r,
            Err(e) => {
                warn!(error = %e, "DeFiLlama fetch failed");
                return HashMap::new();
            }
        };

        let protocols: Vec<LlamaProtocol> = match resp.json().await {
            Ok(d) => d,
            Err(e) => {
                warn!(error = %e, "DeFiLlama parse failed");
                return HashMap::new();
            }
        };

        let mut chain_tvl: HashMap<String, f64> = HashMap::new();
        let dex_categories = [
            "Dexes",
            "Liquid Staking",
            "Lending",
            "DEX Aggregator",
        ];

        for p in &protocols {
            let cat = p.category.as_deref().unwrap_or("");
            if !dex_categories.iter().any(|c| cat.eq_ignore_ascii_case(c)) {
                continue;
            }

            if let Some(chain_tvls) = &p.chain_tvls {
                for (chain, tvl) in chain_tvls {
                    let key = chain.to_lowercase();
                    *chain_tvl.entry(key).or_insert(0.0) += tvl;
                }
            }
        }

        chain_tvl
    }
}

/// Map DeFiLlama chain names to our chain IDs.
fn llama_chain_to_id(name: &str) -> Option<u64> {
    match name {
        "ethereum" => Some(1),
        "arbitrum" => Some(42161),
        "optimism" => Some(10),
        "polygon" => Some(137),
        "bsc" | "binance" => Some(56),
        "avalanche" => Some(43114),
        "base" => Some(8453),
        "celo" => Some(42220),
        "xdai" | "gnosis" => Some(100),
        "linea" => Some(59144),
        _ => None,
    }
}

// ───────────────────────────────────────────────────────────────────────
//  Binance ticker client (CEX price oracle)
// ───────────────────────────────────────────────────────────────────────

/// Binance public ticker API — no API key required for market data.
/// Rate limit: 2400 req/min (we use ~1 req per refresh).
struct BinanceClient {
    client: Client,
}

#[derive(Deserialize, Debug)]
struct BinanceTicker {
    symbol: Option<String>,
    price: Option<String>,
}

impl BinanceClient {
    fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(5))
                .user_agent("ZeroCapArb/1.0")
                .build()
                .expect("http client"),
        }
    }

    /// Fetch spot prices for all USDT-quoted pairs we care about.
    /// Returns a map of base_symbol → price_usd.
    async fn fetch_prices(&self) -> HashMap<String, f64> {
        // Batch request: all tickers at once (one API call)
        let url = "https://api.binance.com/api/v3/ticker/price";
        let resp = match self.client.get(url).send().await {
            Ok(r) => r,
            Err(e) => {
                warn!(error = %e, "Binance ticker fetch failed");
                return HashMap::new();
            }
        };

        let tickers: Vec<BinanceTicker> = match resp.json().await {
            Ok(d) => d,
            Err(e) => {
                warn!(error = %e, "Binance ticker parse failed");
                return HashMap::new();
            }
        };

        let mut prices = HashMap::new();

        // We only want USDT-quoted pairs for USD-denominated prices
        let targets = [
            "ETHUSDT", "BTCUSDT", "LINKUSDT", "UNIUSDT", "AAVEUSDT",
            "LDOUSDT", "CRVUSDT", "ARBUSDT", "OPUSDT", "MATICUSDT",
            "BNBUSDT", "AVAXUSDT", "CELOUSDT", "DAIUSDT", "PEPEUSDT",
            "WLDUSDT", "PENDLEUSDT", "MKRUSDT", "SUSHIUSDT", "COMPUSDT",
            "SNXUSDT", "INJUSDT", "RUNEUSDT", "GMXUSDT",
        ];

        for t in &tickers {
            let sym = t.symbol.as_deref().unwrap_or("");
            if !targets.contains(&sym) {
                continue;
            }
            if let Some(price_str) = &t.price {
                if let Ok(p) = price_str.parse::<f64>() {
                    // Strip "USDT" suffix to get base symbol
                    let base = sym.trim_end_matches("USDT");
                    prices.insert(base.to_string(), p);
                }
            }
        }

        // Stablecoins are ~$1
        prices.insert("USDC".to_string(), 1.0);
        prices.insert("USDT".to_string(), 1.0);
        prices.insert("DAI".to_string(), 1.0);

        prices
    }
}

// ───────────────────────────────────────────────────────────────────────
//  Unified Discovery Service
// ───────────────────────────────────────────────────────────────────────

/// Combines all three external feeds into a single service that the
/// scanner loop can query.
pub struct DiscoveryService {
    dex_screener: DexScreenerClient,
    defillama: DeFiLlamaClient,
    binance: BinanceClient,
    /// CEX benchmark prices — refreshed every 10s.
    pub cex_prices: Arc<RwLock<HashMap<String, f64>>>,
    /// Chain TVL data — refreshed every 5 min.
    chain_tvls: Arc<RwLock<HashMap<String, f64>>>,
    /// Discovered tokens from DEX Screener — refreshed every 30s.
    discovered_tokens: Arc<RwLock<Vec<DiscoveredToken>>>,
    /// Last refresh timestamps.
    last_cex_refresh: Arc<RwLock<Instant>>,
    last_tvl_refresh: Arc<RwLock<Instant>>,
    last_discovery_refresh: Arc<RwLock<Instant>>,
}

impl DiscoveryService {
    pub fn new() -> Self {
        let epoch = Instant::now() - Duration::from_secs(600);
        Self {
            dex_screener: DexScreenerClient::new(),
            defillama: DeFiLlamaClient::new(),
            binance: BinanceClient::new(),
            cex_prices: Arc::new(RwLock::new(HashMap::new())),
            chain_tvls: Arc::new(RwLock::new(HashMap::new())),
            discovered_tokens: Arc::new(RwLock::new(Vec::new())),
            last_cex_refresh: Arc::new(RwLock::new(epoch)),
            last_tvl_refresh: Arc::new(RwLock::new(epoch)),
            last_discovery_refresh: Arc::new(RwLock::new(epoch)),
        }
    }

    // ── Binance CEX price oracle ──────────────────────────────────────

    /// Refresh CEX prices if stale (>10s old).
    pub async fn refresh_cex_prices(&self) {
        let elapsed = self.last_cex_refresh.read().await.elapsed();
        if elapsed < Duration::from_secs(10) {
            return;
        }

        let prices = self.binance.fetch_prices().await;
        if !prices.is_empty() {
            let count = prices.len();
            *self.cex_prices.write().await = prices;
            *self.last_cex_refresh.write().await = Instant::now();
            debug!(count, "Binance CEX prices refreshed");
        }
    }

    /// Get CEX benchmark price for a token symbol.
    pub async fn cex_price(&self, symbol: &str) -> Option<f64> {
        let s = symbol.trim().to_ascii_uppercase();
        // Normalize wrapped to unwrapped
        let key = match s.as_str() {
            "WETH" => "ETH",
            "WBTC" => "BTC",
            "WMATIC" | "MATIC" => "MATIC",
            "WBNB" => "BNB",
            "WAVAX" => "AVAX",
            other => other,
        };
        self.cex_prices.read().await.get(key).copied()
    }

    /// Find tokens whose DEX price deviates from CEX benchmark.
    /// `dex_price_usd` is what the on-chain read shows;
    /// Returns the deviation percentage if it exceeds `min_dev_pct`.
    pub fn compute_cex_deviation(
        cex_price: f64,
        dex_price_usd: f64,
        min_dev_pct: f64,
    ) -> Option<f64> {
        if cex_price <= 0.0 || dex_price_usd <= 0.0 {
            return None;
        }
        let dev = ((dex_price_usd - cex_price) / cex_price * 100.0).abs();
        if dev >= min_dev_pct {
            Some(dev)
        } else {
            None
        }
    }

    // ── DeFiLlama TVL ─────────────────────────────────────────────────

    /// Refresh chain TVL data if stale (>5 min old).
    pub async fn refresh_tvls(&self) {
        let elapsed = self.last_tvl_refresh.read().await.elapsed();
        if elapsed < Duration::from_secs(300) {
            return;
        }

        let tvls = self.defillama.fetch_chain_dex_tvls().await;
        if !tvls.is_empty() {
            let count = tvls.len();
            *self.chain_tvls.write().await = tvls;
            *self.last_tvl_refresh.write().await = Instant::now();
            info!(chains = count, "DeFiLlama TVL data refreshed");
        }
    }

    /// Dynamic minimum depth for a chain, informed by its total DEX TVL.
    /// Falls back to the hard-coded defaults if no TVL data is available.
    pub async fn dynamic_min_depth_usd(&self, chain_id: u64) -> f64 {
        let slug = match chain_slug(chain_id) {
            Some(s) => s,
            None => {
                return match chain_id {
                    42161 | 10 | 8453 | 59144 | 100 => 100.0,
                    137 | 42220 => 200.0,
                    _ => 500.0,
                };
            }
        };

        let tvls = self.chain_tvls.read().await;
        let chain_tvl = tvls.get(slug).copied().unwrap_or(0.0);

        if chain_tvl <= 0.0 {
            // No TVL data — use hard-coded defaults
            return match chain_id {
                42161 | 10 | 8453 | 59144 | 100 => 100.0,
                137 | 42220 => 200.0,
                _ => 500.0,
            };
        }

        // Scale minimum depth proportionally to chain TVL.
        // Chains with >$1B DEX TVL: $500 min
        // Chains with $100M-$1B: $200 min
        // Chains with $10M-$100M: $100 min
        // Chains with <$10M: $50 min (still filter out dust)
        if chain_tvl > 1_000_000_000.0 {
            500.0
        } else if chain_tvl > 100_000_000.0 {
            200.0
        } else if chain_tvl > 10_000_000.0 {
            100.0
        } else {
            50.0
        }
    }

    // ── DEX Screener discovery ────────────────────────────────────────

    /// Refresh discovered tokens if stale (>30s old).
    pub async fn refresh_discovered_tokens(&self, chain_ids: &[u64]) {
        let elapsed = self.last_discovery_refresh.read().await.elapsed();
        if elapsed < Duration::from_secs(30) {
            return;
        }

        let mut all_discovered = Vec::new();
        // Only query 2-3 high-value chains per refresh to stay in rate limits
        let priority_chains: Vec<u64> = chain_ids
            .iter()
            .filter(|&&id| matches!(id, 1 | 42161 | 8453 | 10 | 137))
            .copied()
            .collect();

        for chain_id in priority_chains.iter().take(3) {
            let tokens = self.dex_screener.search_boosted_tokens(*chain_id).await;
            if !tokens.is_empty() {
                debug!(
                    chain_id,
                    found = tokens.len(),
                    "DEX Screener discovered tokens"
                );
            }
            all_discovered.extend(tokens);
        }

        if !all_discovered.is_empty() {
            info!(
                count = all_discovered.len(),
                "DEX Screener discovery: new token candidates"
            );
        }
        *self.discovered_tokens.write().await = all_discovered;
        *self.last_discovery_refresh.write().await = Instant::now();
    }

    /// Get the currently discovered tokens (for merging into the scan list).
    pub async fn get_discovered_tokens(&self) -> Vec<DiscoveredToken> {
        self.discovered_tokens.read().await.clone()
    }

    // ── Combined refresh ──────────────────────────────────────────────

    /// Run all refresh operations in one call. Each has its own staleness
    /// check, so calling this every scan cycle is safe and cheap.
    pub async fn refresh_all(&self, chain_ids: &[u64]) {
        // Run CEX + TVL in parallel (independent APIs)
        let (_, _) = tokio::join!(
            self.refresh_cex_prices(),
            self.refresh_tvls(),
        );
        // DEX Screener after — uses more rate limit budget
        self.refresh_discovered_tokens(chain_ids).await;
    }

    /// Summary stats for logging.
    pub async fn stats(&self) -> (usize, usize, usize) {
        let cex = self.cex_prices.read().await.len();
        let tvl = self.chain_tvls.read().await.len();
        let disc = self.discovered_tokens.read().await.len();
        (cex, tvl, disc)
    }
}

// ───────────────────────────────────────────────────────────────────────
//  Tests
// ───────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_slug_roundtrip() {
        for &id in &[1u64, 42161, 10, 137, 56, 43114, 8453, 42220, 100, 59144] {
            let slug = chain_slug(id).expect("missing slug");
            assert_eq!(slug_to_chain_id(slug), id, "roundtrip failed for {id}");
        }
    }

    #[test]
    fn cex_deviation_positive() {
        // CEX = $100, DEX = $101.5 → 1.5% deviation
        let dev = DiscoveryService::compute_cex_deviation(100.0, 101.5, 0.3);
        assert!(dev.is_some());
        assert!((dev.unwrap() - 1.5).abs() < 0.01);
    }

    #[test]
    fn cex_deviation_below_threshold() {
        // CEX = $100, DEX = $100.1 → 0.1% < 0.3% threshold
        let dev = DiscoveryService::compute_cex_deviation(100.0, 100.1, 0.3);
        assert!(dev.is_none());
    }

    #[test]
    fn cex_deviation_zero_prices() {
        assert!(DiscoveryService::compute_cex_deviation(0.0, 100.0, 0.3).is_none());
        assert!(DiscoveryService::compute_cex_deviation(100.0, 0.0, 0.3).is_none());
    }

    #[test]
    fn llama_chain_mapping() {
        assert_eq!(llama_chain_to_id("ethereum"), Some(1));
        assert_eq!(llama_chain_to_id("arbitrum"), Some(42161));
        assert_eq!(llama_chain_to_id("bsc"), Some(56));
        assert_eq!(llama_chain_to_id("xdai"), Some(100));
        assert_eq!(llama_chain_to_id("unknown"), None);
    }
}
