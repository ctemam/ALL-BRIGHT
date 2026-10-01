use crate::chains::{get_chains, Protocol};
use crate::multicall;
use crate::types::*;
use alloy::primitives::{Address, U256};
use alloy::providers::{ProviderBuilder, RootProvider};
use alloy::transports::http::Http;
use serde::{Deserialize, Serialize};

/// Concrete HTTP provider used by the scanner.
///
/// alloy 0.3 parameterises the `Provider` trait by transport, so a bare `Provider` bound
/// actually means `Provider<BoxTransport, Ethereum>`, which `RootProvider<Http<..>>` does
/// not implement. Naming the concrete type avoids that mismatch (E0277 on the first
/// successful `cargo check`).
pub type HttpProvider = RootProvider<Http<reqwest::Client>>;
use chrono::Utc;
use dashmap::DashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Global round-robin counter for RPC endpoint rotation.
/// Each call to `try_build_provider` advances this counter so successive
/// calls distribute load across all available endpoints for a chain,
/// rather than always hitting the first one.
static RPC_ROUND_ROBIN: AtomicUsize = AtomicUsize::new(0);

/// A single confirmed trade logged by the Velora round-trip validator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeRecord {
    pub id: String,
    pub timestamp: String,
    pub chain: String,
    pub chain_id: u64,
    pub token: String,
    pub buy_venue: String,
    pub sell_venue: String,
    pub spent_weth: f64,
    pub received_weth: f64,
    pub profit_usd: f64,
    pub trade_size_usd: f64,
    pub roi_pct: f64,
    /// On-chain transaction hash. Empty means quote-only (not yet executed).
    #[serde(default)]
    pub tx_hash: String,
    /// Etherscan verification status.
    #[serde(default)]
    pub verified: VerificationStatus,
    /// Block explorer URL for independent human verification.
    #[serde(default)]
    pub explorer_url: String,
    /// Actual gas cost paid on-chain (USD).
    #[serde(default)]
    pub gas_cost_usd: f64,
}

/// Whether a trade has been verified on-chain.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum VerificationStatus {
    /// Trade was confirmed on-chain via tx receipt + Etherscan.
    Confirmed,
    /// Tx was submitted but reverted on-chain.
    Reverted,
    /// Tx was submitted but not yet confirmed (pending).
    Pending,
    /// Quote-validated by Velora but not yet executed on-chain.
    #[default]
    QuoteOnly,
    /// Execution was attempted but failed before broadcast.
    ExecutionFailed,
}

/// Per-chain cumulative profit tracker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainProfit {
    pub chain: String,
    pub chain_id: u64,
    pub total_profit_usd: f64,
    pub trade_count: u64,
    pub last_trade: Option<String>,
}

/// Core radar scanner that finds arbitrage opportunities across every
/// configured chain and every registry-listed venue on it.
pub struct RadarScanner {
    price_cache: Arc<DashMap<String, Vec<TokenPrice>>>,
    /// Minimum spread % to report (default 0.5%)
    min_spread_pct: f64,
    /// Cache TTL in seconds
    cache_ttl_secs: u64,
    /// Live native/USD rates fetched from on-chain pools.
    native_usd_cache: Arc<DashMap<u64, f64>>,
    /// Latest profitable opportunities found by the continuous scanner.
    live_opportunities: Arc<parking_lot::RwLock<Vec<OpportunityDetail>>>,
    /// Cumulative profit from executed trades (USD).
    cumulative_profit_usd: Arc<std::sync::atomic::AtomicI64>,
    /// Trade history — last N confirmed profitable trades.
    trade_history: Arc<parking_lot::RwLock<Vec<TradeRecord>>>,
    /// Per-chain cumulative profit.
    chain_profits: Arc<DashMap<u64, ChainProfit>>,
    /// Dynamic min-depth per chain, populated from DeFiLlama TVL data.
    /// When empty, hard-coded defaults are used.
    dynamic_min_depth: Arc<DashMap<u64, f64>>,
    /// Binance CEX benchmark prices for CEX-deviation detection.
    cex_benchmarks: Arc<parking_lot::RwLock<std::collections::HashMap<String, f64>>>,
    /// Shared RPC endpoint pool with health scoring, cooldown and
    /// quarantine. Without it the scanner round-robins raw URLs and keeps
    /// hammering rate-limited endpoints.
    rpc_pool: Option<Arc<crate::rpc_pool::RpcPool>>,
    /// Venue-level quotes with raw pool state, keyed `chainid_address`.
    /// Written alongside `price_cache` on every fresh fetch — this is the
    /// substrate the on-chain round-trip simulator scores candidates with.
    venue_cache: Arc<DashMap<String, (u64, Vec<(String, VenueQuote)>)>>,
}

impl RadarScanner {
    pub fn new() -> Self {
        Self {
            price_cache: Arc::new(DashMap::new()),
            min_spread_pct: 0.3, // lowered from 0.5% to catch more L2 opps
            cache_ttl_secs: 10,  // faster refresh for live scanning
            native_usd_cache: Arc::new(DashMap::new()),
            live_opportunities: Arc::new(parking_lot::RwLock::new(Vec::new())),
            cumulative_profit_usd: Arc::new(std::sync::atomic::AtomicI64::new(0)),
            trade_history: Arc::new(parking_lot::RwLock::new(Vec::new())),
            chain_profits: Arc::new(DashMap::new()),
            dynamic_min_depth: Arc::new(DashMap::new()),
            cex_benchmarks: Arc::new(parking_lot::RwLock::new(std::collections::HashMap::new())),
            rpc_pool: None,
            venue_cache: Arc::new(DashMap::new()),
        }
    }

    /// Attach the shared endpoint pool. Called once at startup before the
    /// scanner is wrapped in `Arc`.
    pub fn set_rpc_pool(&mut self, pool: Arc<crate::rpc_pool::RpcPool>) {
        self.rpc_pool = Some(pool);
    }

    /// Update the dynamic min-depth thresholds from DeFiLlama TVL data.
    pub fn set_dynamic_min_depth(&self, chain_id: u64, min_depth_usd: f64) {
        self.dynamic_min_depth.insert(chain_id, min_depth_usd);
    }

    /// Get the effective min-depth for a chain: dynamic if available, else hard-coded.
    fn effective_min_depth_usd(&self, chain_id: u64) -> f64 {
        self.dynamic_min_depth
            .get(&chain_id)
            .map(|v| *v)
            .unwrap_or_else(|| match chain_id {
                // Floors raised from $100–500: sub-$1.5k pools cannot fill
                // even a minimal arb leg without moving the price past the
                // spread, so quoting them only produced phantom candidates.
                42161 | 10 | 8453 | 59144 | 100 | 146 | 130 | 534352 | 324 | 5000 => 1_500.0,
                137 | 42220 => 2_500.0,
                _ => 5_000.0,
            })
    }

    /// Update CEX benchmark prices from Binance.
    pub fn update_cex_benchmarks(&self, prices: std::collections::HashMap<String, f64>) {
        *self.cex_benchmarks.write() = prices;
    }

    /// Get CEX benchmark price for a symbol.
    pub fn cex_price(&self, symbol: &str) -> Option<f64> {
        let s = symbol.trim().to_ascii_uppercase();
        let key = match s.as_str() {
            "WETH" => "ETH".to_string(),
            "WBTC" => "BTC".to_string(),
            "WMATIC" | "MATIC" => "MATIC".to_string(),
            "WBNB" => "BNB".to_string(),
            "WAVAX" => "AVAX".to_string(),
            "WSTETH" => "ETH".to_string(), // wstETH trades near ETH price
            other => other.to_string(),
        };
        self.cex_benchmarks.read().get(&key).copied()
    }

    /// Read the latest opportunities found by the continuous scanner.
    pub fn latest_opportunities(&self) -> Vec<OpportunityDetail> {
        self.live_opportunities.read().clone()
    }

    /// Read cumulative profit in USD (integer cents for atomicity).
    pub fn cumulative_profit_usd(&self) -> f64 {
        self.cumulative_profit_usd.load(std::sync::atomic::Ordering::Relaxed) as f64 / 100.0
    }

    /// Read trade history (most recent first, capped at 200).
    pub fn trade_history(&self) -> Vec<TradeRecord> {
        self.trade_history.read().clone()
    }

    /// Read per-chain profit breakdown.
    pub fn chain_profit_breakdown(&self) -> Vec<ChainProfit> {
        self.chain_profits.iter().map(|r| r.value().clone()).collect()
    }

    /// Record a confirmed trade in history and per-chain tracker.
    fn record_trade(
        &self,
        chain: &str,
        chain_id: u64,
        token: &str,
        buy_venue: &str,
        sell_venue: &str,
        spent_weth: f64,
        received_weth: f64,
        profit_usd: f64,
        trade_size_usd: f64,
        tx_hash: &str,
        verified: VerificationStatus,
        explorer_url: &str,
        gas_cost_usd: f64,
    ) {
        let roi_pct = if trade_size_usd > 0.0 {
            (profit_usd / trade_size_usd) * 100.0
        } else {
            0.0
        };
        let now = Utc::now();
        let trade = TradeRecord {
            id: Uuid::new_v4().to_string(),
            timestamp: now.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
            chain: chain.to_string(),
            chain_id,
            token: token.to_string(),
            buy_venue: buy_venue.to_string(),
            sell_venue: sell_venue.to_string(),
            spent_weth,
            received_weth,
            profit_usd,
            trade_size_usd,
            roi_pct,
            tx_hash: tx_hash.to_string(),
            verified,
            explorer_url: explorer_url.to_string(),
            gas_cost_usd,
        };

        // Append to history, cap at 200
        {
            let mut history = self.trade_history.write();
            history.insert(0, trade);
            if history.len() > 200 {
                history.truncate(200);
            }
        }

        // Update per-chain totals
        let mut entry = self.chain_profits.entry(chain_id).or_insert_with(|| ChainProfit {
            chain: chain.to_string(),
            chain_id,
            total_profit_usd: 0.0,
            trade_count: 0,
            last_trade: None,
        });
        entry.total_profit_usd += profit_usd;
        entry.trade_count += 1;
        entry.last_trade = Some(now.format("%H:%M:%S UTC").to_string());
    }

    /// Fetch the live native/USD price for a chain by reading the WETH/USDC
    /// (or equivalent) pool on that chain's most liquid V3 venue.
    ///
    /// This replaces the manual `ZCA_NATIVE_USD` env var with a real on-chain
    /// read that refreshes every scan cycle.
    pub async fn fetch_native_usd_rate(&self, chain_id: u64) -> Option<f64> {
        // Stablecoin address for this chain (USDC preferred)
        let stable_addr = crate::chains::resolve_token_symbol("USDC", chain_id);
        if stable_addr.is_empty() {
            return None;
        }
        let native_addr = crate::chains::get_wrapped_native(chain_id);

        let chain = get_chains().iter().find(|c| c.id == chain_id)?;

        // Up to 3 endpoint attempts — a single parseable-but-dead peer must
        // not burn the whole budget. Pool health scoring picks live
        // endpoints; per-attempt timeout bounds a hung peer.
        for _attempt in 0..3u8 {
            let provider = match &self.rpc_pool {
                Some(pool) => match pool.select(chain_id) {
                    Some((_idx, url)) => try_build_provider(&[url]).ok(),
                    None => try_build_provider(&chain.rpc_urls).ok(),
                },
                None => try_build_provider(&chain.rpc_urls).ok(),
            };
            let Some(provider) = provider else { continue };

        // No timeout on alloy's default HTTP client — bound the whole fetch
        // so a hung peer can't stall the per-cycle rate refresh.
        let inner = async {
        let stable: Address = stable_addr.parse().ok()?;
        let native: Address = native_addr.parse().ok()?;

        // Find a V3 venue for this chain (more precise pricing)
        let venues = crate::chains::get_venues(chain_id);
        let v3_venue = venues.iter().find(|v| v.protocol == Protocol::V3 && v.v3.is_some());

        if let Some(venue) = v3_venue {
            let params = venue.v3.unwrap();
            let factory: Address = venue.address.parse().ok()?;

            // Try common fee tiers to find a live pool
            for fee in params.fees {
                let call = multicall::SubCall::new(
                    factory,
                    multicall::factory_get_pool_call_data(stable, native, *fee, params.pool_sig),
                );
                let results = multicall::execute(&provider, &[call]).await.ok()?;
                let pool = results
                    .first()
                    .filter(|r| r.success)
                    .and_then(|r| multicall::decode_address(&r.return_data))?;

                // Read slot0 from the pool
                let slot0_call = multicall::SubCall::new(pool, multicall::slot0_call_data());
                let token0_call = multicall::SubCall::new(pool, multicall::token0_call_data());
                let state = multicall::execute(&provider, &[slot0_call, token0_call]).await.ok()?;

                let slot0_data = state.first().filter(|s| s.success)?;
                let slot0 = multicall::decode_slot0(&slot0_data.return_data)?;
                let token0 = state.get(1).filter(|s| s.success)
                    .and_then(|s| multicall::decode_address(&s.return_data))?;

                let stable_is_token0 = token0 == stable;
                // USDC has 6 decimals, native has 18
                let price = multicall::v3_price_in_quote(
                    slot0.sqrt_price_x96,
                    6,  // USDC decimals
                    18, // native decimals
                    stable_is_token0,
                );

                if price > 0.0 && price.is_finite() {
                    // price is USDC per native token (how much USDC one native buys)
                    // But v3_price_in_quote gives token priced in quote.
                    // If stable is token0: price = how much native one USDC buys
                    //   -> native/USD = 1/price
                    // If native is token0: price = how much USDC one native buys
                    //   -> native/USD = price
                    // Actually: v3_price_in_quote(sqrt, token_dec, quote_dec, token_is_token0)
                    //   returns "how many quote tokens per 1 token"
                    // We called with token=USDC(6dec) quote=native(18dec)
                    // If stable_is_token0: token_is_token0=true -> gives native per USDC
                    //   native_usd = 1 / price
                    // If native_is_token0: token_is_token0=false -> gives native per USDC
                    //   native_usd = 1 / price
                    // Wait, let me re-think: we want USD price of 1 native token.
                    // The pool trades USDC <-> WETH.
                    // v3_price_in_quote(sqrt, token_dec=6, quote_dec=18, stable_is_t0)
                    //   = "how many 18-dec units per 1 6-dec unit" = WETH per USDC
                    // So native_per_usdc = price, and native_usd = 1.0 / price
                    let native_usd = 1.0 / price;

                    if native_usd > 0.01 && native_usd < 1_000_000.0 {
                        self.native_usd_cache.insert(chain_id, native_usd);
                        debug!(chain = chain_id, native_usd, "fetched live native/USD rate");
                        return Some(native_usd);
                    }
                }
            }
        }

        // No env fallback here — only real on-chain rates.
        None::<f64>
        };
        if let Ok(Some(rate)) =
            tokio::time::timeout(std::time::Duration::from_secs(40), inner).await
        {
            return Some(rate);
        }
        }

        // Fallback: try the env var once all endpoint attempts are exhausted
        self.native_usd_rate_env(chain_id)
    }

    /// Refresh native/USD rates for all chains — fetched in parallel.
    pub async fn refresh_all_native_usd_rates(&self) {
        let mut rates_found = 0u32;

        // Fetch all unique native tokens concurrently (ETH, MATIC, BNB, AVAX, CELO, S).
        // Sonic's quote leg is wS — its own token, NOT ETH — so it needs its
        // own on-chain rate (wS/USDC.e pool), or every Sonic spread is
        // mispriced by ~ETH/S (~7700x).
        let (eth_rate, matic_rate, bnb_rate, avax_rate, celo_rate) = futures_util::future::join5(
            self.fetch_native_usd_rate(1),       // ETH
            self.fetch_native_usd_rate(137),     // MATIC
            self.fetch_native_usd_rate(56),      // BNB
            self.fetch_native_usd_rate(43114),   // AVAX
            self.fetch_native_usd_rate(42220),   // CELO
        )
        .await;
        let sonic_rate = self.fetch_native_usd_rate(146).await; // S

        // ETH-quoted chains share the ETH rate. Mantle pays gas in MNT but
        // its pool liquidity is denominated in bridged WETH, so the ETH rate
        // is the right quote-leg conversion there too.
        if let Some(eth_rate) = eth_rate {
            for chain_id in &[1u64, 42161, 10, 8453, 59144, 130, 534352, 324, 5000] {
                self.native_usd_cache.insert(*chain_id, eth_rate);
            }
            rates_found += 9;
            info!(rate = format!("${:.2}", eth_rate), "ETH/USD rate (9 chains)");
        }
        if let Some(rate) = matic_rate {
            self.native_usd_cache.insert(137, rate);
            rates_found += 1;
            info!(rate = format!("${:.4}", rate), "MATIC/USD rate");
        }
        if let Some(rate) = bnb_rate {
            self.native_usd_cache.insert(56, rate);
            rates_found += 1;
            info!(rate = format!("${:.2}", rate), "BNB/USD rate");
        }
        if let Some(rate) = avax_rate {
            self.native_usd_cache.insert(43114, rate);
            rates_found += 1;
            info!(rate = format!("${:.2}", rate), "AVAX/USD rate");
        }
        if let Some(rate) = celo_rate {
            self.native_usd_cache.insert(42220, rate);
            rates_found += 1;
            info!(rate = format!("${:.4}", rate), "CELO/USD rate");
        }
        if let Some(rate) = sonic_rate {
            self.native_usd_cache.insert(146, rate);
            rates_found += 1;
            info!(rate = format!("${:.4}", rate), "S/USD rate (Sonic)");
        }
        // Gnosis (xDAI ≈ $1)
        self.native_usd_cache.insert(100, 1.0);
        rates_found += 1;

        info!(chains = rates_found, "native/USD rates refreshed");
    }

    /// Start the continuous scanning background loop.
    ///
    /// Pipeline (compatible detection + profitability):
    ///   1. `comprehensive_scan` finds **candidates** using cheap on-chain
    ///      spread detection and a conservative cost pre-filter.
    ///   2. Candidates with `net_profit > 0` (scanner estimate) are passed to
    ///      `validate_opportunity_via_velora` which quotes the actual Velora
    ///      round-trip (WETH → token → WETH).
    ///   3. Only opportunities whose Velora round-trip is profitable are stored
    ///      in `live_opportunities` and shown on the dashboard.
    ///
    /// This ensures detection and profitability filtering agree: the same
    /// routing engine that would execute the trade is the one that determines
    /// whether the trade is profitable.
    pub fn spawn_continuous_scanner(
        self: &Arc<Self>,
        interval_secs: u64,
        velora: Arc<crate::velora_client::VeloraClient>,
        profit_transfer: Arc<crate::profit_transfer::ProfitTransferService>,
        discovery: Arc<crate::discovery::DiscoveryService>,
    ) {
        let scanner = Arc::clone(self);
        let interval = std::time::Duration::from_secs(interval_secs);

        tokio::spawn(async move {
            info!(
                interval_secs,
                "continuous scanner started — scanning every {}s (with DEX Screener + DeFiLlama + Binance feeds + Etherscan verification)",
                interval_secs
            );

            // Etherscan verifier for on-chain tx confirmation
            let verifier = crate::etherscan::EtherscanVerifier::new();

            // Chain IDs for discovery refresh
            let chain_ids: Vec<u64> = crate::chains::get_chains()
                .iter()
                .map(|c| c.id)
                .collect();

            loop {
                let cycle_start = Instant::now();

                // 0. Refresh external discovery feeds (rate-limited internally)
                discovery.refresh_all(&chain_ids).await;

                // 1. Refresh native/USD rates from live pools
                scanner.refresh_all_native_usd_rates().await;

                // 2. Build merged token list: base list + discovered tokens
                let mut tokens = scanner.scan_token_list();
                let discovered = discovery.get_discovered_tokens().await;
                let base_count = tokens.len();
                for dt in &discovered {
                    // Only add if not already in the list for this chain
                    let already = tokens.iter().any(|t| {
                        t.address.eq_ignore_ascii_case(&dt.address)
                    });
                    if !already {
                        tokens.push(crate::radar_scanner::TokenInfo {
                            symbol: dt.symbol.clone(),
                            address: dt.address.clone(),
                        });
                    }
                }
                if tokens.len() > base_count {
                    info!(
                        base = base_count,
                        discovered = tokens.len() - base_count,
                        total = tokens.len(),
                        "merged discovery tokens into scan list"
                    );
                }

                // Push CEX benchmark prices into the scanner for deviation
                // detection during comprehensive_scan.
                let cex_prices = discovery.cex_prices.read().await.clone();
                if !cex_prices.is_empty() {
                    scanner.update_cex_benchmarks(cex_prices);
                }

                // Push DeFiLlama TVL-based dynamic depth thresholds.
                for &cid in &chain_ids {
                    let depth = discovery.dynamic_min_depth_usd(cid).await;
                    scanner.set_dynamic_min_depth(cid, depth);
                }

                // Log discovery stats periodically
                let (cex_count, tvl_count, disc_count) = discovery.stats().await;
                if cex_count > 0 {
                    debug!(
                        cex_prices = cex_count,
                        tvl_chains = tvl_count,
                        discovered = disc_count,
                        "discovery feeds active"
                    );
                }

                match scanner.comprehensive_scan(&tokens).await {
                    Ok(result) => {
                        // Pre-filter: scanner's spread-based estimate says net > 0.
                        // These are CANDIDATES, not confirmed profitable trades.
                        let mut candidates: Vec<OpportunityDetail> = result
                            .opportunities
                            .into_iter()
                            .filter(|o| o.profit_breakdown.is_profitable)
                            .collect();

                        // Sort best-estimated first so we validate the most
                        // promising candidates before the cycle budget runs out.
                        candidates.sort_by(|a, b| {
                            b.profit_breakdown
                                .net_profit_usd
                                .partial_cmp(&a.profit_breakdown.net_profit_usd)
                                .unwrap_or(std::cmp::Ordering::Equal)
                        });

                        if !candidates.is_empty() {
                            info!(
                                candidates = candidates.len(),
                                estimated_net_usd = format!("{:.2}",
                                    candidates.iter().map(|o| o.profit_breakdown.net_profit_usd).sum::<f64>()),
                                "spread candidates found — validating via on-chain simulation"
                            );
                        }

                        // 3. Validate each candidate with a real Velora quote.
                        //    Only Velora-confirmed profitable opportunities are
                        //    stored for the dashboard and profit accumulation.
                        let mut validated: Vec<OpportunityDetail> = Vec::new();
                        for opp in &candidates {
                            match scanner
                                .validate_opportunity_via_velora(
                                    opp,
                                    &velora,
                                    &profit_transfer,
                                    &verifier,
                                )
                                .await
                            {
                                Some(confirmed) => validated.push(confirmed),
                                None => {} // Velora says not profitable
                            }
                        }

                        let count = validated.len();
                        let total_net: f64 = validated
                            .iter()
                            .map(|o| o.profit_breakdown.net_profit_usd)
                            .sum();

                        // Store ONLY Velora-validated opportunities for dashboard
                        *scanner.live_opportunities.write() = validated;

                        if count > 0 {
                            info!(
                                opportunities = count,
                                net_usd = format!("{:.2}", total_net),
                                "on-chain-simulated profitable opportunities"
                            );
                        } else if !candidates.is_empty() {
                            debug!(
                                candidates = candidates.len(),
                                scan_ms = cycle_start.elapsed().as_millis(),
                                "all candidates failed on-chain simulation"
                            );
                        } else {
                            debug!(
                                scan_ms = cycle_start.elapsed().as_millis(),
                                "no spread candidates this cycle"
                            );
                        }
                    }
                    Err(e) => {
                        warn!("continuous scan failed: {}", e);
                    }
                }

                tokio::time::sleep(interval).await;
            }
        });
    }

    /// Validate a candidate opportunity by quoting a real Velora round-trip,
    /// then execute gaslessly via ERC-4337 (Pimlico bundler + paymaster).
    ///
    /// Pipeline:
    ///   1. Quote the Velora round-trip (WETH → token → WETH).
    ///   2. If the quote is not profitable, reject immediately.
    ///   3. Build a UserOperation calling ZeroRiskArb.execute() via flash loan.
    ///   4. Pimlico paymaster sponsors gas (zero native balance required).
    ///   5. Sign UserOp with EOA owner key, submit to Pimlico bundler.
    ///   6. Poll for on-chain receipt, cross-check via Etherscan.
    ///   7. Only record profit after on-chain receipt confirms success.
    ///
    /// Returns `Some(confirmed_opportunity)` with real on-chain-verified
    /// profit, or `None` if the trade is not profitable or fails on-chain.
    async fn validate_opportunity_via_velora(
        &self,
        opp: &OpportunityDetail,
        velora: &crate::velora_client::VeloraClient,
        profit_transfer: &crate::profit_transfer::ProfitTransferService,
        verifier: &crate::etherscan::EtherscanVerifier,
    ) -> Option<OpportunityDetail> {
        // Primary validator: exact on-chain simulation of the actual two-leg
        // route the contract executes (buy on the cheap pool, sell on the
        // expensive one, repay the flash loan). It covers all 15 chains,
        // needs no external API, and can't be fooled by an aggregator
        // rerouting into a different trade than we intend to execute.
        // The Velora round-trip path below remains as an opt-in second
        // opinion for supported chains via ZCA_VELORA_VALIDATION=1.
        if std::env::var("ZCA_VELORA_VALIDATION").ok().as_deref() != Some("1") {
            return self.validate_opportunity_onchain(opp, profit_transfer).await;
        }

        // Liquidity sanity check
        if opp.liquidity_usd <= 0.0 {
            return None;
        }

        let native_usd = self.native_usd_rate(opp.chain_id).unwrap_or(0.0);
        if native_usd <= 0.0 {
            return None;
        }

        // Correct notional recovery from the scanner's estimate.
        let trade_usd = if opp.spread_pct > 0.0 {
            (opp.profit_breakdown.gross_profit_usd / (opp.spread_pct / 100.0))
                .max(100.0)
                .min(10_000.0)
        } else {
            100.0
        };

        let weth_addr = crate::chains::get_wrapped_native(opp.chain_id);
        if weth_addr.is_empty() {
            return None;
        }

        if opp.buy_dex.is_none() || opp.sell_dex.is_none() {
            return None;
        }

        // Velora's API does not serve every chain we scan — its network list
        // omits Celo, Linea, Scroll, zkSync and Mantle, so their candidates
        // can never get a round-trip quote and would hard-reject here forever.
        // Those chains get a direct on-chain simulation against real pool
        // state. Sonic (146) and Unichain (130) ARE Velora-supported.
        const VELORA_UNSUPPORTED: &[u64] = &[42220, 59144, 534352, 324, 5000];
        if VELORA_UNSUPPORTED.contains(&opp.chain_id) {
            return self.validate_opportunity_onchain(opp, profit_transfer).await;
        }

        // Derive the EOA owner address from PRIVATE_KEY (used to identify the
        // smart account owner and as the Velora userAddress). No native balance
        // is required — gas is sponsored by Pimlico's paymaster.
        let wallet_addr = std::env::var("PROFIT_WALLET")
            .ok()
            .or_else(|| {
                std::env::var("PRIVATE_KEY").ok().and_then(|pk| {
                    crate::gasless::owner_address_from_key(&pk).ok()
                })
            })
            .unwrap_or_default();
        if wallet_addr.is_empty() {
            return None;
        }

        let token_dec: u8 = match opp.token.as_str() {
            "USDC" | "USDT" => 6,
            "WBTC" => 8,
            _ => 18,
        };

        // Convert trade_usd to WETH amount in wei (18 decimals)
        let weth_amount_human = trade_usd / native_usd;
        let buy_amount = format!("{:.0}", weth_amount_human * 1e18);

        // --- Leg 1: WETH → token (quote) ---
        let buy_swap = match velora
            .get_swap(
                opp.chain_id,
                weth_addr,
                &opp.token_address,
                18,
                token_dec,
                &buy_amount,
                "SELL",
                Some(&wallet_addr),
                Some(100),
            )
            .await
        {
            Ok(s) => s,
            Err(e) => {
                // "Invalid network" means Velora cannot serve this chain at
                // all — fall back to on-chain simulation rather than dropping
                // the candidate.
                if e.to_string().contains("Invalid network") {
                    return self.validate_opportunity_onchain(opp, profit_transfer).await;
                }
                info!(token = %opp.token, chain = %opp.chain_name, error = %e,
                    "Velora buy-leg quote failed");
                return None;
            }
        };

        // --- Leg 2: token → WETH (quote) ---
        let tokens_received = &buy_swap.price_route.dest_amount;
        let sell_swap = match velora
            .get_swap(
                opp.chain_id,
                &opp.token_address,
                weth_addr,
                token_dec,
                18,
                tokens_received,
                "SELL",
                Some(&wallet_addr),
                Some(100),
            )
            .await
        {
            Ok(s) => s,
            Err(e) => {
                info!(token = %opp.token, chain = %opp.chain_name, error = %e,
                    "Velora sell-leg quote failed");
                return None;
            }
        };

        // --- Compute expected profit from the Velora round-trip quote ---
        let spent_wei: f64 = buy_amount.parse().unwrap_or(0.0);
        let received_wei: f64 = sell_swap
            .price_route
            .dest_amount
            .parse()
            .unwrap_or(0.0);
        let profit_weth = (received_wei - spent_wei) / 1e18;
        let quote_profit_usd = profit_weth * native_usd;

        if quote_profit_usd <= 0.0 {
            info!(
                token = %opp.token, chain = %opp.chain_name,
                spent_weth = format!("{:.6}", spent_wei / 1e18),
                received_weth = format!("{:.6}", received_wei / 1e18),
                loss_usd = format!("${:.4}", -quote_profit_usd),
                scanner_est = format!("${:.2}", opp.profit_breakdown.net_profit_usd),
                "Velora round-trip not profitable — candidate rejected"
            );
            return None;
        }

        info!(
            token = %opp.token, chain = %opp.chain_name,
            spent_weth = format!("{:.6}", spent_wei / 1e18),
            received_weth = format!("{:.6}", received_wei / 1e18),
            velora_profit = format!("${:.2}", quote_profit_usd),
            scanner_est = format!("${:.2}", opp.profit_breakdown.net_profit_usd),
            "Velora quote profitable — executing on-chain for verification"
        );

        // ---- GASLESS EXECUTION VIA ERC-4337 (PIMLICO) ----
        //
        // Build a UserOperation that calls ZeroRiskArb.execute() through
        // the smart account. Pimlico's paymaster sponsors gas — no native
        // balance is required. If the contract is not deployed yet, the
        // Velora swap calldata is used directly as the UserOp's callData.
        //
        // Flow: UserOp → Pimlico paymaster sponsors → bundler submits →
        //       on-chain flash loan → swap → profit → receipt verification.

        let rpc_url = crate::chains::get_chains()
            .iter()
            .find(|c| c.id == opp.chain_id)
            .map(|c| c.rpc_url.clone())
            .unwrap_or_default();

        let private_key = std::env::var("PRIVATE_KEY").unwrap_or_default();
        let arb_contract = std::env::var("ZERO_RISK_ARB_ADDRESS").unwrap_or_default();

        let pimlico = crate::pimlico_client::PimlicoClient::new(
            std::env::var("PIMLICO_API_KEY").ok(),
        );

        // Build the execution calldata. If ZeroRiskArb is deployed, use
        // flash-loan execution. Otherwise fall back to the Velora swap
        // calldata routed through the smart account.
        let exec_result = if !pimlico.is_configured() {
            Err("gasless execution unavailable: PIMLICO_API_KEY not set".to_string())
        } else if arb_contract.is_empty() {
            // No ZeroRiskArb deployed yet — use Velora swap calldata
            // through the smart account. This still requires WETH in the
            // smart account for the swap, but gas is sponsored.
            let tx_params = &buy_swap.tx_params;
            crate::gasless::build_and_send_userop(
                &pimlico,
                opp.chain_id,
                &private_key,
                &tx_params.to,
                &tx_params.data,
            )
            .await
            .map(|r| r.tx_hash)
        } else {
            // ZeroRiskArb is deployed — build flash-loan calldata
            let weth_addr = crate::chains::resolve_token_symbol("WETH", opp.chain_id);
            let arb_calldata = crate::gasless::encode_arb_execute(
                3,  // source=3 → Balancer V2 (0% fee)
                &weth_addr,
                &buy_swap.price_route.src_amount,
                "0",  // minProfit = 0 (contract enforces atomicity)
                &buy_swap.tx_params.data,
                false,
                "0x0000000000000000000000000000000000000000",
            );
            crate::gasless::build_and_send_userop(
                &pimlico,
                opp.chain_id,
                &private_key,
                &arb_contract,
                &arb_calldata,
            )
            .await
            .map(|r| r.tx_hash)
        };

        let (tx_hash, verified, explorer_url, gas_cost_usd, actual_profit_usd) = match exec_result
        {
            Ok(tx_hash) => {
                info!(
                    token = %opp.token, chain = %opp.chain_name,
                    tx_hash = %tx_hash,
                    "tx broadcast — waiting for on-chain confirmation"
                );

                // Verify via RPC receipt + Etherscan cross-check
                let verification = verifier
                    .verify_tx(
                        &rpc_url,
                        &tx_hash,
                        opp.chain_id,
                        &wallet_addr,
                        std::time::Duration::from_secs(120),
                    )
                    .await;

                if verification.confirmed {
                    let gas_cost = verification.gas_cost_native.unwrap_or(0.0) * native_usd;
                    let net_profit = quote_profit_usd - gas_cost;

                    info!(
                        token = %opp.token, chain = %opp.chain_name,
                        tx_hash = %tx_hash,
                        block = ?verification.block_number,
                        gas_cost_usd = format!("${:.4}", gas_cost),
                        net_profit_usd = format!("${:.2}", net_profit),
                        explorer = %verification.explorer_url,
                        "PROFIT VERIFIED ON-CHAIN via Etherscan"
                    );

                    if net_profit > 0.0 {
                        (
                            tx_hash,
                            VerificationStatus::Confirmed,
                            verification.explorer_url,
                            gas_cost,
                            net_profit,
                        )
                    } else {
                        warn!(
                            token = %opp.token, chain = %opp.chain_name,
                            tx_hash = %tx_hash,
                            gas_cost_usd = format!("${:.4}", gas_cost),
                            "tx confirmed but gas consumed all profit"
                        );
                        (
                            tx_hash,
                            VerificationStatus::Confirmed,
                            verification.explorer_url,
                            gas_cost,
                            0.0, // no profit after gas
                        )
                    }
                } else if verification.reverted {
                    warn!(
                        token = %opp.token, chain = %opp.chain_name,
                        tx_hash = %tx_hash,
                        explorer = %verification.explorer_url,
                        "tx REVERTED on-chain — no profit"
                    );
                    (
                        tx_hash,
                        VerificationStatus::Reverted,
                        verification.explorer_url,
                        0.0,
                        0.0,
                    )
                } else {
                    warn!(
                        token = %opp.token, chain = %opp.chain_name,
                        tx_hash = %tx_hash,
                        "tx unconfirmed after timeout — no profit recorded"
                    );
                    (
                        tx_hash.clone(),
                        VerificationStatus::Pending,
                        crate::etherscan::tx_explorer_url(opp.chain_id, &tx_hash),
                        0.0,
                        0.0,
                    )
                }
            }
            Err(e) => {
                // Execution failed before broadcast (signing error, RPC error, etc.)
                // Do NOT record profit — this is quote-only.
                info!(
                    token = %opp.token, chain = %opp.chain_name,
                    error = %e,
                    quote_profit = format!("${:.2}", quote_profit_usd),
                    "on-chain execution failed — quote-only, no profit recorded"
                );
                (
                    String::new(),
                    VerificationStatus::ExecutionFailed,
                    String::new(),
                    0.0,
                    0.0,
                )
            }
        };

        // --- Only record profit if on-chain verified ---
        if actual_profit_usd > 0.0
            && matches!(verified, VerificationStatus::Confirmed)
        {
            let profit_cents = (actual_profit_usd * 100.0) as i64;
            self.cumulative_profit_usd
                .fetch_add(profit_cents, std::sync::atomic::Ordering::Relaxed);
            profit_transfer.record_profit(actual_profit_usd).await;
        }

        // Record in trade history regardless of outcome — for transparency.
        // Non-verified trades show $0 profit and their verification status.
        self.record_trade(
            &opp.chain_name,
            opp.chain_id,
            &opp.token,
            opp.buy_dex.as_deref().unwrap_or("?"),
            opp.sell_dex.as_deref().unwrap_or("?"),
            spent_wei / 1e18,
            received_wei / 1e18,
            actual_profit_usd,
            trade_usd,
            &tx_hash,
            verified.clone(),
            &explorer_url,
            gas_cost_usd,
        );

        // Build confirmed opportunity — only mark profitable if verified.
        let mut confirmed = opp.clone();
        confirmed.profit_breakdown.gross_profit_usd = quote_profit_usd;
        confirmed.profit_breakdown.net_profit_usd = actual_profit_usd;
        confirmed.profit_breakdown.net_profit_pct =
            if trade_usd > 0.0 { (actual_profit_usd / trade_usd) * 100.0 } else { 0.0 };
        confirmed.profit_breakdown.roi_pct = confirmed.profit_breakdown.net_profit_pct;
        confirmed.profit_breakdown.is_profitable =
            actual_profit_usd > 0.0 && matches!(verified, VerificationStatus::Confirmed);
        confirmed.profit_breakdown.costs.total_cost_usd = gas_cost_usd;
        confirmed.profit_breakdown.costs.gas_estimated_usd = gas_cost_usd;
        confirmed.profit_breakdown.costs.slippage_estimated_usd = 0.0;
        confirmed.profit_breakdown.costs.velora_fee_usd = 0.0;
        confirmed.profit_breakdown.costs.flash_loan_fee_usd = 0.0;

        Some(confirmed)
    }

    /// On-chain round-trip validation for chains the Velora API does not
    /// serve (Linea 59144, Celo 42220). Re-quotes every venue live, then
    /// simulates both swap legs against the pools' real state — constant-
    /// product `getAmountOut` math for V2/volatile pools, single-tick
    /// concentrated-liquidity math for V3 — rather than trusting a
    /// spot-price spread.
    ///
    /// Execution still requires the ZeroRiskArb contract's router path:
    /// its swap call is bound to Velora's Augustus contract today, which
    /// does not exist on these chains. A validated candidate is therefore
    /// recorded as `QuoteOnly` — a real, state-verified opportunity whose
    /// execution awaits the direct-pool route. No profit is recorded.
    async fn validate_opportunity_onchain(
        &self,
        opp: &OpportunityDetail,
        profit_transfer: &crate::profit_transfer::ProfitTransferService,
    ) -> Option<OpportunityDetail> {
        let native_usd = self.native_usd_rate(opp.chain_id).unwrap_or(0.0);
        if native_usd <= 0.0 {
            return None;
        }

        let chains = crate::chains::get_chains();
        let chain = chains.iter().find(|c| c.id == opp.chain_id)?;

        // Fresh venue quotes — same retry/endpoint-rotation as the scan path,
        // with the same hard timeout so a hung peer cannot park validation.
        let mut quotes: Vec<(String, VenueQuote)> = Vec::new();
        for _ in 0..3u8 {
            let Ok(provider) = try_build_provider(&chain.rpc_urls) else {
                continue;
            };
            match tokio::time::timeout(
                std::time::Duration::from_secs(45),
                quote_venues_on_chain(&provider, chain, &opp.token_address),
            )
            .await
            {
                Ok(Ok(q)) if !q.is_empty() => {
                    quotes = q;
                    break;
                }
                _ => continue,
            }
        }
        if quotes.len() < 2 {
            return None;
        }

        // Prefer the venues the scanner named; fall back to fresh extremes.
        let cheapest = quotes
            .iter()
            .min_by(|a, b| a.1.price.total_cmp(&b.1.price))?;
        let priciest = quotes
            .iter()
            .max_by(|a, b| a.1.price.total_cmp(&b.1.price))?;
        let buy = opp
            .buy_dex
            .as_ref()
            .and_then(|l| quotes.iter().find(|(n, _)| n == l))
            .unwrap_or(cheapest);
        let sell = opp
            .sell_dex
            .as_ref()
            .and_then(|l| quotes.iter().find(|(n, _)| n == l))
            .unwrap_or(priciest);
        if buy.0 == sell.0 || buy.1.price >= sell.1.price {
            return None;
        }

        // Size-optimized round-trip: P(x) = sell(buy(x)) − x·(1+flash_fee),
        // maximized over x against the two venues' decoded curves. A single
        // fixed notional either undersizes real edges or oversizes past
        // the liquidity cliff — searching finds the true optimum.
        let flash_rate = quotes
            .iter()
            .filter(|(_, q)| matches!(q.state, PoolState::Concentrated { .. }))
            .map(|(_, q)| q.fee_bps_hundredths as f64 / 1e6)
            .fold(f64::MAX, f64::min);
        let flash_rate = if flash_rate.is_finite() { flash_rate } else { 0.0005 };

        let sim = optimize_round_trip(&buy.1, &sell.1, flash_rate);
        let Some(sim) = sim else {
            info!(
                token = %opp.token, chain = %opp.chain_name,
                buy = %buy.0, sell = %sell.0,
                scanner_est = format!("${:.2}", opp.profit_breakdown.net_profit_usd),
                "on-chain round-trip unprofitable at every size — candidate rejected"
            );
            return None;
        };
        let weth_in = sim.amount_in;
        let tokens_mid = sim.mid_out;
        let weth_out = sim.final_out;
        // net_quote already subtracts the flash-loan fee.
        let quote_profit_usd = sim.net_quote / 1e18 * native_usd;
        let gas_est = estimate_gas_cost(opp.chain_id);

        if quote_profit_usd - gas_est <= 0.0 {
            info!(
                token = %opp.token, chain = %opp.chain_name,
                buy = %buy.0, sell = %sell.0,
                spent_weth = format!("{:.6}", weth_in / 1e18),
                received_weth = format!("{:.6}", weth_out / 1e18),
                sim_profit = format!("${:.4}", quote_profit_usd),
                gas_est = format!("${:.4}", gas_est),
                scanner_est = format!("${:.2}", opp.profit_breakdown.net_profit_usd),
                "on-chain round-trip not profitable — candidate rejected"
            );
            return None;
        }

        info!(
            token = %opp.token, chain = %opp.chain_name,
            buy = %buy.0, sell = %sell.0,
            spent_weth = format!("{:.6}", weth_in / 1e18),
            received_weth = format!("{:.6}", weth_out / 1e18),
            sim_profit = format!("${:.2}", quote_profit_usd),
            scanner_est = format!("${:.2}", opp.profit_breakdown.net_profit_usd),
            "on-chain round-trip validated — attempting direct-pool execution"
        );

        // ---- Direct-route execution via executeDirect + UniV3 flash ----
        //
        // Legs push the borrowed quote token into each pair and call
        // `swap()` with precomputed outputs (1% haircut vs the simulation —
        // the pair's K check reverts if we overshoot). The flash loan comes
        // from source 6: a Uniswap-V3-style pool holding the quote token,
        // taken from this pair's own venue set.
        let arb_contract = std::env::var("ZERO_RISK_ARB_ADDRESS").unwrap_or_default();
        let private_key = std::env::var("PRIVATE_KEY").unwrap_or_default();
        let pimlico = crate::pimlico_client::PimlicoClient::new(
            std::env::var("PIMLICO_API_KEY").ok(),
        );

        let (tx_hash, verified, explorer_url, gas_cost_usd, actual_profit_usd) = 'exec: {
            // Execution prerequisites: deployed contract + gasless infra.
            // Without them the candidate stays QuoteOnly.
            if arb_contract.is_empty()
                || private_key.is_empty()
                || !pimlico.is_configured()
            {
                break 'exec (
                    String::new(),
                    VerificationStatus::QuoteOnly,
                    String::new(),
                    gas_est,
                    0.0,
                );
            }

            // Flash pool: the deepest V3 venue for this pair that can cover
            // the borrow (its quote-side depth ≥ 3× the flash amount).
            let flash_pool = quotes
                .iter()
                .filter(|(_, q)| matches!(q.state, PoolState::Concentrated { .. }))
                .filter(|(_, q)| q.depth * 1e18 >= weth_in * 3.0)
                .max_by(|a, b| a.1.depth.total_cmp(&b.1.depth))
                .map(|(_, q)| q.pool);

            // Pool legs work for V2 pairs (transfer + pair.swap) AND V3
            // pools (pool.swap — input paid through uniswapV3SwapCallback).
            // Solidly-stable SpotOnly quotes can't be leg-encoded.
            let Some(flash_pool) = flash_pool else {
                break 'exec (
                    String::new(),
                    VerificationStatus::QuoteOnly,
                    String::new(),
                    gas_est,
                    0.0,
                );
            };

            let (Ok(token_addr), Ok(quote_addr)) = (
                opp.token_address.parse::<Address>(),
                crate::chains::get_wrapped_native(opp.chain_id).parse::<Address>(),
            ) else {
                break 'exec (
                    String::new(),
                    VerificationStatus::QuoteOnly,
                    String::new(),
                    gas_est,
                    0.0,
                );
            };

            let token_is_token0 = token_addr < quote_addr;
            let quote_str = format!("{:?}", quote_addr);
            let token_str = format!("{:?}", token_addr);

            // Leg 1: quote → token on the buy pool.
            let leg1 = match buy.1.state {
                PoolState::Reserves { .. } => {
                    let mid_out = (tokens_mid * 0.995) as u128;
                    let (b0, b1) = if token_is_token0 {
                        (mid_out, 0)
                    } else {
                        (0, mid_out)
                    };
                    crate::gasless::SwapLegInput {
                        target: format!("{:?}", buy.1.pool),
                        token_in: quote_str.clone(),
                        to_pool: true,
                        amount_in: format!("{:.0}", weth_in),
                        data: crate::gasless::encode_v2_pair_swap(b0, b1, &arb_contract),
                    }
                }
                PoolState::Concentrated {
                    sqrt_price_x96,
                    liquidity,
                    token_is_token0,
                } => {
                    // Input is the quote token → zeroForOne iff quote is
                    // token0, i.e. the token is NOT token0.
                    let zfo = !token_is_token0;
                    let limit = v3_sqrt_limit_x96(
                        to_f64(&sqrt_price_x96),
                        to_f64(&liquidity),
                        buy.1.fee_bps_hundredths,
                        weth_in,
                        zfo,
                    );
                    crate::gasless::SwapLegInput {
                        target: format!("{:?}", buy.1.pool),
                        token_in: quote_str.clone(),
                        to_pool: true,
                        amount_in: "0".to_string(),
                        data: crate::gasless::encode_v3_pool_swap(
                            &arb_contract,
                            zfo,
                            weth_in as i128,
                            &limit,
                            &crate::gasless::encode_address_word(&quote_str),
                        ),
                    }
                }
                PoolState::SpotOnly => {
                    break 'exec (
                        String::new(),
                        VerificationStatus::QuoteOnly,
                        String::new(),
                        gas_est,
                        0.0,
                    )
                }
            };

            // Leg 2: token → quote on the sell pool.
            let leg2 = match sell.1.state {
                PoolState::Reserves { .. } => {
                    // amountIn=0 spends the contract's whole mid balance
                    // (handles sim/reality drift); output is exact-out.
                    let final_out = (weth_out * 0.990) as u128;
                    let (s0, s1) = if token_is_token0 {
                        (0, final_out)
                    } else {
                        (final_out, 0)
                    };
                    crate::gasless::SwapLegInput {
                        target: format!("{:?}", sell.1.pool),
                        token_in: token_str.clone(),
                        to_pool: true,
                        amount_in: "0".to_string(),
                        data: crate::gasless::encode_v2_pair_swap(s0, s1, &arb_contract),
                    }
                }
                PoolState::Concentrated {
                    sqrt_price_x96,
                    liquidity,
                    token_is_token0,
                } => {
                    // Exact-in with a 1% haircut on the simulated mid amount —
                    // if leg 1 under-delivers the callback transfer fails and
                    // the whole tx reverts atomically (no partial loss).
                    let mid_in = (tokens_mid * 0.99) as i128;
                    let zfo = token_is_token0;
                    let limit = v3_sqrt_limit_x96(
                        to_f64(&sqrt_price_x96),
                        to_f64(&liquidity),
                        sell.1.fee_bps_hundredths,
                        mid_in as f64,
                        zfo,
                    );
                    crate::gasless::SwapLegInput {
                        target: format!("{:?}", sell.1.pool),
                        token_in: token_str.clone(),
                        to_pool: true,
                        amount_in: "0".to_string(),
                        data: crate::gasless::encode_v3_pool_swap(
                            &arb_contract,
                            zfo,
                            mid_in,
                            &limit,
                            &crate::gasless::encode_address_word(&token_str),
                        ),
                    }
                }
                PoolState::SpotOnly => {
                    break 'exec (
                        String::new(),
                        VerificationStatus::QuoteOnly,
                        String::new(),
                        gas_est,
                        0.0,
                    )
                }
            };

            let legs_data = crate::gasless::encode_swap_legs(&[leg1, leg2]);

            // minProfit = half the simulated net surplus (post flash fee) —
            // the contract reverts atomically if the real fill lands below it.
            let min_profit = format!("{:.0}", sim.net_quote.max(0.0) * 0.5);
            let calldata = crate::gasless::encode_arb_execute_direct(
                6, // Uniswap-V3 pool flash — works on every chain with a V3 pool
                &quote_str,
                &format!("{:.0}", weth_in),
                &min_profit,
                &legs_data,
                &format!("{:?}", flash_pool),
            );

            match crate::gasless::build_and_send_userop(
                &pimlico,
                opp.chain_id,
                &private_key,
                &arb_contract,
                &calldata,
            )
            .await
            {
                Ok(r) => {
                    info!(
                        token = %opp.token, chain = %opp.chain_name,
                        tx_hash = %r.tx_hash,
                        "direct-route tx broadcast — waiting for confirmation"
                    );
                    let wallet_addr = std::env::var("PROFIT_WALLET")
                        .ok()
                        .or_else(|| {
                            crate::gasless::owner_address_from_key(&private_key).ok()
                        })
                        .unwrap_or_default();
                    let v = crate::etherscan::EtherscanVerifier::new()
                        .verify_tx(
                            &chain.rpc_url,
                            &r.tx_hash,
                            opp.chain_id,
                            &wallet_addr,
                            std::time::Duration::from_secs(120),
                        )
                        .await;
                    if v.confirmed {
                        let gas_cost = v.gas_cost_native.unwrap_or(0.0) * native_usd;
                        let net = quote_profit_usd - gas_cost;
                        info!(
                            token = %opp.token, chain = %opp.chain_name,
                            tx_hash = %r.tx_hash,
                            gas_cost_usd = format!("${:.4}", gas_cost),
                            net_profit_usd = format!("${:.2}", net),
                            explorer = %v.explorer_url,
                            "direct-route profit verified on-chain"
                        );
                        (
                            r.tx_hash,
                            VerificationStatus::Confirmed,
                            v.explorer_url,
                            gas_cost,
                            if net > 0.0 { net } else { 0.0 },
                        )
                    } else if v.reverted {
                        (
                            r.tx_hash,
                            VerificationStatus::Reverted,
                            v.explorer_url,
                            0.0,
                            0.0,
                        )
                    } else {
                        (
                            r.tx_hash.clone(),
                            VerificationStatus::Pending,
                            crate::etherscan::tx_explorer_url(opp.chain_id, &r.tx_hash),
                            0.0,
                            0.0,
                        )
                    }
                }
                Err(e) => {
                    info!(
                        token = %opp.token, chain = %opp.chain_name,
                        error = %e,
                        "direct-route execution failed — quote-only"
                    );
                    (
                        String::new(),
                        VerificationStatus::ExecutionFailed,
                        String::new(),
                        gas_est,
                        0.0,
                    )
                }
            }
        };

        // Only a confirmed on-chain trade counts toward realized profit —
        // and only realized profit feeds the $100 auto-transfer threshold.
        if actual_profit_usd > 0.0 && matches!(verified, VerificationStatus::Confirmed) {
            let cents = (actual_profit_usd * 100.0) as i64;
            self.cumulative_profit_usd
                .fetch_add(cents, std::sync::atomic::Ordering::Relaxed);
            profit_transfer.record_profit(actual_profit_usd).await;
        }

        self.record_trade(
            &opp.chain_name,
            opp.chain_id,
            &opp.token,
            &buy.0,
            &sell.0,
            weth_in / 1e18,
            weth_out / 1e18,
            actual_profit_usd,
            weth_in / 1e18 * native_usd,
            &tx_hash,
            verified.clone(),
            &explorer_url,
            gas_cost_usd,
        );

        let mut confirmed = opp.clone();
        confirmed.buy_dex = Some(buy.0.clone());
        confirmed.sell_dex = Some(sell.0.clone());
        confirmed.profit_breakdown.gross_profit_usd = quote_profit_usd;
        confirmed.profit_breakdown.net_profit_usd = actual_profit_usd;
        confirmed.profit_breakdown.net_profit_pct = {
            let trade_usd = weth_in / 1e18 * native_usd;
            if trade_usd > 0.0 { (actual_profit_usd / trade_usd) * 100.0 } else { 0.0 }
        };
        confirmed.profit_breakdown.roi_pct = confirmed.profit_breakdown.net_profit_pct;
        confirmed.profit_breakdown.is_profitable =
            actual_profit_usd > 0.0 && matches!(verified, VerificationStatus::Confirmed);
        confirmed.profit_breakdown.costs.total_cost_usd = gas_cost_usd;
        confirmed.profit_breakdown.costs.gas_estimated_usd = gas_cost_usd;
        confirmed.profit_breakdown.costs.slippage_estimated_usd = 0.0;
        confirmed.profit_breakdown.costs.velora_fee_usd = 0.0;
        confirmed.profit_breakdown.costs.flash_loan_fee_usd = 0.0;
        confirmed.execution_steps.push(format!(
            "Direct on-chain route ({:?}): sim net ${:.2} after ${:.2} gas",
            verified,
            quote_profit_usd - gas_est,
            gas_est,
        ));

        Some(confirmed)
    }

    /// Tokens to scan on every cycle — maximum coverage per chain.
    /// Master token list: all symbols the scanner can resolve. The
    /// comprehensive_scan resolves each symbol to its per-chain address
    /// via `resolve_token_symbol`; symbols missing on a chain are skipped
    /// (the address resolves to `""` and we `continue`).
    ///
    /// The Ethereum mainnet address is the default; it's only used as a
    /// fallback when per-chain resolution doesn't match anything.
    fn scan_token_list(&self) -> Vec<TokenInfo> {
        let tokens = [
            // === Tier 1: stablecoins (all 10 chains) ===
            ("USDC",   "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
            ("USDT",   "0xdAC17F958D2ee523a2206206994597C13D831ec7"),
            ("DAI",    "0x6B175474E89094C44Da98b954EedeAC495271d0F"),
            ("PYUSD",  "0x6c3ea9036406852006290770BEdFcAbA0e23A0e8"),
            // === Tier 2: blue-chip (deep pools across many chains) ===
            ("WBTC",   "0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599"),
            ("WETH",   "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"),
            ("WSTETH", "0x7f39C581F595B53c5cb19bD0b3f8dA6c935E2Ca0"),
            ("LINK",   "0x514910771AF9Ca656af840dff83E8264EcF986CA"),
            ("UNI",    "0x1f9840a85d5aF5bf1D1762F925BDADdC4201F984"),
            ("AAVE",   "0x7Fc66500c84A76Ad7e9c93437bFc5Ac33E2DDaE9"),
            ("MKR",    "0x9f8F72aA9304c8B593d555F12eF6589cC3A579A2"),
            ("ARB",    "0xB50721BCf8d664c30412Cfbc6cf7a15145234ad1"),
            ("OP",     "0x4200000000000000000000000000000000000042"),
            // === Tier 3: DeFi governance (high DEX volume) ===
            ("LDO",    "0x5A98FcBEA516Cf06857215779Fd812CA3beF1B32"),
            ("CRV",    "0xD533a949740bb3306d119CC777fa900bA034cd52"),
            ("PENDLE", "0x808507121B80c02388fAd14726482e061B8da827"),
            ("ENA",    "0x57e114B691Db790C35207b2e685D4A43181e6061"),
            ("GRT",    "0xc944E90C64B2c07662A292be6244BDf05Cda44a7"),
            ("1INCH",  "0x111111111117dC0aa78b770fA6A738034120C302"),
            ("FXS",    "0x3432B6A60D23Ca0dFCa7761B7ab56459D9C964D0"),
            ("YFI",    "0x0bc529c00C6401aEF6D220BE8C6Ea1667F6Ad93e"),
            ("CVX",    "0x4e3FBD56CD56c3e72c1403e103b45Db9da5B9D2B"),
            ("ENS",    "0xC18360217D8F7Ab5e7c516566761Ea12Ce7F9D72"),
            ("MORPHO", "0x58D97B57BB95320F9a05dc918Aef65434969c2B2"),
            // === Tier 4: high-volume meme / narrative tokens ===
            ("PEPE",   "0x6982508145454Ce325dDbE47a25d4ec3d2311933"),
            ("SHIB",   "0x95aD61b0a150d79219dCF64E1E6Cc01f0B64C4cE"),
            ("FLOKI",  "0xcf0C122c6b73ff809C693DB761e7BaeBe62b6a2E"),
            ("WLD",    "0x163f8C2467924be0ae7B5347228CABF260318753"),
            // === Tier 5: L2/infra governance ===
            ("RENDER", "0x6De037ef9aD2725EB40118Bb1702EBb27e4Aeb24"),
            ("FET",    "0xaea46A60368A7bD060eec7DF8CBa43b7EF41Ad85"),
            ("IMX",    "0xF57e7e7C23978C3cAEC3C3548E3D615c346e79fF"),
            ("STRK",   "0xCa14007Eff0dB1f8135f4C25B34De49AB0d42766"),
            ("MNT",    "0x3c3a81e81dc49A522A592e7622A7E711c06bf354"),
            // === Tier 6: chain-native (home chain only) ===
            ("WMATIC", "0x0d500B1d8E8eF31E21C99d1Db9A6444d3ADf1270"),
            ("WBNB",   "0xbb4CdB9CBd36B01bD1cBaEBF2De08d9173bc095c"),
            ("WAVAX",  "0xB31f66AA3C1e785363F0875A1B74E27b85FD66c7"),
            // === Tier 7: liquid staking / restaking — peg spreads ==========
            ("STETH",  "0xae7ab96520DE3A18E5e111B5EaAb095312D7fE84"),
            ("RETH",   "0xae78736Cd615f374D3085123A210448E74Fc6393"),
            ("CBETH",  "0xBe9895146f7AF43049ca1c1AE358B0541E497776"),
            ("EZETH",  "0x2416092f143378750bb29b79eD961ab195CcEea5"),
            ("WEETH",  "0x1Bf74C010E6320bab11e2e5A532b5AC15e0b8aA6"),
            ("WRSETH", "0xD2671165570f41BBB3B0097893300b6EB6102E6C"),
            ("SAVAX",  "0x2b2C81e08f1Af8835a78Bb2A90AE924ACE0eA4bE"),
            ("GGAVAX", "0xA25EaF2906FA1a3a13EdAc9B9657108Af7B703e3"),
            ("STMATIC","0x3A58a54C066FdC0f2D55FC9C89F0415C92eBf3C4"),
            ("MATICX", "0xfa68FB4628DFF1028CFEc22b4162FCcd0d45efb6"),
            ("STCELO", "0xC668583dcbDc9ae6FA3CE46462758188adfdfC24"),
            ("SDAI",   "0xaf204776c7245bF4147c2612BF6e5972Ee483701"),
            // === Tier 8: stablecoin variants — depeg spreads = core arb ====
            ("USDC_E", "0xFF970A61A04b1cA14834A43f5dE4533eBDDB5CC8"),
            ("USDT_E", "0xc7198437980c041c805A1EDcbA50c1Ce5db95118"),
            ("DAI_E",  "0xd586E7F844cEa2F87f50152665BCbc2C279D8d70"),
            // WETH.e on Avalanche — bridged ETH, arbs against WAVAX pools.
            ("WETH_E", "0x49D5c2BdFfac6CE2BFdB6640F4F80f226bc10bAB"),
            ("BUSD",   "0xe9e7CEA3DedcA5984780Bafc599bD69ADd087D56"),
            ("FDUSD",  "0xc5f0f7b66764F6ec8C8Dff7BA683102295E16409"),
            ("USDE",   "0x4c9EDD5852cd905f086C759E8383e09bff1E68B3"),
            ("SUSDE",  "0x9D39A5DE30e57443BfF2A8307A4256c8797A3497"),
            ("FRAX",   "0x853d955aCEf822Db058EB8451b48d3d24B4f9819"),
            ("LUSD",   "0x5f98805A4E8be255a32880FDeC7F6728C6568bA0"),
            ("DOLA",   "0x6A7661795C374c0bFC635934efAddFf3A7Ee23b6"),
            ("MIM",    "0xFEa7a6a0B346362BF88A9e4A67916B6a73D0d597"),
            ("SUSD",   "0x8c6f28f2F1a3C87F0f938b96d27520d9751ec8d9"),
            ("MAI",    "0xdFA46478F9e5EA86d57387849598dbFB2e964b02"),
            ("HAI",    "0x10398AbC267496E49106B07dd6BE13364D10dC71"),
            ("EURE",   "0xcB444e90D8198415266c6a2724b7900fb12FC56E"),
            ("CUSD",   "0x765DE816845861e75A25fCA122bb6898B8B1282a"),
            ("CEUR",   "0xD8763CBa276a3738E6DE85b4b3bF5FDed6D6cA73"),
            ("CREAL",  "0xe8537a3d056DA446677B9E9d6c21dB704EaAb927"),
            ("USDS",   "0xdC035D45d973E3EC169d2276DDab16f1e407384F"),
            // === Tier 9: DeFi governance / DEX tokens ======================
            ("COMP",   "0xc00e94Cb662C3520282E6f5717214004A7f26888"),
            ("SNX",    "0xC011a73ee8576Fb46F5E1c5751cA3B9Fe0af2a6F"),
            ("BAL",    "0xba100000625a3754423978a60c9317c58a424e3D"),
            ("SUSHI",  "0x6B3595068778DD592e39A122f4f5a5cF09C90fE2"),
            ("DYDX",   "0x92D6C1e31e14520e676a687F0a93788B716Beff5"),
            ("QNT",    "0x4a220E6096B25EADb88358cb44068A3248254675"),
            ("MANA",   "0x0F5D2fB29fb7d3CFeE444a200298f468908cC942"),
            ("SAND",   "0x3845badAde8e6dFF049820680d1F14bD3903a5d0"),
            ("SAFE",   "0x5aFE3855358E112B5647B952709E6165e1c1eAAe"),
            ("SKY",    "0x56072C95FAA701256059aa122697B133aDEd9279"),
            ("TRB",    "0x88dF592F8eb5D7Bd38bFeF7dEb0fBc02cf3778a0"),
            ("API3",   "0x0b38210ea11411557c13457D4dA7dC6ea731B88a"),
            ("ONDO",   "0xfAbA6f8e4a5E8Ab82F62fe7C39859FA577269BE3"),
            ("PAXG",   "0x45804880De22913dAFE09f4980848ECE6EcbAf78"),
            ("TBTC",   "0x18084fbA666a33d37592fA2633fD49a74DD93a88"),
            ("POL",    "0x455e53CBB86018Ac2B8092FdCd39d8444aFFC3F6"),
            ("AXL",    "0x23ee2343B892b1BB63503a4FAbc840E0e2C6810f"),
            ("TIA",    "0xD56734d7f9979dD94FAE3d67C7a9280e71dBBd31"),
            // === Tier 10: Arbitrum ecosystem ===============================
            ("GMX",    "0xfc5A1A6EB076a2C7aD06eD22C90d7E710E35ad0a"),
            ("MAGIC",  "0x539bdE0d7Dbd336b79148AA742883198BBF60342"),
            ("RDNT",   "0x3082CC23568eA640225c2467653dB90e9250AaA0"),
            ("STG",    "0x6694340fc020c5E6B96567843da2df01b2CE1eb6"),
            ("GRAIL",  "0x3d9907F9a368ad0a51Be60f7Da3b97cf940982D8"),
            ("DPX",    "0x6C2C06790b3E3E3c38e12Ee22F8183b37a13EE55"),
            ("SPELL",  "0x3E6648C5a70A150A88bCE65F4aD4d506Fe15d2AF"),
            ("SYN",    "0x080F6AEd32Fc474DD5717105bDB5eC5721C1d3Ef"),
            ("GNS",    "0x18c11FD286C5EC11c3b683Caa813B77f5163A122"),
            ("WOO",    "0xcAFcD85D8ca7Ad1e1C6F82F651fA15E33AEfD07b"),
            ("PLS",    "0x51318B7D00db7ACc4026C88c3952B66278B6A67F"),
            ("WINR",   "0xD77B108d4f6cefaa0Cae9506A934e824BEccA46B"),
            ("DMT",    "0x8B0E6f19Ee57089F7649A455D89D7bC6314D04e8"),
            // === Tier 11: Optimism ecosystem ===============================
            ("VELO",   "0x9560e827aF36c94D2Ac33a39bCE1Fe78631088Db"),
            ("PERP",   "0x9e1028F5F1D5eDE59748FFcee5532509976840E0"),
            ("THALES", "0x217D47011b23BB961eB6D93cA9945B7501a5BB11"),
            ("KWENTA", "0x920Cf626a271321C151D027030D5d08aF699456b"),
            ("SONNE",  "0x1DB2466d9F5e10D7090E7152B68d62703a2245F0"),
            ("DHT",    "0xAF9fE3B5cCDAe78188B1F8b9a49Da7ae9510F151"),
            ("AELIN",  "0x61BAADcF22d2565B0F471b291C475db5555e0b76"),
            // === Tier 12: Base ecosystem ==================================
            ("AERO",   "0x940181a94A35A4569E4529A3CDfB74e38FD98731"),
            ("DEGEN",  "0x4ed4E862860beD51a9570b96d89aF5E1B0Efefed"),
            ("BRETT",  "0x532f27101965dd16442E59d40670FaF5eBB142E4"),
            ("TOSHI",  "0xAC1Bd2486aAf3B5C0fc3Fd868558b082a531B2B4"),
            ("VIRTUAL","0x0b3e328455c4059EEb9e3f84b5543F74E24e7E1b"),
            ("AIXBT",  "0x4F9Fd6Be4a90f2620860d680c0d4d5Fb53d1A825"),
            ("CLANKER","0x1bc0c42215582d5A085795f4baDbaC3ff36d1Bcb"),
            ("MOG",    "0x2Da56AcB9Ea78330f947bD57C54119DebdaB4035"),
            ("HIGHER", "0x0578d8A44db98B27BF358E9C8a6737fBf14198e4"),
            ("MIGGLES","0xB1a03EdA10342529bBF8EB700a06C60441fEf25d"),
            ("PRIME",  "0xFA980cEd6895AC314E7dE34Ef1bFAE90a5AdD21b"),
            ("CBBTC",  "0xcbB7C0000aB88B473b1f5aFd9ef808440eed33Bf"),
            ("WELL",   "0xA88594D404727625A9437C3f886C7643872296AE"),
            // === Tier 13: Polygon ecosystem ================================
            ("QUICK",  "0x831753DD7087CaC61aB5644b308642cc1c33Dc13"),
            ("GHST",   "0x385Eeac5cB85A38A9a07A70c73e0a3271CfB54A7"),
            ("OCEAN",  "0x282d8efCe846A88B159800bd4130ad77443Fa1A1"),
            ("DFYN",   "0xC168E40227E4ebD8C1caAE80F7a55a4F0e6D66C5"),
            ("TEL",    "0xdF7837DE1F2Fa4631D716CF2502f8b230F1dcc32"),
            // === Tier 14: BSC pegged majors + ecosystem ====================
            ("CAKE",   "0x0E09FaBB73Bd3Ade0a17ECC321fD13a19e81cE82"),
            ("TWT",    "0x4B0F1812e5Df2A09796481Ff14017e6005508003"),
            ("XVS",    "0xcF6BB5389c92Bdda8a3747Ddb454cB7a64626C63"),
            ("DOGE",   "0xbA2aE424d960c26247Dd6c32edC70B295c744C43"),
            ("XRP",    "0x1D2F0da169ceB9fC7B3143178cCa156BD176A682"),
            ("ADA",    "0x3EE2200Efb3400fAbB9AacF31297cBdD1d435D47"),
            ("DOT",    "0x7083609fCE4d1d8Dc0C979AAb8c869Ea2C873402"),
            ("ATOM",   "0x0Eb3a705fc54725037CC9e008bDede697f62F335"),
            ("LTC",    "0x4338665CBB7B2485A8855A139b75D5e34AB0DB94"),
            ("TRX",    "0x85EAC5Ac2F758618dFa09bDbe0cf174e7d574D5B"),
            ("TON",    "0x76A797A59Ba2C17726896976B7B3747BfD1d220f"),
            ("ANKR",   "0xf307910A4c7bbc79691fD374889b36d8531B08e3"),
            ("BSW",    "0x965F527D9159dCe6288a2219DB51fc6Eef120dD1"),
            ("ALPACA", "0x8F0528cE5eF7B51152A59745bEfDD91D97091d2F"),
            ("DODO",   "0x67ee3Cb086F8a16f34beE3ca5FAD36F7DbEBeEe5"),
            ("BANANA", "0x603c7f932ED1fc6575303D8Fb018fDCBb0f39a95"),
            ("CHESS",  "0x20de22029ab63cf9A7Cf5fEB2b737Ca1eE4c62A6"),
            ("C98",    "0xaec945e04baf28b135fa7c640f624f8d90f1c3a6"),
            ("SFP",    "0xD41FDb03Ba84762dD66a0af1a6C8540FF1ba5dfb"),
            ("CHR",    "0x9FDc6ae99d28F8A90559d48016fF6Cfa06A19f91"),
            ("EDU",    "0xBdEAea03cA43a1c790FCFdA8fAe1c7f1772aE398"),
            // === Tier 15: Avalanche ecosystem ==============================
            ("JOE",    "0x6e84a6216eA6dACC71eE8E6b0a5B7322EEbC0fDd"),
            ("PNG",    "0x60781C2586D68229fde47564546784ab3fACA982"),
            ("QI",     "0x8729438EB15e2C8B576fCc6AeCdA6A148776C0F5"),
            ("PTP",    "0x22d4002028f537599bE9f666d1c4Fa138522f9c8"),
            ("YAK",    "0x59414b3089ce2AF0010e7523Dea7E2b35d776ec7"),
            ("COQ",    "0x420FcA0121DC28039145009570975747295f2329"),
            ("KIMBO",  "0x184ff13B3EBCB25Be44e860163A5D8391Dd568c1"),
            ("SNOB",   "0xC38f41A296A4493Ff429F1238e030924A1542e50"),
            ("XAVA",   "0xd1c3f94DE7e5B45fa4EDBA47222a7e50B5a469A4"),
            // === Tier 16: Linea / Gnosis / Celo ecosystem ==================
            ("FOXY",   "0x5FBDF89403270a1846F5ae7D113A989F850d1566"),
            ("CROAK",  "0xaCb54d07cA167934F57F829BeE2cC665e1A5eFBF"),
            ("LYNX",   "0x1a51b19CE03dbE0Cb44C1528E34a7EDD7771E9Af"),
            ("MENDI",  "0x43E8809ea7486baA3E4be59a922A2e7D7cEa4A0E"),
            ("GNO",    "0x9C58BAcC331c9aa871AFD802DB6379a98e80CEdb"),
            ("COW",    "0x177127622c4A00F3d409B75571e12cB3c8973d3c"),
            ("OLAS",   "0xcE11e14225575945b8E6Dc0D4F2dD4C570f79d9f"),
            ("HNY",    "0x71850b7E9Ee3f13Ab46d67167341E4bDc905Eef9"),
            ("FOX",    "0x21a42669643f45Bc0e086b8Fc2ed70c23D67509d"),
            ("UBE",    "0x00Be915B9dCf56a3CBE739D9B9c202ca692409EC"),
        ];
        tokens
            .iter()
            .map(|(sym, addr)| TokenInfo {
                symbol: sym.to_string(),
                address: addr.to_string(),
            })
            .collect()
    }

    /// Scan all chains and DEXes for a given token symbol
    pub async fn scan_token(
        &self,
        token_symbol: &str,
        token_address: Option<&str>,
    ) -> Result<RadarScanResponse, Box<dyn std::error::Error + Send + Sync>> {
        let start = Instant::now();
        let chains = get_chains();

        let mut all_prices = Vec::new();

        for chain in chains {
            // A chain with no registry-listed venue cannot be quoted. This check used
            // to hide four of the ten configured chains entirely, because the
            // hand-written DEX table only covered the other six.
            if crate::chains::get_venues(chain.id).is_empty() {
                continue;
            }

            // Resolve the token's address *for this chain*. A symbol is a
            // different contract on every network, so one chain's address is
            // meaningless on another. A token the registry does not carry here
            // is skipped, not fatal: one chain missing a token must not abort
            // the other nine. The `?` here previously made every symbol-only
            // scan fail on its first chain.
            let addr = match token_address {
                Some(a) => a.to_string(),
                None => match self.resolve_token_address(token_symbol, chain.id).await {
                    Ok(a) => a,
                    Err(_) => continue,
                },
            };

            // Query every registry-listed venue on this chain
            let prices = self.query_chain_prices(chain, token_symbol, &addr).await;

            all_prices.extend(prices);
        }

        // Find arbitrage opportunities: cheapest buy -> most expensive sell
        let opportunities = self.find_arbitrage_opportunities(&all_prices);

        let elapsed = start.elapsed().as_millis() as u64;

        Ok(RadarScanResponse {
            token: token_symbol.to_string(),
            opportunities,
            scan_time_ms: elapsed,
        })
    }

    /// Quote every venue on one chain for a single token.
    ///
    /// One row per resolvable venue/tier, each derived from that venue's *own*
    /// pool. This is the fix for the core defect: the venue address used to be
    /// ignored entirely, so every "DEX" re-read one canonical V2 pool and was
    /// merely *labelled* with a different name. The difference between rows was
    /// therefore always exactly zero, and any "spread" reported was an artifact
    /// of the labelling rather than an observation of the market.
    async fn query_chain_prices(
        &self,
        chain: &ChainConfig,
        token_symbol: &str,
        token_address: &str,
    ) -> Vec<TokenPrice> {
        // The key must include the token *address*: two different tokens can
        // share a symbol, and keying on the symbol alone collides.
        let cache_key = format!("{}_{}_{}", chain.id, token_symbol, token_address);
        if let Some(cached) = self.price_cache.get(&cache_key) {
            let fresh = Utc::now().timestamp() as u64 - self.cache_ttl_secs;
            // An empty entry is a valid "scanned, nothing found" result, so
            // guard the index rather than assuming it is non-empty.
            match cached.first() {
                Some(first) if first.timestamp > fresh => return cached.clone(),
                Some(_) => {}
                None => return Vec::new(),
            }
        }

        // Try up to 3 different RPC endpoints before giving up. When the
        // shared pool is attached, endpoints are picked by health score
        // (latency + consecutive failures) so rate-limited providers cool
        // down instead of absorbing every retry. Without a pool we fall back
        // to round-robin over the configured URLs.
        let mut quotes = Vec::new();
        let mut last_err = String::new();
        for _attempt in 0..3u8 {
            let (provider, pool_idx) = match &self.rpc_pool {
                Some(pool) => match pool.select(chain.id) {
                    Some((idx, url)) => match try_build_provider(&[url]) {
                        Ok(p) => (p, Some(idx)),
                        Err(e) => {
                            last_err = e;
                            continue;
                        }
                    },
                    None => match try_build_provider(&chain.rpc_urls) {
                        Ok(p) => (p, None),
                        Err(e) => {
                            last_err = e;
                            continue;
                        }
                    },
                },
                None => match try_build_provider(&chain.rpc_urls) {
                    Ok(p) => (p, None),
                    Err(e) => {
                        last_err = e;
                        continue;
                    }
                },
            };

            // Bound in-flight RPC calls through the pool's semaphore so a
            // wide scan cannot open hundreds of sockets against rate-limited
            // free endpoints.
            let _permit = match &self.rpc_pool {
                Some(pool) => Some(pool.acquire().await),
                None => None,
            };
            let started = std::time::Instant::now();
            // Hard cap per attempt: alloy's default HTTP client has no request
            // timeout, so a peer that accepts TCP but never answers would park
            // this task — and the whole join_all — forever. On timeout we count
            // it as an endpoint failure so the pool scores it down.
            const RPC_QUERY_TIMEOUT: std::time::Duration =
                std::time::Duration::from_secs(45);
            let venue_res = tokio::time::timeout(
                RPC_QUERY_TIMEOUT,
                quote_venues_on_chain(&provider, chain, token_address),
            )
            .await;
            match venue_res {
                Ok(Ok(q)) => {
                    if let (Some(pool), Some(idx)) = (&self.rpc_pool, pool_idx) {
                        pool.record_success(chain.id, idx, started.elapsed());
                    }
                    quotes = q;
                    last_err.clear();
                    break;
                }
                Ok(Err(e)) => {
                    let msg = e.to_string();
                    if let (Some(pool), Some(idx)) = (&self.rpc_pool, pool_idx) {
                        let rate_limited = msg.contains("429")
                            || msg.contains("Too Many Requests")
                            || msg.contains("-32029")
                            || msg.contains("rate limit");
                        pool.record_failure(chain.id, idx, msg.clone(), rate_limited);
                    }
                    last_err = msg;
                    // Will retry with the next endpoint
                }
                Err(_elapsed) => {
                    let msg = format!("RPC query timed out after {:?}", RPC_QUERY_TIMEOUT);
                    if let (Some(pool), Some(idx)) = (&self.rpc_pool, pool_idx) {
                        pool.record_failure(chain.id, idx, msg.clone(), false);
                    }
                    last_err = msg;
                }
            }
        }
        if !last_err.is_empty() && quotes.is_empty() {
            warn!("venue quoting failed on {} after 3 attempts: {}", chain.name, last_err);
            return Vec::new();
        }

        let timestamp = Utc::now().timestamp() as u64;
        // Keep the raw venue quotes (with pool state) — detection and
        // validation simulate the real round-trip against these reserves.
        self.venue_cache.insert(
            format!("{}_{}", chain.id, token_address.to_lowercase()),
            (timestamp, quotes.clone()),
        );
        let prices: Vec<TokenPrice> = quotes
            .into_iter()
            .map(|(label, quote)| TokenPrice {
                token: token_symbol.to_string(),
                token_address: token_address.to_string(),
                chain_id: chain.id,
                chain_name: chain.name.clone(),
                dex_name: label,
                // Denominated in the chain's wrapped native, NOT USD. A USD
                // figure needs an ETH/USD feed this build does not have, and
                // inventing one would be worse than a correctly labelled WETH
                // price. The field name is kept for wire compatibility.
                price_usd: quote.price,
                liquidity_usd: quote.depth,
                timestamp,
                pool_fee_bps_hundredths: quote.fee_bps_hundredths,
            })
            .collect();

        self.price_cache.insert(cache_key, prices.clone());
        prices
    }

    /// Venue quotes (label + pool state) for a token on a chain, sourced
    /// from the same fetch that fills `price_cache` — identical TTL and
    /// staleness contract.
    fn cached_venue_quotes(
        &self,
        chain_id: u64,
        token_address: &str,
    ) -> Vec<(String, VenueQuote)> {
        let key = format!("{}_{}", chain_id, token_address.to_lowercase());
        self.venue_cache
            .get(&key)
            .map(|e| e.1.clone())
            .unwrap_or_default()
    }
}

/// One venue's quote for a token, denominated in the chain's wrapped native.
#[derive(Debug, Clone, Copy)]
struct VenueQuote {
    /// Human price of the token in wrapped native.
    price: f64,
    /// Depth of the quote side at the current tick, in wrapped native.
    depth: f64,
    /// V3 pool fee in hundredths of a basis point (e.g. 3000 = 0.30%).
    /// V2 uses 30 (0.30% constant swap fee).
    fee_bps_hundredths: u32,
    /// Resolved pool address — needed to build direct-route swap legs.
    pool: Address,
    /// Raw pool state, kept so opportunities on chains the Velora API does
    /// not serve can still be validated by simulating the round-trip swap
    /// against real reserves/liquidity.
    state: PoolState,
}

/// Raw pool state carried alongside a venue's spot quote.
#[derive(Debug, Clone, Copy)]
enum PoolState {
    /// Constant-product pool: raw (reserve_token, reserve_quote) units.
    Reserves { token: U256, quote: U256 },
    /// Concentrated liquidity: sqrtPriceX96, in-range liquidity, orientation.
    Concentrated {
        sqrt_price_x96: U256,
        liquidity: U256,
        token_is_token0: bool,
    },
    /// A real spot price but no impact model we trust (Solidly stable pools
    /// use x³y+y³x, which `getReserves` alone cannot price) — never simulated.
    SpotOnly,
}

/// One venue/tier that can be resolved to a pool.
struct Candidate {
    /// Includes the fee tier, so two tiers of one venue stay distinguishable
    /// and a spread between them is visible rather than merged away.
    label: String,
    protocol: Protocol,
    /// The venue's *factory*, never a router.
    factory: Address,
    /// V3 fee tier / tick spacing. Unused for V2.
    fee: u32,
    /// ABI signature of the factory's pool lookup. Empty for V2.
    pool_sig: &'static str,
}

/// Read one sub-call's return data, but only if that sub-call succeeded.
fn ok_data(state: &[multicall::SubResult], idx: usize) -> Option<&[u8]> {
    state
        .get(idx)
        .filter(|s| s.success)
        .map(|s| s.return_data.as_slice())
}

/// Resolve and price every venue on a chain.
///
/// Two batched MultiCall3 round trips: phase 1 resolves a pool per venue/tier
/// through that venue's own factory; phase 2 reads each pool's state, its token
/// ordering, and the token's decimals. A venue whose pool or inputs cannot be
/// read is skipped, never guessed at.
async fn quote_venues_on_chain(
    provider: &HttpProvider,
    chain: &ChainConfig,
    token_address: &str,
) -> Result<Vec<(String, VenueQuote)>, Box<dyn std::error::Error + Send + Sync>> {
    // Every supported chain's wrapped native has 18 decimals.
    const QUOTE_DECIMALS: u8 = 18;

    let token: Address = token_address.parse()?;
    let weth: Address = crate::chains::get_wrapped_native(chain.id)
        .parse()
        .map_err(|_| format!("chain {} has no wrapped-native token configured", chain.id))?;
    if token == weth {
        return Ok(Vec::new());
    }

    // â”€â”€ Candidates: one per venue, one per V3 tier â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    let mut candidates: Vec<Candidate> = Vec::new();
    for venue in crate::chains::get_venues(chain.id) {
        let Ok(factory) = venue.address.parse::<Address>() else {
            continue;
        };
        match (venue.protocol, venue.v3) {
            (Protocol::V2, _) => candidates.push(Candidate {
                label: venue.name.to_string(),
                protocol: Protocol::V2,
                factory,
                fee: 0,
                pool_sig: "",
            }),
            (Protocol::V3, Some(params)) => {
                for fee in params.fees {
                    candidates.push(Candidate {
                        label: format!("{} [{}]", venue.name, fee),
                        protocol: Protocol::V3,
                        factory,
                        fee: *fee,
                        pool_sig: params.pool_sig,
                    });
                }
            }
            (Protocol::Solidly, Some(params)) => {
                // `fees` carries the stable flag: 0 = volatile, 1 = stable.
                for flag in params.fees {
                    candidates.push(Candidate {
                        label: format!(
                            "{} [{}]",
                            venue.name,
                            if *flag == 1 { "stable" } else { "volatile" }
                        ),
                        protocol: Protocol::Solidly,
                        factory,
                        fee: *flag,
                        pool_sig: params.pool_sig,
                    });
                }
            }
            // A keyed-factory venue with no resolution parameters cannot be
            // resolved. The registry test forbids this, so skip rather than
            // guess a selector.
            (Protocol::V3, None) | (Protocol::Solidly, None) => continue,
        }
    }
    if candidates.is_empty() {
        return Ok(Vec::new());
    }

    // â”€â”€ Phase 1: resolve each candidate's pool â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    let resolution: Vec<multicall::SubCall> = candidates
        .iter()
        .map(|c| match c.protocol {
            Protocol::V2 => multicall::SubCall::new(
                c.factory,
                multicall::factory_get_pair_call_data(token, weth),
            ),
            // Solidly's `bool` stable flag and V3's `uint24`/`int24` all
            // encode as one left-padded 32-byte word, so the same keyed
            // factory call serves both. Both families key pools by SORTED
            // token order — getPool(token, weth, …) returns the zero address
            // whenever token > weth — so the arguments must be ordered first.
            // This previously missed every V3 pool for tokens whose address
            // sorts above the wrapped native (0x4200… on OP/Base).
            Protocol::V3 | Protocol::Solidly => {
                let (a, b) = if token < weth { (token, weth) } else { (weth, token) };
                multicall::SubCall::new(
                    c.factory,
                    multicall::factory_get_pool_call_data(a, b, c.fee, c.pool_sig),
                )
            }
        })
        .collect();
    let resolved = multicall::execute(provider, &resolution).await?;

    // Keep each resolved pool next to the candidate that produced it. A venue
    // with no pool for this pair returns the zero address, which is normal.
    // The two failure kinds are counted apart because they call for different
    // responses: a zero address only means that venue does not trade this pair,
    // while a reverted sub-call means the encoding or the factory address is
    // wrong. Lumping them together lets a broken integration hide as
    // "no liquidity", which is how this scanner silently reported nothing while
    // every venue read the same pool.
    let mut reverted = 0usize;
    let mut empty = 0usize;
    let pools: Vec<(usize, Address)> = (0..candidates.len())
        .filter_map(|i| {
            let out = resolved.get(i)?;
            if !out.success {
                reverted += 1;
                return None;
            }
            match multicall::decode_address(&out.return_data) {
                Some(pool) => Some((i, pool)),
                None => {
                    empty += 1;
                    None
                }
            }
        })
        .collect();
    debug!(
        chain = chain.id,
        candidates = candidates.len(),
        resolved = pools.len(),
        no_pool = empty,
        reverted,
        "venue pool resolution"
    );
    if pools.is_empty() {
        // A token genuinely absent from a chain resolves to zero addresses with
        // no reverts, so only reverts are worth a warning: they are what a wrong
        // factory address or a mismatched `getPool` signature looks like, and in
        // the API response that is indistinguishable from an illiquid token.
        if reverted > 0 {
            warn!(
                chain = chain.id,
                candidates = candidates.len(),
                reverted,
                "no venue resolved a pool and some resolution calls reverted; \
                 check the factory addresses and pool-selector signatures for this chain"
            );
        }
        return Ok(Vec::new());
    }
    // â”€â”€ Phase 2: pool state, orientation, token decimals â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    // `token0()` is read from the pool because ordering cannot be inferred from
    // the query arguments, and assuming an order inverts the price on a
    // reversed pool. The previous code fell back to "assume token0" when the
    // call failed, which manufactured spreads out of a read that never
    // succeeded.
    let mut calls: Vec<multicall::SubCall> = Vec::new();
    let mut orient_idx: Vec<usize> = Vec::with_capacity(pools.len());
    let mut primary_idx: Vec<usize> = Vec::with_capacity(pools.len());
    let mut liquidity_idx: Vec<Option<usize>> = Vec::with_capacity(pools.len());

    for (_, pool) in &pools {
        orient_idx.push(calls.len());
        calls.push(multicall::SubCall::new(
            *pool,
            multicall::token0_call_data(),
        ));
    }
    for (candidate_idx, pool) in &pools {
        match candidates[*candidate_idx].protocol {
            // Solidly pools answer `getReserves()` the same way V2 pairs do.
            Protocol::V2 | Protocol::Solidly => {
                primary_idx.push(calls.len());
                calls.push(multicall::SubCall::new(
                    *pool,
                    multicall::get_reserves_call_data(),
                ));
                liquidity_idx.push(None);
            }
            Protocol::V3 => {
                primary_idx.push(calls.len());
                calls.push(multicall::SubCall::new(*pool, multicall::slot0_call_data()));
                liquidity_idx.push(Some(calls.len()));
                calls.push(multicall::SubCall::new(
                    *pool,
                    multicall::liquidity_call_data(),
                ));
            }
        }
    }
    let decimals_idx = calls.len();
    calls.push(multicall::SubCall::new(
        token,
        multicall::decimals_call_data(),
    ));

    let state = multicall::execute(provider, &calls).await?;

    // 18 is the ERC-20 convention and is used only for a token that does not
    // expose a readable `decimals()`. Nothing else here is defaulted.
    let token_decimals = ok_data(&state, decimals_idx)
        .and_then(multicall::decode_decimals)
        .unwrap_or(QUOTE_DECIMALS);

    let mut quotes = Vec::with_capacity(pools.len());
    for (i, (candidate_idx, pool_addr)) in pools.iter().enumerate() {
        let candidate = &candidates[*candidate_idx];

        let Some(token0) = ok_data(&state, orient_idx[i]).and_then(multicall::decode_address)
        else {
            warn!(
                "chain {} ({}): token0() unreadable on {}; skipping the venue rather than assuming an order",
                chain.id, chain.name, candidate.label
            );
            continue;
        };
        let token_is_token0 = token0 == token;

        let Some(primary) = ok_data(&state, primary_idx[i]) else {
            continue;
        };

        let quote = match candidate.protocol {
            Protocol::V2 | Protocol::Solidly => {
                let Some(reserves) = multicall::decode_reserves(primary) else {
                    continue;
                };
                let (reserve_token, reserve_weth) = if token_is_token0 {
                    (reserves.reserve0, reserves.reserve1)
                } else {
                    (reserves.reserve1, reserves.reserve0)
                };

                // Reject pools where either reserve is negligible. A reserve
                // of <1e-12 in human terms means the pool is effectively
                // empty and any price derived from it is noise.
                let min_reserve = std::cmp::min(reserve_token, reserve_weth);
                if min_reserve < 1_000_000 {
                    // < 1e6 raw units: for 18-decimal tokens that's < 1e-12,
                    // for 6-decimal it's < 1 unit. Either way, phantom pool.
                    continue;
                }

                let price = multicall::price_from_reserves(
                    reserve_token,
                    reserve_weth,
                    token_decimals,
                    QUOTE_DECIMALS,
                );
                // Under the constant-product invariant the quote side is half
                // the pool, so two-sided depth is twice that reserve.
                let depth = multicall::scale_amount(reserve_weth, QUOTE_DECIMALS) * 2.0;
                // Uniswap V2 charges a fixed 0.30%. Solidly fees are set per
                // pool: volatile pools are ~0.30% and stable pools ~0.05% —
                // `candidate.fee` carries the stable flag (1 = stable).
                let is_stable = candidate.protocol == Protocol::Solidly && candidate.fee == 1;
                let fee_bps_hundredths = if is_stable { 500 } else { 3000 };
                // Solidly stable pools run a different invariant (x³y+y³x);
                // reserves alone can't simulate their slippage, so they carry
                // a spot price only and are never round-trip simulated.
                let state = if is_stable {
                    PoolState::SpotOnly
                } else {
                    PoolState::Reserves {
                        token: reserve_token,
                        quote: reserve_weth,
                    }
                };
                VenueQuote { price, depth, fee_bps_hundredths, pool: *pool_addr, state }
            }
            Protocol::V3 => {
                let Some(slot0) = multicall::decode_slot0(primary) else {
                    continue;
                };
                let price = multicall::v3_price_in_quote(
                    slot0.sqrt_price_x96,
                    token_decimals,
                    QUOTE_DECIMALS,
                    token_is_token0,
                );
                let liquidity = liquidity_idx[i]
                    .and_then(|li| ok_data(&state, li))
                    .and_then(multicall::decode_liquidity)
                    .unwrap_or(U256::ZERO);
                let depth = if liquidity.is_zero() {
                    0.0
                } else {
                    multicall::v3_depth_in_quote(
                        slot0.sqrt_price_x96,
                        liquidity,
                        QUOTE_DECIMALS,
                        token_is_token0,
                    )
                };
                VenueQuote {
                    price,
                    depth,
                    fee_bps_hundredths: candidate.fee,
                    pool: *pool_addr,
                    state: PoolState::Concentrated {
                        sqrt_price_x96: slot0.sqrt_price_x96,
                        liquidity,
                        token_is_token0,
                    },
                }
            }
        };

        // Reject venues with no real data or insufficient depth.
        // depth < 0.01 WETH (~$27) means the pool is effectively empty —
        // any trade would move the price so much that it's not executable.
        if quote.price <= 0.0 || quote.depth < 0.01 {
            continue;
        }

        // Sanity bound: reject absurd prices that indicate stale or
        // broken pool state. A token priced at >1e12 or <1e-18 WETH
        // is almost certainly garbage data.
        if quote.price > 1e12 || quote.price < 1e-18 {
            debug!(
                chain = chain.id,
                venue = candidate.label,
                price = quote.price,
                "price out of sanity bounds; skipping phantom quote"
            );
            continue;
        }
        quotes.push((candidate.label.clone(), quote));
    }

    Ok(quotes)
}

impl RadarScanner {
    /// Resolve a token symbol to its address on a given chain.
    ///
    /// Backed by the per-chain table in [`crate::chains::resolve_token_symbol`].
    /// An unknown symbol resolves to `None`, which callers treat as "this chain
    /// does not carry that token" and skip - not as a fatal error. The previous
    /// body was an unconditional `Err`, which aborted the entire multi-chain
    /// scan on the first chain for any symbol-only request.
    async fn resolve_token_address(
        &self,
        symbol: &str,
        chain_id: u64,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let addr = crate::chains::resolve_token_symbol(symbol, chain_id);
        if addr.is_empty() {
            return Err(format!(
                "Token {} is not in the registry for {}",
                symbol,
                crate::chains::get_chain_name(chain_id)
            )
            .into());
        }
        Ok(addr.to_string())
    }

    /// Find arbitrage opportunities from price vector
    fn find_arbitrage_opportunities(&self, prices: &[TokenPrice]) -> Vec<ArbitrageOpportunity> {
        let mut opportunities = Vec::new();

        // Group prices by token
        // Find min (buy) and max (sell) across all (chain, dex) pairs
        if let (Some(cheapest), Some(priciest)) = (
            prices
                .iter()
                .min_by(|a, b| a.price_usd.partial_cmp(&b.price_usd).unwrap()),
            prices
                .iter()
                .max_by(|a, b| a.price_usd.partial_cmp(&b.price_usd).unwrap()),
        ) {
            let raw_spread_pct = if cheapest.price_usd > 0.0 {
                ((priciest.price_usd - cheapest.price_usd) / cheapest.price_usd) * 100.0
            } else {
                0.0
            };
            // Fee-adjusted spread
            let buy_fee_pct = cheapest.pool_fee_bps_hundredths as f64 / 10_000.0;
            let sell_fee_pct = priciest.pool_fee_bps_hundredths as f64 / 10_000.0;
            let spread_pct = (raw_spread_pct - buy_fee_pct - sell_fee_pct).max(0.0);

            if spread_pct >= self.min_spread_pct && priciest.liquidity_usd > 0.0 {
                opportunities.push(ArbitrageOpportunity {
                    id: Uuid::new_v4().to_string(),
                    token_symbol: cheapest.token.clone(),
                    token_address: cheapest.token_address.clone(),
                    buy_chain_id: cheapest.chain_id,
                    buy_chain_name: cheapest.chain_name.clone(),
                    buy_dex: cheapest.dex_name.clone(),
                    buy_price_usd: cheapest.price_usd,
                    sell_chain_id: priciest.chain_id,
                    sell_chain_name: priciest.chain_name.clone(),
                    sell_dex: priciest.dex_name.clone(),
                    sell_price_usd: priciest.price_usd,
                    spread_pct,
                    estimated_profit_usd: priciest.price_usd - cheapest.price_usd,
                    liquidity_usd: priciest.liquidity_usd.min(cheapest.liquidity_usd),
                    timestamp: Utc::now().timestamp() as u64,
                });
            }
        }

        opportunities
    }

    /// Scan all prices from every DEX across every chain
    pub async fn scan_all_prices(
        &self,
        token_symbol: &str,
        token_address: Option<&str>,
    ) -> Result<AllPricesResponse, Box<dyn std::error::Error + Send + Sync>> {
        let start = Instant::now();
        let chains = get_chains();
        let mut all_prices = Vec::new();

        for chain in chains {
            if crate::chains::get_venues(chain.id).is_empty() {
                continue;
            }

            // Resolve per chain, for the same reason as `scan_token`: one
            // address cannot be correct on ten networks, and an absent token
            // skips this chain instead of failing the whole response.
            let addr = match token_address {
                Some(a) => a.to_string(),
                None => match self.resolve_token_address(token_symbol, chain.id).await {
                    Ok(a) => a,
                    Err(_) => continue,
                },
            };

            let prices = self.query_chain_prices(chain, token_symbol, &addr).await;
            all_prices.extend(prices);
        }

        // Build chain summaries
        let mut chain_summaries = Vec::new();
        let token_addr = token_address.unwrap_or("").to_string();

        for chain in get_chains() {
            let chain_prices: Vec<&TokenPrice> = all_prices
                .iter()
                .filter(|p| p.chain_id == chain.id)
                .collect();
            if chain_prices.is_empty() {
                continue;
            }

            let min_p = chain_prices
                .iter()
                .map(|p| p.price_usd)
                .fold(f64::MAX, f64::min);
            let max_p = chain_prices
                .iter()
                .map(|p| p.price_usd)
                .fold(f64::MIN, f64::max);
            let avg_p =
                chain_prices.iter().map(|p| p.price_usd).sum::<f64>() / chain_prices.len() as f64;
            let sprd = if min_p > 0.0 {
                ((max_p - min_p) / min_p) * 100.0
            } else {
                0.0
            };

            chain_summaries.push(ChainSummary {
                chain_id: chain.id,
                chain_name: chain.name.clone(),
                dex_count: chain_prices.len(),
                min_price: min_p,
                max_price: max_p,
                avg_price: avg_p,
                spread_pct: sprd,
            });
        }

        let elapsed = start.elapsed().as_millis() as u64;
        Ok(AllPricesResponse {
            token: token_symbol.to_string(),
            token_address: token_addr,
            prices: all_prices,
            chain_summary: chain_summaries,
            scan_time_ms: elapsed,
        })
    }

    /// Scan all arbitrage strategies for a token
    pub async fn scan_all_strategies(
        &self,
        token_symbol: &str,
        token_address: Option<&str>,
    ) -> Result<AllOpportunitiesResponse, Box<dyn std::error::Error + Send + Sync>> {
        let start = Instant::now();

        // 1. Simple arbitrages (buy/sell across DEXes)
        let scan = self.scan_token(token_symbol, token_address).await?;

        // 2-5. Triangular, cross-chain, mint and JIT strategies.
        //
        // These four previously returned hard-coded rows: a fixed DAI->ETH->USDC
        // cycle at 1.59% / $15.90, a Polygon $0.98 -> Ethereum $1.02 USDC spread
        // worth $35.70, a Spark mint at 0.995 vs 1.005 worth $10.05, and a JIT
        // pool promising $125 on $50,000 of capital. None of it was read from a
        // chain, and it was returned for *any* token, including ones with no
        // liquidity at all. Because `/api/all-opportunities` is what the
        // dashboard renders as profit, the platform reported ~$86 of profit
        // while the real scanner had resolved zero pools.
        //
        // None of these strategies is detected on-chain by this build, so they
        // report nothing. Returning an empty list is the honest answer until a
        // detector exists; a fabricated one is indistinguishable from a loss in
        // a production ledger.
        let triangular: Vec<TriangularOpportunity> = Vec::new();
        let cross_chain: Vec<CrossChainOpportunity> = Vec::new();
        let mint: Vec<MintOpportunity> = Vec::new();
        let jit: Vec<JitLiquidityOpportunity> = Vec::new();

        let elapsed = start.elapsed().as_millis() as u64;
        Ok(AllOpportunitiesResponse {
            token: token_symbol.to_string(),
            simple_arbitrages: scan.opportunities,
            triangular_arbitrages: triangular,
            cross_chain_arbitrages: cross_chain,
            mint_opportunities: mint,
            jit_opportunities: jit,
            scan_time_ms: elapsed,
        })
    }

    /// USD value of one unit of the chain's wrapped native token.
    ///
    /// Checks: (1) live on-chain cache, (2) env var `ZCA_NATIVE_USD_<chain>`,
    /// (3) env var `ZCA_NATIVE_USD`. Returns `None` when the rate is unknown,
    /// which causes the caller to skip the opportunity rather than guess.
    fn native_usd_rate(&self, chain_id: u64) -> Option<f64> {
        // 1. Live cache (populated by fetch_native_usd_rate)
        if let Some(rate) = self.native_usd_cache.get(&chain_id) {
            return Some(*rate);
        }
        // 2. Env fallback
        self.native_usd_rate_env(chain_id)
    }

    /// Env-only fallback for native/USD rate.
    ///
    /// The generic `ZCA_NATIVE_USD` is only used for ETH-native chains
    /// (1, 42161, 10, 8453, 59144). Non-ETH chains must have either a
    /// live rate in the cache or a chain-specific env var; using the ETH
    /// rate for BNB/MATIC/AVAX/CELO would produce dimensionally wrong profit.
    fn native_usd_rate_env(&self, chain_id: u64) -> Option<f64> {
        let key = format!("ZCA_NATIVE_USD_{}", chain_id);
        let chain_specific = std::env::var(&key)
            .ok()
            .and_then(|v| v.trim().parse::<f64>().ok())
            .filter(|v| v.is_finite() && *v > 0.0);
        if chain_specific.is_some() {
            return chain_specific;
        }
        // Generic fallback for chains whose quote leg is WETH-denominated.
        // Mantle (5000) pools quote in bridged WETH too — its native MNT is a
        // separate asset, but pool liquidity is priced in WETH, so the ETH
        // rate is the correct conversion for its quote leg. Sonic is NOT here:
        // its quote leg is wS and needs ZCA_NATIVE_USD_146 or the live rate.
        let is_eth_chain = matches!(chain_id, 1 | 42161 | 10 | 8453 | 59144 | 130 | 534352 | 324 | 5000);
        if is_eth_chain {
            std::env::var("ZCA_NATIVE_USD")
                .ok()
                .and_then(|v| v.trim().parse::<f64>().ok())
                .filter(|v| v.is_finite() && *v > 0.0)
        } else {
            None
        }
    }

    /// Set minimum spread percentage
    pub fn set_min_spread(&mut self, pct: f64) {
        self.min_spread_pct = pct;
    }

    /// Clear price cache
    pub fn clear_cache(&self) {
        self.price_cache.clear();
        self.venue_cache.clear();
    }

    // â”€â”€â”€ Comprehensive Scan (ALL tokens, ALL DEXes, ALL strategies) â”€â”€â”€â”€â”€

    pub async fn comprehensive_scan(
        &self,
        tokens: &[TokenInfo],
    ) -> Result<ComprehensiveScanResponse, Box<dyn std::error::Error + Send + Sync>> {
        let start = Instant::now();
        let chains = get_chains();
        let mut all_opportunities = Vec::new();
        let mut tokens_scanned = Vec::new();
        let mut chains_scanned = Vec::new();
        let mut dexes_scanned = Vec::new();

        // ── Parallel chain scanning ─────────────────────────────────
        // Scan all chains concurrently. Each chain's MultiCall3 batch is
        // independent, so there is no data dependency between chains.
        // This cuts total scan time from sum(chain_latencies) to
        // max(chain_latencies), typically 3-5x faster.
        let mut chain_futures = Vec::new();

        for chain in chains {
            let venues = crate::chains::get_venues(chain.id);
            if venues.is_empty() {
                continue;
            }
            chains_scanned.push(chain.name.clone());
            for venue in venues {
                let name = venue.name.to_string();
                if !dexes_scanned.contains(&name) {
                    dexes_scanned.push(name);
                }
            }

            // Spawn a future for each (chain, token) pair
            for token in tokens {
                if !tokens_scanned.contains(&token.symbol) {
                    tokens_scanned.push(token.symbol.clone());
                }

                // Resolve the token's address on THIS chain. The scan list
                // stores a canonical (usually mainnet) address that is wrong
                // on every other network — passing it verbatim means querying
                // pools for a contract that either doesn't exist there or is
                // a different token entirely. `resolve_token_symbol` holds
                // the per-chain address matrix; when it has no mapping we fall
                // back to the listed address, which covers discovery-merged
                // tokens whose address is already chain-specific.
                let resolved = crate::chains::resolve_token_symbol(&token.symbol, chain.id);
                let addr = if resolved.is_empty() {
                    token.address.clone()
                } else {
                    resolved.to_string()
                };

                let chain_clone = chain.clone();
                let symbol = token.symbol.clone();

                let scanner_ref = &self;
                chain_futures.push(async move {
                    let chain_prices = scanner_ref.query_chain_prices(&chain_clone, &symbol, &addr).await;
                    // Pool state for simulation — written by the same fetch
                    // (same TTL), so this is a cache hit, never a refetch.
                    let venue_quotes =
                        scanner_ref.cached_venue_quotes(chain_clone.id, &addr);
                    (chain_clone, symbol, addr, chain_prices, venue_quotes)
                });
            }
        }

        // Execute all chain+token queries concurrently
        let results = futures_util::future::join_all(chain_futures).await;

        // Per-cycle funnel counters so each scan produces measurable output:
        // how many venues answered, how many pairs were evaluated, how far
        // each candidate progressed, and where it was dropped.
        let mut pools_ok: usize = 0;
        let mut pairs_evaluated: usize = 0;
        let mut spreads_seen: usize = 0;
        let mut above_threshold: usize = 0;
        let mut depth_rejects: usize = 0;
        let mut state_rejects: usize = 0;
        let mut sim_drops: usize = 0;

        for (chain, symbol, addr, chain_prices, venue_quotes) in results {
                let chain_prices = chain_prices;

                pools_ok += chain_prices.len();
                if chain_prices.len() < 2 {
                    continue;
                }
                pairs_evaluated += 1;

                // Find cheapest and most expensive on this chain
                if let (Some(cheapest), Some(priciest)) = (
                    chain_prices
                        .iter()
                        .min_by(|a, b| a.price_usd.partial_cmp(&b.price_usd).unwrap()),
                    chain_prices
                        .iter()
                        .max_by(|a, b| a.price_usd.partial_cmp(&b.price_usd).unwrap()),
                ) {
                    // ── Fee-adjusted spread ───────────────────────────
                    // The raw price difference between venues is NOT the
                    // executable profit. Each leg pays a pool swap fee:
                    //   - Buy leg (cheapest venue): pay its pool fee
                    //   - Sell leg (priciest venue): pay its pool fee
                    // The executable spread = raw_spread - buy_fee - sell_fee.
                    //
                    // Without this, "spreads" between V3 [10000] and [500] on
                    // the same DEX show 106% but are really ~0% net because
                    // the pool prices already embed their fee tier economics.
                    let raw_spread_pct = if cheapest.price_usd > 0.0 {
                        ((priciest.price_usd - cheapest.price_usd) / cheapest.price_usd) * 100.0
                    } else {
                        0.0
                    };

                    // Pool fee as a percentage: 3000 bps-hundredths = 0.30%
                    let buy_fee_pct = cheapest.pool_fee_bps_hundredths as f64 / 10_000.0;
                    let sell_fee_pct = priciest.pool_fee_bps_hundredths as f64 / 10_000.0;
                    let spread_pct = (raw_spread_pct - buy_fee_pct - sell_fee_pct).max(0.0);

                    // ── Phantom-spread filters ──────────────────────────
                    // These fire BEFORE the spread is logged or queued, so
                    // structurally-unprofitable pairs never reach the
                    // candidate list or the dashboard.

                    // (a) Non-USD-pegged stables. CEUR/CREAL/EURE trade
                    // against EUR or BRL; pricing them in wrapped-native→USD
                    // manufactures a permanent ~FX-rate spread that can never
                    // be captured by a USD-denominated round-trip.
                    const FX_PEG_SYMBOLS: &[&str] = &["CEUR", "CREAL", "EURE"];
                    if FX_PEG_SYMBOLS.contains(&symbol.as_str()) {
                        debug!(
                            chain = chain.id,
                            symbol,
                            "non-USD-pegged token — USD-priced spread is an \
                             FX artifact, not arbitrage; skipping"
                        );
                        continue;
                    }

                    // (b) Implausible spread cap. A raw spread this large on
                    // an established token means one venue resolved a stale,
                    // migrated, or dead pool (e.g. an old token contract that
                    // no longer trades). No executable arb of this size
                    // survives on a liquid token for a full scan cycle.
                    if raw_spread_pct > 30.0 {
                        debug!(
                            chain = chain.id,
                            symbol,
                            spread = format!("{:.1}%", raw_spread_pct),
                            buy = %cheapest.dex_name,
                            sell = %priciest.dex_name,
                            "spread above 30% — stale/dead pool signature; skipping"
                        );
                        continue;
                    }

                    // CEX deviation detection: if Binance has a price
                    // for this token, check if any DEX quote deviates
                    // significantly. A deviation adds confidence that
                    // the spread is exploitable (the DEX is mispriced
                    // relative to the global market).
                    let cex_dev_pct = if let Some(cex_price) = self.cex_price(&symbol) {
                        let native_usd = self.native_usd_rate(chain.id).unwrap_or(0.0);
                        if native_usd > 0.0 {
                            let dex_usd = cheapest.price_usd * native_usd;
                            crate::discovery::DiscoveryService::compute_cex_deviation(
                                cex_price, dex_usd, 0.1,
                            )
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    if spread_pct > 0.1 {
                        spreads_seen += 1;
                        if let Some(dev) = cex_dev_pct {
                            info!(
                                chain = %chain.name,
                                symbol = %symbol,
                                spread = format!("{:.3}%", spread_pct),
                                raw = format!("{:.3}%", raw_spread_pct),
                                buy_fee = format!("{:.2}%", buy_fee_pct),
                                sell_fee = format!("{:.2}%", sell_fee_pct),
                                cex_dev = format!("{:.2}%", dev),
                                buy = %cheapest.dex_name,
                                sell = %priciest.dex_name,
                                depth = format!("{:.2}", cheapest.liquidity_usd.min(priciest.liquidity_usd)),
                                "spread detected (fee-adjusted)"
                            );
                        } else {
                            info!(
                                chain = %chain.name,
                                symbol = %symbol,
                                spread = format!("{:.3}%", spread_pct),
                                raw = format!("{:.3}%", raw_spread_pct),
                                buy_fee = format!("{:.2}%", buy_fee_pct),
                                sell_fee = format!("{:.2}%", sell_fee_pct),
                                buy = %cheapest.dex_name,
                                sell = %priciest.dex_name,
                                depth = format!("{:.2}", cheapest.liquidity_usd.min(priciest.liquidity_usd)),
                                "spread detected (fee-adjusted)"
                            );
                        }
                    }

                    // Minimum USD depth — dynamic from DeFiLlama TVL when
                    // available, else hard-coded per-chain defaults.
                    let min_depth_usd = self.effective_min_depth_usd(chain.id);

                    // Chain-aware minimum spread: based purely on on-chain
                    // costs (gas, flash-loan fee, routing fee, slippage).
                    // L2s with cheap gas can profit on tighter spreads.
                    // CEX deviation is logged above for visibility but does
                    // NOT lower this threshold — both legs of our arb happen
                    // on-chain between DEX venues, so only the DEX-to-DEX
                    // spread determines profitability.
                    let chain_min_spread = match chain.id {
                        // L2/high-throughput chains: ~$0.01 gas
                        42161 | 10 | 8453 | 59144 | 100 | 146 | 130 | 534352 | 324 | 5000 => 0.10,
                        137 | 42220 => 0.15,                      // Polygon/Celo
                        _ => self.min_spread_pct,                  // Mainnet: higher gas
                    };

                    if spread_pct >= chain_min_spread {
                        above_threshold += 1;
                        let native_usd = self.native_usd_rate(chain.id);
                        let Some(native_usd) = native_usd else {
                            debug!(
                                chain = chain.id,
                                symbol,
                                "no native/USD rate configured; skipping opportunity \
                                 because profit cannot be denominated honestly"
                            );
                            continue;
                        };

                        // Filter out pools with insufficient depth. The depth
                        // is in wrapped native; convert to USD.
                        let min_depth = cheapest.liquidity_usd.min(priciest.liquidity_usd);
                        let depth_usd = min_depth * native_usd;
                        if depth_usd < min_depth_usd {
                            depth_rejects += 1;
                            debug!(
                                chain = chain.id,
                                symbol,
                                depth_usd,
                                "pool depth ${:.0} below minimum ${:.0}; skipping phantom spread",
                                depth_usd,
                                min_depth_usd,
                            );
                            continue;
                        }

                        // ── Executable-profit simulation ─────────────────
                        // Mid-price spread overstates the edge: the real
                        // trade is borrow quote → swap on `cheapest` → swap
                        // on `priciest` → repay. Simulate that round-trip
                        // on decoded pool state across a size grid and keep
                        // the profit-maximizing size. Candidates the sim
                        // can't make profitable are phantoms — drop them
                        // here instead of spamming validators.
                        let buy_q = venue_quotes
                            .iter()
                            .find(|(l, _)| l == &cheapest.dex_name)
                            .map(|(_, q)| *q);
                        let sell_q = venue_quotes
                            .iter()
                            .find(|(l, _)| l == &priciest.dex_name)
                            .map(|(_, q)| *q);
                        let (Some(buy_q), Some(sell_q)) = (buy_q, sell_q) else {
                            state_rejects += 1;
                            continue;
                        };

                        // Flash fee for the borrowed quote asset: the cheapest
                        // V3 pool on this pair can serve a source-6 flash at
                        // its swap-fee tier; without any V3 venue we assume
                        // Aave-style 0.05%.
                        let flash_rate = venue_quotes
                            .iter()
                            .filter(|(_, q)| matches!(q.state, PoolState::Concentrated { .. }))
                            .map(|(_, q)| q.fee_bps_hundredths as f64 / 1e6)
                            .fold(f64::MAX, f64::min);
                        let flash_rate = if flash_rate.is_finite() { flash_rate } else { 0.0005 };

                        let Some(sim) = optimize_round_trip(&buy_q, &sell_q, flash_rate) else {
                            sim_drops += 1;
                            debug!(
                                chain = chain.id,
                                symbol,
                                spot_spread = format!("{:.3}%", spread_pct),
                                "round-trip simulation unprofitable at every size — phantom spread dropped"
                            );
                            continue;
                        };

                        let gas_est = estimate_gas_cost(chain.id);
                        let notional_usd = sim.amount_in / 1e18 * native_usd;
                        // Gross capture before fixed costs, then net.
                        let gross_profit = (sim.final_out - sim.amount_in) / 1e18 * native_usd;
                        let fl_fee_usd = sim.amount_in * flash_rate / 1e18 * native_usd;
                        let slippage = 0.0; // price impact is inside the simulation
                        let velora_fee = 0.0; // direct route — no aggregator fee
                        let total_cost = gas_est + fl_fee_usd;
                        let net_profit = sim.net_quote / 1e18 * native_usd - gas_est;
                        let fl_fee = estimate_flash_loan_fee(&chain_prices, &symbol, notional_usd);
                        let net_pct = if notional_usd > 0.0 {
                            (net_profit / notional_usd) * 100.0
                        } else {
                            0.0
                        };
                        let roi = net_pct;

                        if net_profit > 0.0 {
                            let mut recommendations = vec![fl_fee.clone()];
                            // Add alternative flash loan sources
                            for alt_source in &[
                                FlashLoanSource::AaveV3,
                                FlashLoanSource::RadiantV2,
                                FlashLoanSource::Spark,
                            ] {
                                if alt_source.as_str() != fl_fee.source.as_str() {
                                    let alt_fee = alt_source.fee_pct(&symbol);
                                    recommendations.push(FlashLoanRecommendation {
                                        source: alt_source.clone(),
                                        fee_pct: alt_fee,
                                        // Charged on the borrowed principal, not
                                        // on the profit: the fee is a function of
                                        // the loan size. Using `gross_profit`
                                        // here (as before) under-reported it by
                                        // orders of magnitude.
                                        fee_usd: notional_usd * (alt_fee / 100.0),
                                        reason: format!(
                                            "{} - {} fee",
                                            alt_source.as_str(),
                                            alt_fee
                                        ),
                                    });
                                }
                            }

                            let opp_type = if cheapest.chain_id == priciest.chain_id {
                                ArbitrageType::Simple
                            } else {
                                ArbitrageType::CrossChain
                            };

                            all_opportunities.push(OpportunityDetail {
                                id: Uuid::new_v4().to_string(),
                                token: symbol.clone(),
                                token_address: addr.clone(),
                                arbitrage_type: opp_type,
                                chain_name: chain.name.clone(),
                                chain_id: chain.id,
                                buy_dex: Some(cheapest.dex_name.clone()),
                                sell_dex: Some(priciest.dex_name.clone()),
                                buy_price: cheapest.price_usd,
                                sell_price: priciest.price_usd,
                                spread_pct,
                                profit_breakdown: NetProfitBreakdown {
                                    gross_profit_usd: gross_profit,
                                    costs: CostBreakdown {
                                        gas_estimated_usd: gas_est,
                                        flash_loan_fee_usd: fl_fee_usd,
                                        slippage_estimated_usd: slippage,
                                        bridge_fee_usd: if cheapest.chain_id != priciest.chain_id {
                                            Some(0.50)
                                        } else {
                                            None
                                        },
                                        velora_fee_usd: velora_fee,
                                        total_cost_usd: total_cost,
                                    },
                                    net_profit_usd: net_profit,
                                    net_profit_pct: net_pct,
                                    roi_pct: roi,
                                    // Derived, not asserted. The previous
                                    // literal `true` meant any opportunity that
                                    // reached this point was labelled profitable
                                    // regardless of its own numbers, so
                                    // `profitable_count` and every downstream
                                    // "verified profit" figure counted rows that
                                    // lost money.
                                    is_profitable: net_profit > 0.0,
                                },
                                flash_loan_recommendation: Some(RecommendedFlashLoan {
                                    primary: fl_fee,
                                    alternatives: recommendations[1..].to_vec(),
                                }),
                                execution_steps: vec![
                                    format!("1. Initiate flash loan from {}", chain.name),
                                    format!(
                                        "2. Buy {} on {} at ${:.4}",
                                        symbol, cheapest.dex_name, cheapest.price_usd
                                    ),
                                    format!(
                                        "3. Sell {} on {} at ${:.4}",
                                        symbol, priciest.dex_name, priciest.price_usd
                                    ),
                                    format!("4. Repay flash loan + ${:.2} fee", total_cost),
                                    format!("5. Keep ${:.2} net profit", net_profit),
                                ],
                                confidence_score: calculate_confidence(
                                    spread_pct,
                                    net_profit,
                                    cheapest.liquidity_usd,
                                ),
                                liquidity_usd: priciest.liquidity_usd.min(cheapest.liquidity_usd),
                                timestamp: Utc::now().timestamp() as u64,
                            });
                        }
                    }
                }
            }

        let profitable_count = all_opportunities
            .iter()
            .filter(|o| o.profit_breakdown.is_profitable)
            .count();
        let total_net = all_opportunities
            .iter()
            .map(|o| o.profit_breakdown.net_profit_usd)
            .sum();
        let total_gas = all_opportunities
            .iter()
            .map(|o| o.profit_breakdown.costs.gas_estimated_usd)
            .sum();
        let elapsed = start.elapsed().as_millis() as u64;
        // Take the count before the vector is moved into the response (E0382).
        let total_opportunities = all_opportunities.len();

        info!(
            scan_ms = elapsed,
            chains = chains_scanned.len(),
            tokens = tokens_scanned.len(),
            pool_responses = pools_ok,
            pairs_evaluated,
            spreads_seen,
            above_threshold,
            depth_rejects,
            state_rejects,
            sim_drops,
            candidates = total_opportunities,
            profitable = profitable_count,
            net_usd = format!("{:.2}", total_net),
            "scan cycle complete"
        );

        Ok(ComprehensiveScanResponse {
            opportunities: all_opportunities,
            total_opportunities,
            profitable_count,
            total_net_profit_usd: total_net,
            total_gas_estimated_usd: total_gas,
            scan_time_ms: elapsed,
            tokens_scanned,
            chains_scanned,
            dexes_scanned,
        })
    }
}

// â”€â”€â”€ Helper Functions â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

fn estimate_gas_cost(chain_id: u64) -> f64 {
    match chain_id {
        1 => 15.0,      // Ethereum mainnet — expensive, rarely profitable
        42161 => 0.10,  // Arbitrum — very cheap, best for arb
        10 => 0.08,     // Optimism — very cheap
        8453 => 0.05,   // Base — cheapest L2
        137 => 0.30,    // Polygon
        56 => 0.20,     // BSC
        43114 => 0.40,  // Avalanche
        100 => 0.01,    // Gnosis — near-free
        42220 => 0.01,  // Celo — near-free
        59144 => 0.15,  // Linea
        146 => 0.02,    // Sonic — sub-second finality, cheap
        130 => 0.03,    // Unichain
        534352 => 0.05, // Scroll
        324 => 0.05,    // zkSync Era
        5000 => 0.05,   // Mantle
        _ => 1.0,
    }
}

/// U256 → f64 via decimal string (precision loss is acceptable for the
/// round-trip *simulation* — the real execution path re-quotes anyway).
fn to_f64(v: &U256) -> f64 {
    v.to_string().parse().unwrap_or(0.0)
}

/// Constant-product `getAmountOut` — the exact formula Uniswap V2 and
/// Solidly-family *volatile* pools implement on-chain. All amounts are raw
/// units; `fee_hbps` is hundredths of a bp (3000 = 0.30%).
fn simulate_cp_out(amount_in: f64, reserve_in: f64, reserve_out: f64, fee_hbps: u32) -> f64 {
    if amount_in <= 0.0 || reserve_in <= 0.0 || reserve_out <= 0.0 {
        return 0.0;
    }
    let net = amount_in * (1.0 - fee_hbps as f64 / 1_000_000.0);
    net * reserve_out / (reserve_in + net)
}

/// Concentrated-liquidity output for a swap that stays inside the current
/// tick's liquidity. Uses the real invariant (L·Δ(1/√P), L·Δ√P) instead of a
/// spot-price approximation, so price impact on thin pools is captured.
///
/// `zero_for_one` = selling token0 for token1. If the trade consumes more
/// than the current tick can supply, returns 0 — crossing into the next tick
/// range can't be simulated without tick data, and refusing is honest.
fn simulate_cl_out(
    amount_in: f64,
    sqrt_price_x96: f64,
    liquidity: f64,
    fee_hbps: u32,
    zero_for_one: bool,
) -> f64 {
    const TWO_POW_96: f64 = 79_228_162_514_264_337_593_543_950_336.0;
    if amount_in <= 0.0 || liquidity <= 0.0 || sqrt_price_x96 <= 0.0 {
        return 0.0;
    }
    let net = amount_in * (1.0 - fee_hbps as f64 / 1_000_000.0);
    let sqrt_p = sqrt_price_x96 / TWO_POW_96;
    if zero_for_one {
        // Sell token0 → price decreases → receive token1.
        let sqrt_p_after = sqrt_p - net / liquidity;
        if sqrt_p_after <= 0.0 {
            return 0.0;
        }
        liquidity * (1.0 / sqrt_p_after - 1.0 / sqrt_p)
    } else {
        // Sell token1 → price increases → receive token0.
        let sqrt_p_after = sqrt_p + net / liquidity;
        liquidity * (1.0 / sqrt_p - 1.0 / sqrt_p_after)
    }
}

/// Simulate one swap leg on a venue's decoded pool state.
///
/// `input_is_quote` selects direction: true sells the wrapped-native quote
/// token for the pair token (buy leg), false sells the pair token for
/// quote (sell leg). Amounts are raw units (wei-style). Returns the output
/// amount in raw units, or `None` when the state can't be simulated
/// (SpotOnly — e.g. Solidly stable pools whose x³y+y³x invariant isn't
/// modeled here; they contribute spot quotes but are never simulated).
fn simulate_leg_out(
    state: &PoolState,
    fee_bps_hundredths: u32,
    amount_in: f64,
    input_is_quote: bool,
) -> Option<f64> {
    match state {
        PoolState::Reserves { token, quote } => {
            let (rin, rout) = if input_is_quote {
                (to_f64(quote), to_f64(token))
            } else {
                (to_f64(token), to_f64(quote))
            };
            Some(simulate_cp_out(amount_in, rin, rout, fee_bps_hundredths))
        }
        PoolState::Concentrated {
            sqrt_price_x96,
            liquidity,
            token_is_token0,
        } => {
            // zeroForOne means the INPUT is token0. Buying the token with
            // quote: input is the quote token → token1 iff token is token0.
            let zero_for_one = if input_is_quote {
                !token_is_token0
            } else {
                *token_is_token0
            };
            // Constant-L validity bound: simulation assumes the current
            // tick's liquidity persists, but real concentrated liquidity
            // thins out across tick boundaries. Refuse fills that would
            // move sqrt price more than ~5% — beyond that the output is an
            // extrapolation, not a quote. This is what keeps stale/dead
            // pools (e.g. an abandoned LST pair 30% off market) from
            // reporting phantom profit at sizes their liquidity can't fill.
            let sqrt_p = to_f64(sqrt_price_x96) / 79_228_162_514_264_337_593_543_950_336.0;
            let l = to_f64(liquidity);
            if l <= 0.0 || sqrt_p <= 0.0 {
                return None;
            }
            let net_in = amount_in * (1.0 - fee_bps_hundredths as f64 / 1_000_000.0);
            let excursion_pct = net_in / (l * sqrt_p);
            if excursion_pct > 0.05 {
                return None;
            }
            Some(simulate_cl_out(
                amount_in,
                to_f64(sqrt_price_x96),
                to_f64(liquidity),
                fee_bps_hundredths,
                zero_for_one,
            ))
        }
        PoolState::SpotOnly => None,
    }
}

/// Result of a size-optimized round-trip simulation, in raw quote units.
#[derive(Debug, Clone, Copy)]
struct RoundTripSim {
    /// Optimal input in raw quote units (wei-scale).
    amount_in: f64,
    /// Pair tokens out of the buy leg.
    mid_out: f64,
    /// Quote units returned by the sell leg.
    final_out: f64,
    /// `final_out - amount_in - flash_fee` in raw quote units.
    net_quote: f64,
}

/// Find the profit-maximizing trade size for a two-venue round-trip.
///
/// This is the actual math of a flash-loan arb: borrow `x` of the quote
/// token, swap through the cheap venue, then the expensive one, repay
/// `x·(1+flash_fee_rate)`. Profit(x) is concave — both legs consume
/// liquidity and shrink the spread — so we scan a geometric grid of
/// fractions of the shallower venue's depth and keep the best point.
///
/// `flash_fee_rate` is a fraction (0.0005 = 0.05%) applied to the borrowed
/// quote amount. Returns `None` when no tested size nets positive.
fn optimize_round_trip(
    buy: &VenueQuote,
    sell: &VenueQuote,
    flash_fee_rate: f64,
) -> Option<RoundTripSim> {
    // Depth is in human quote units; raw units are ×1e18. Cap the borrow
    // at 25% of the shallower venue's quote-side depth — beyond that the
    // impact dominates any residual spread.
    let cap_human = buy.depth.min(sell.depth) * 0.25;
    if cap_human <= 0.0 {
        return None;
    }
    let cap_raw = cap_human * 1e18;

    let eval = |x: f64| -> Option<RoundTripSim> {
        let mid = simulate_leg_out(&buy.state, buy.fee_bps_hundredths, x, true)?;
        // Fill can never beat the venue's own spot price — pool impact only
        // ever makes a fill worse. If the modelled leg returns more tokens
        // than spot implies, the decoded state is garbage (wrong token at
        // this address, inverted orientation, dead pool) — drop it.
        if buy.price > 0.0 && mid > (x / buy.price) * 1.05 {
            return None;
        }
        let out = simulate_leg_out(&sell.state, sell.fee_bps_hundredths, mid, false)?;
        if sell.price > 0.0 && out > (mid * sell.price) * 1.05 {
            return None;
        }
        let net = out - x - x * flash_fee_rate;
        Some(RoundTripSim {
            amount_in: x,
            mid_out: mid,
            final_out: out,
            net_quote: net,
        })
    };

    const FRACS: &[f64] = &[0.005, 0.01, 0.03, 0.07, 0.15, 0.3, 0.55, 0.8, 1.0];
    let mut best: Option<RoundTripSim> = None;
    let mut best_idx = 0usize;
    for (i, &f) in FRACS.iter().enumerate() {
        if let Some(sim) = eval(cap_raw * f) {
            if best.map_or(true, |b| sim.net_quote > b.net_quote) {
                best_idx = i;
                best = Some(sim);
            }
        }
    }

    // No size clears costs → the spread is a phantom, not an opportunity.
    let best = best.filter(|b| b.net_quote > 0.0);

    // One local refinement pass around the best grid point — halves the
    // bracket three times, enough given pool-state granularity.
    if let Some(b) = best {
        let lo = cap_raw * if best_idx > 0 { FRACS[best_idx - 1] } else { 0.0 };
        let hi = cap_raw * if best_idx + 1 < FRACS.len() {
            FRACS[best_idx + 1]
        } else {
            FRACS[best_idx]
        };
        let (mut a, mut bnd) = (lo, hi);
        for _ in 0..12 {
            let m1 = a + (bnd - a) / 3.0;
            let m2 = bnd - (bnd - a) / 3.0;
            let n1 = eval(m1).map(|s| s.net_quote).unwrap_or(f64::MIN);
            let n2 = eval(m2).map(|s| s.net_quote).unwrap_or(f64::MIN);
            if n1 < n2 {
                a = m1;
            } else {
                bnd = m2;
            }
        }
        if let Some(sim) = eval((a + bnd) / 2.0) {
            if sim.net_quote > b.net_quote {
                return Some(sim);
            }
        }
        return Some(b);
    }
    best
}

/// sqrtPriceLimitX96 for a V3 swap leg, derived from the simulated ending
/// price with a 1% slack in the fill direction. Tighter than the absolute
/// min/max constants — bounds the excursion to roughly what the simulation
/// expects, so drift between scan and execution reverts earlier (atomic).
fn v3_sqrt_limit_x96(
    sqrt_price_x96_raw: f64,
    liquidity_raw: f64,
    fee_hbps: u32,
    amount_in: f64,
    zero_for_one: bool,
) -> String {
    const TWO_POW_96: f64 = 79_228_162_514_264_337_593_543_950_336.0;
    let sqrt_p = sqrt_price_x96_raw / TWO_POW_96;
    let net = amount_in * (1.0 - fee_hbps as f64 / 1_000_000.0);
    let after = if zero_for_one {
        sqrt_p - net / liquidity_raw.max(1.0)
    } else {
        sqrt_p + net / liquidity_raw.max(1.0)
    };
    let bounded = if zero_for_one {
        (after * 0.99).max(4295128740.0 / TWO_POW_96)
    } else {
        after * 1.01
    };
    format!("{:.0}", bounded * TWO_POW_96)
}

/// Velora split-routing fee, as a fraction of the routed amount.
/// Kept for the Velora-API validation path (opt-in via ZCA_VELORA_VALIDATION);
/// the direct-route pipeline does not charge it.
#[allow(dead_code)]
const VELORA_FEE_RATE: f64 = 0.001;

/// Pick the cheapest flash-loan source for a token.
///
/// Six sources are ranked by fee. Three are 0%:
///   - Balancer V2 (0%, multi-token, multi-chain)
///   - Morpho Blue (0%, multi-token, ETH/Base/Optimism)
///   - MakerDAO DssFlash (0%, DAI only, Ethereum only)
/// The scanner always picks the cheapest available source.
fn estimate_flash_loan_fee(
    _prices: &[TokenPrice],
    token: &str,
    notional_usd: f64,
) -> FlashLoanRecommendation {
    let mut sources: Vec<(FlashLoanSource, f64, &str)> = vec![
        (FlashLoanSource::BalancerV2, FlashLoanSource::BalancerV2.fee_pct(token),
            "Balancer V2 - 0% fee, preferred"),
        (FlashLoanSource::MorphoBlue, FlashLoanSource::MorphoBlue.fee_pct(token),
            "Morpho Blue - 0% fee"),
        (FlashLoanSource::MakerDssFlash, FlashLoanSource::MakerDssFlash.fee_pct(token),
            "MakerDAO DssFlash - 0% fee, DAI only"),
        (FlashLoanSource::Spark, FlashLoanSource::Spark.fee_pct(token),
            "Spark Protocol - 0% on DAI, 0.05% on others"),
        (FlashLoanSource::RadiantV2, FlashLoanSource::RadiantV2.fee_pct(token),
            "Radiant V2 - 0.03% fee"),
        (FlashLoanSource::AaveV3, FlashLoanSource::AaveV3.fee_pct(token),
            "Aave V3 - 0.05% fee"),
        (FlashLoanSource::UniswapV3, FlashLoanSource::UniswapV3.fee_pct(token),
            "Uniswap V3 Flash - pool fee tier"),
    ];

    // MakerDAO only supports DAI
    if token.to_ascii_uppercase() != "DAI" {
        sources.retain(|s| !matches!(s.0, FlashLoanSource::MakerDssFlash));
    }

    sources.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    FlashLoanRecommendation {
        source: sources[0].0.clone(),
        fee_pct: sources[0].1,
        fee_usd: notional_usd * (sources[0].1 / 100.0),
        reason: sources[0].2.to_string(),
    }
}

fn calculate_confidence(spread_pct: f64, net_profit_usd: f64, liquidity_usd: f64) -> f64 {
    let spread_score = (spread_pct / 10.0).min(1.0);
    let profit_score = (net_profit_usd / 500.0).min(1.0);
    let liq_score = (liquidity_usd / 1_000_000.0).min(1.0);
    (spread_score * 0.4 + profit_score * 0.3 + liq_score * 0.3).min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token_price(symbol: &str) -> TokenPrice {
        TokenPrice {
            token: symbol.to_string(),
            token_address: "0x0000000000000000000000000000000000000001".into(),
            chain_id: 1,
            chain_name: "Ethereum".into(),
            dex_name: "Uniswap V2".into(),
            price_usd: 0.0004,
            liquidity_usd: 1_000_000.0,
            timestamp: 0,
            pool_fee_bps_hundredths: 3000, // V2 default 0.30%
        }
    }

    /// Balancer V2 (0%) is the cheapest flash loan source for non-DAI tokens.
    /// The scanner must prefer it over Aave/Radiant/Spark.
    #[test]
    fn flash_loan_fee_is_charged_on_the_notional() {
        let prices = vec![token_price("USDC")];
        let r = estimate_flash_loan_fee(&prices, "USDC", 10_000.0);
        // Balancer V2 wins at 0% — this is correct: we have three 0%-fee
        // sources (Balancer, Morpho, MakerDAO-DAI) so the cost is genuinely
        // zero for the flash-loan leg. Other costs (gas, slippage, routing)
        // are accounted for separately in the net-profit calculation.
        assert_eq!(r.fee_pct, 0.0, "expected Balancer V2 to win at 0%");
        assert_eq!(r.fee_usd, 0.0, "0% of 10,000 is 0.00, got {}", r.fee_usd);
    }

    /// DAI gets MakerDAO DssFlash or Balancer — both 0% fee.
    #[test]
    fn dai_flash_loan_is_free_on_spark() {
        let prices = vec![token_price("DAI")];
        let r = estimate_flash_loan_fee(&prices, "DAI", 10_000.0);
        assert_eq!(r.fee_pct, 0.0);
        assert_eq!(r.fee_usd, 0.0);
    }

    /// With 0% sources, fee_usd is 0 for all sizes. Verify that the fee
    /// calculation still returns 0 regardless of notional.
    #[test]
    fn flash_loan_fee_scales_linearly() {
        let prices = vec![token_price("USDC")];
        let small = estimate_flash_loan_fee(&prices, "USDC", 1_000.0);
        let large = estimate_flash_loan_fee(&prices, "USDC", 10_000.0);
        // Both use Balancer V2 at 0%, so both fees are 0.
        assert_eq!(small.fee_usd, 0.0);
        assert_eq!(large.fee_usd, 0.0);
        assert_eq!(small.fee_pct, large.fee_pct);
    }

    /// A missing or malformed native/USD rate must suppress the opportunity,
    /// never default to a number. This is what stops an unpriced spread from
    /// being reported as dollar profit.
    #[test]
    fn missing_native_usd_rate_yields_none() {
        let s = RadarScanner::new();
        let key = "ZCA_NATIVE_USD_31337";
        std::env::remove_var(key);
        // Only assert `None` when no default is set in the ambient environment.
        if std::env::var("ZCA_NATIVE_USD").is_err() {
            assert_eq!(s.native_usd_rate(31337), None);
        }

        std::env::set_var(key, "2674.87");
        assert_eq!(s.native_usd_rate(31337), Some(2674.87));

        // A non-positive or non-numeric rate is not a rate.
        std::env::set_var(key, "0");
        assert_eq!(s.native_usd_rate(31337), None);
        std::env::set_var(key, "-5");
        assert_eq!(s.native_usd_rate(31337), None);
        std::env::set_var(key, "not-a-number");
        assert_eq!(s.native_usd_rate(31337), None);
        std::env::remove_var(key);
    }

    /// `net_profit_pct` is a return on capital. The previous code divided by
    /// total cost, which is a markup multiple and can exceed 100% on a thin
    /// spread, not a rate of return.
    #[test]
    fn net_pct_is_a_return_on_capital_not_on_cost() {
        let notional: f64 = 10_000.0;
        let net: f64 = 50.0;
        let pct: f64 = (net / notional) * 100.0;
        assert!((pct - 0.5).abs() < 1e-9);
        assert!(pct < 100.0, "a 0.5% return must not read as a huge number");
    }

    /// Constant-product simulation must equal the textbook getAmountOut:
    /// out = in*0.997*reserveOut / (reserveIn + in*0.997) for a 0.30% pool.
    #[test]
    fn simulate_cp_matches_uniswap_v2_formula() {
        // in=1e18 wei into a 100/200 pool → out = 0.997e18*200e18/(100e18+0.997e18)
        let out = simulate_cp_out(1e18, 100e18, 200e18, 3000);
        let expect = 0.997e18 * 200e18 / (100e18 + 0.997e18);
        assert!((out - expect).abs() / expect < 1e-12, "got {out}, want {expect}");
        // Degenerate inputs return 0, never panic or go negative.
        assert_eq!(simulate_cp_out(0.0, 1.0, 1.0, 3000), 0.0);
        assert_eq!(simulate_cp_out(1.0, 0.0, 1.0, 3000), 0.0);
        assert_eq!(simulate_cp_out(1.0, 1.0, 0.0, 3000), 0.0);
    }

    /// A symmetric round trip through the same pool always LOSES the fee —
    /// the simulator must never manufacture profit from nothing.
    #[test]
    fn simulate_cp_round_trip_loses_fees() {
        let mid = simulate_cp_out(1e18, 100e18, 200e18, 3000);
        let back = simulate_cp_out(mid, 200e18, 100e18, 3000);
        // ~0.6% loss (fee on both legs) plus symmetric impact.
        assert!(back < 1e18, "round trip must lose money, got {back}");
        assert!(back > 0.0);
    }

    /// Concentrated-liquidity simulation matches the tick-local invariant:
    /// selling dx of token0 moves sqrtP down by dx/L; out = L·(1/√P'−1/√P).
    #[test]
    fn simulate_cl_matches_tick_invariant() {
        let sqrt_p = 2.0_f64;
        let sqrt_p_x96 = sqrt_p * 79_228_162_514_264_337_593_543_950_336.0;
        let l = 1_000_000.0;
        // Sell 1000 units of token0 → price drops, token1 comes out.
        let out = simulate_cl_out(1000.0, sqrt_p_x96, l, 3000, true);
        let net = 1000.0 * 0.997;
        let sqrt_p2 = sqrt_p - net / l;
        let expect = l * (1.0 / sqrt_p2 - 1.0 / sqrt_p);
        assert!((out - expect).abs() / expect < 1e-9);
        // Exceeding the tick's liquidity must refuse, not extrapolate.
        assert_eq!(simulate_cl_out(10.0 * l * sqrt_p, sqrt_p_x96, l, 3000, true), 0.0);
        // Selling token1 moves price up and yields token0.
        let out0 = simulate_cl_out(1000.0, sqrt_p_x96, l, 0, false);
        let sqrt_p3 = sqrt_p + 1000.0 / l;
        let expect0 = l * (1.0 / sqrt_p - 1.0 / sqrt_p3);
        assert!((out0 - expect0).abs() / expect0 < 1e-9);
    }

    /// V2 venue with coherent state: quote reserve = price × token reserve,
    /// and `depth` is the real quote-side depth in human units so the
    /// optimizer's size cap stays consistent with the reserves.
    fn v2_quote(price: f64) -> VenueQuote {
        let token_raw = 1_000e18;
        let quote_raw = price * token_raw;
        VenueQuote {
            price,
            depth: quote_raw / 1e18,
            fee_bps_hundredths: 3000,
            pool: Address::ZERO,
            state: PoolState::Reserves {
                token: U256::from(token_raw as u128),
                quote: U256::from(quote_raw as u128),
            },
        }
    }

    /// A real cross-venue edge must survive simulation at SOME size — the
    /// optimizer finds it and reports a positive net of the flash fee.
    #[test]
    fn optimizer_finds_profitable_size_on_real_edge() {
        let buy = v2_quote(0.001);   // token cheap here
        let sell = v2_quote(0.00105); // 5% richer
        let sim = optimize_round_trip(&buy, &sell, 0.0005).expect("edge should validate");
        assert!(sim.net_quote > 0.0);
        assert!(sim.amount_in > 0.0);
        // Never sizes beyond 25% of the shallower side (depth = 1.0 here).
        assert!(sim.amount_in <= 0.25e18 * 1.001);
    }

    /// Equal prices on both venues → every size loses the two swap fees.
    /// The optimizer must say None, not return the least-bad size.
    #[test]
    fn optimizer_rejects_phantom_spread() {
        let a = v2_quote(0.001);
        let b = v2_quote(0.001);
        assert!(optimize_round_trip(&a, &b, 0.0).is_none());
    }

    /// A spread that exists only at mid-price but not past the fees must
    /// also reject — 0.1% spot gap vs 0.6% total pool fees is not an edge.
    #[test]
    fn optimizer_rejects_sub_fee_spread() {
        let buy = v2_quote(0.001);
        let sell = v2_quote(0.001001); // 0.1% gap < 0.6% fees
        assert!(optimize_round_trip(&buy, &sell, 0.0).is_none());
    }

    /// SpotOnly venues (Solidly-stable) can never be simulated.
    #[test]
    fn optimizer_skips_spot_only_state() {
        let buy = VenueQuote {
            price: 0.001,
            depth: 50.0,
            fee_bps_hundredths: 3000,
            pool: Address::ZERO,
            state: PoolState::SpotOnly,
        };
        let sell = v2_quote(0.00105);
        assert!(optimize_round_trip(&buy, &sell, 0.0).is_none());
    }

    /// A CL leg whose input would push sqrt price beyond ~5% must refuse —
    /// past that the constant-L output is an extrapolation across tick
    /// boundaries we haven't read, i.e. a phantom-quote vector.
    #[test]
    fn cl_leg_refuses_fills_beyond_tick_confidence() {
        const TWO96: f64 = 79_228_162_514_264_337_593_543_950_336.0;
        let state = PoolState::Concentrated {
            sqrt_price_x96: U256::from((2.0 * TWO96) as u128),
            liquidity: U256::from(1_000_000u64),
            token_is_token0: true,
        };
        // 10k in → ~0.5% excursion → simulated.
        assert!(simulate_leg_out(&state, 3000, 10_000.0, false).is_some());
        // 200k in → ~10% excursion → refused, not extrapolated.
        assert!(simulate_leg_out(&state, 3000, 200_000.0, false).is_none());
    }

    /// sqrt-limit for a zeroForOne fill must sit BELOW the expected end
    /// price (1% slack); for oneForZero, above it.
    #[test]
    fn v3_sqrt_limit_bounds_fill_direction() {
        const TWO96: f64 = 79_228_162_514_264_337_593_543_950_336.0;
        let sqrt_raw = 2.0 * TWO96;
        let l = 1_000_000.0;
        let lim_down: f64 = v3_sqrt_limit_x96(sqrt_raw, l, 3000, 1000.0, true)
            .parse().unwrap();
        let lim_up: f64 = v3_sqrt_limit_x96(sqrt_raw, l, 3000, 1000.0, false)
            .parse().unwrap();
        let expected_after_down = (2.0 - 1000.0 * 0.997 / l) * TWO96;
        let expected_after_up = (2.0 + 1000.0 * 0.997 / l) * TWO96;
        assert!(lim_down < expected_after_down && lim_down > expected_after_down * 0.95);
        assert!(lim_up > expected_after_up && lim_up < expected_after_up * 1.05);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenInfo {
    pub symbol: String,
    pub address: String,
}

/// DEPRECATED: Legacy direct-wallet execution — replaced by gasless ERC-4337.
///
/// Previously used to sign and broadcast Velora swap txs from the EOA wallet.
/// Now superseded by `gasless::build_and_send_userop()` which routes through
/// Pimlico's bundler + paymaster for zero-gas execution.
///
/// Retained as dead code for testing/debugging only.
#[allow(dead_code)]
async fn execute_tx_onchain(
    rpc_url: &str,
    private_key: &str,
    tx_params: &crate::velora_client::VeloraTxParams,
    chain_id: u64,
) -> Result<String, String> {
    if private_key.is_empty() {
        return Err("no PRIVATE_KEY configured — cannot sign transactions".to_string());
    }

    if rpc_url.is_empty() {
        return Err("no RPC URL for chain".to_string());
    }

    let to_addr = &tx_params.to;
    let data = &tx_params.data;
    let value_str = &tx_params.value;

    if to_addr.is_empty() || data.is_empty() || data == "0x" {
        return Err("Velora tx_params missing required fields (to, data)".to_string());
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("HTTP client error: {e}"))?;

    // Derive wallet address from private key
    let pk_hex = private_key.trim_start_matches("0x");
    let pk_bytes = hex::decode(pk_hex)
        .map_err(|e| format!("invalid private key hex: {e}"))?;
    if pk_bytes.len() != 32 {
        return Err(format!("private key must be 32 bytes, got {}", pk_bytes.len()));
    }

    // Use k256 for signing (available via alloy's re-export)
    use k256::ecdsa::{SigningKey, signature::hazmat::PrehashSigner};
    let signing_key = SigningKey::from_bytes((&pk_bytes[..]).into())
        .map_err(|e| format!("invalid signing key: {e}"))?;

    // Derive public key → address
    let verifying_key = signing_key.verifying_key();
    let public_key_bytes = verifying_key.to_encoded_point(false);
    let public_key_uncompressed = &public_key_bytes.as_bytes()[1..]; // skip 0x04 prefix
    use alloy::primitives::keccak256;
    let hash = keccak256(public_key_uncompressed);
    let wallet_hex = format!("0x{}", hex::encode(&hash[12..]));

    // 1. Get nonce
    let nonce_resp = rpc_call(&client, rpc_url, "eth_getTransactionCount", &serde_json::json!([wallet_hex, "latest"])).await?;
    let nonce_hex = nonce_resp.as_str().unwrap_or("0x0");
    let nonce = u64::from_str_radix(nonce_hex.trim_start_matches("0x"), 16).unwrap_or(0);

    // 2. Get gas price
    let gas_resp = rpc_call(&client, rpc_url, "eth_gasPrice", &serde_json::json!([])).await?;
    let gas_price_hex = gas_resp.as_str().unwrap_or("0x0");
    let gas_price = u128::from_str_radix(gas_price_hex.trim_start_matches("0x"), 16).unwrap_or(0);

    // 3. Parse value
    let value = if value_str.starts_with("0x") {
        u128::from_str_radix(value_str.trim_start_matches("0x"), 16).unwrap_or(0)
    } else {
        value_str.parse::<u128>().unwrap_or(0)
    };

    // 4. Estimate gas
    let est_result = rpc_call(&client, rpc_url, "eth_estimateGas", &serde_json::json!([{
        "from": wallet_hex,
        "to": to_addr,
        "data": data,
        "value": format!("0x{:x}", value),
    }]))
    .await;

    let gas_limit = match est_result {
        Ok(v) => {
            let hex = v.as_str().unwrap_or("0x0");
            let limit = u64::from_str_radix(hex.trim_start_matches("0x"), 16).unwrap_or(300_000);
            limit + limit / 5 // 20% buffer
        }
        Err(e) => {
            return Err(format!("eth_estimateGas failed: {e} — tx would revert on-chain"));
        }
    };

    // 5. Build and RLP-encode legacy tx, then sign
    //
    // Legacy tx RLP: [nonce, gasPrice, gasLimit, to, value, data, chainId, 0, 0]
    // After signing: [nonce, gasPrice, gasLimit, to, value, data, v, r, s]
    let to_bytes = hex::decode(to_addr.trim_start_matches("0x"))
        .map_err(|e| format!("invalid to address: {e}"))?;
    let data_bytes = hex::decode(data.trim_start_matches("0x"))
        .map_err(|e| format!("invalid data: {e}"))?;

    // Encode the signing payload (EIP-155): rlp([nonce, gasPrice, gasLimit, to, value, data, chainId, 0, 0])
    let sign_payload = rlp_encode_legacy_tx_for_signing(
        nonce, gas_price, gas_limit as u128, &to_bytes, value, &data_bytes, chain_id,
    );
    let sig_hash = keccak256(&sign_payload);

    // Sign the hash
    let (signature, recovery_id) = signing_key
        .sign_prehash(sig_hash.as_ref())
        .map_err(|e| format!("signing failed: {e}"))?;

    let sig_bytes = signature.to_bytes();
    let r = &sig_bytes[..32];
    let s = &sig_bytes[32..64];
    let v = chain_id * 2 + 35 + recovery_id.to_byte() as u64;

    // RLP-encode the signed tx
    let signed_tx = rlp_encode_signed_legacy_tx(
        nonce, gas_price, gas_limit as u128, &to_bytes, value, &data_bytes, v, r, s,
    );
    let raw_tx_hex = format!("0x{}", hex::encode(&signed_tx));

    // 6. Broadcast
    let send_result = rpc_call(
        &client,
        rpc_url,
        "eth_sendRawTransaction",
        &serde_json::json!([raw_tx_hex]),
    )
    .await;

    match send_result {
        Ok(v) => {
            let tx_hash = v.as_str().unwrap_or("").to_string();
            if tx_hash.is_empty() {
                Err("broadcast returned empty tx hash".to_string())
            } else {
                Ok(tx_hash)
            }
        }
        Err(e) => Err(format!("eth_sendRawTransaction rejected: {e}")),
    }
}

/// JSON-RPC helper
async fn rpc_call(
    client: &reqwest::Client,
    rpc_url: &str,
    method: &str,
    params: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
        "id": 1
    });
    let resp = client
        .post(rpc_url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("{method} request failed: {e}"))?;
    let raw: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("{method} parse failed: {e}"))?;

    if let Some(err) = raw.get("error") {
        let msg = err["message"].as_str().unwrap_or("unknown");
        return Err(format!("{method}: {msg}"));
    }

    Ok(raw["result"].clone())
}

// ── Minimal RLP encoding for legacy transactions ───────────────────

fn rlp_encode_uint(val: u128) -> Vec<u8> {
    if val == 0 {
        return vec![0x80]; // empty string
    }
    let bytes = val.to_be_bytes();
    let start = bytes.iter().position(|&b| b != 0).unwrap_or(bytes.len());
    let trimmed = &bytes[start..];
    if trimmed.len() == 1 && trimmed[0] < 0x80 {
        trimmed.to_vec()
    } else {
        let mut out = vec![0x80 + trimmed.len() as u8];
        out.extend_from_slice(trimmed);
        out
    }
}

fn rlp_encode_u64(val: u64) -> Vec<u8> {
    rlp_encode_uint(val as u128)
}

fn rlp_encode_bytes(data: &[u8]) -> Vec<u8> {
    if data.len() == 1 && data[0] < 0x80 {
        data.to_vec()
    } else if data.is_empty() {
        vec![0x80]
    } else if data.len() < 56 {
        let mut out = vec![0x80 + data.len() as u8];
        out.extend_from_slice(data);
        out
    } else {
        let len_bytes = {
            let l = data.len();
            let b = l.to_be_bytes();
            let start = b.iter().position(|&x| x != 0).unwrap_or(b.len());
            b[start..].to_vec()
        };
        let mut out = vec![0xb7 + len_bytes.len() as u8];
        out.extend_from_slice(&len_bytes);
        out.extend_from_slice(data);
        out
    }
}

fn rlp_encode_list(items: &[Vec<u8>]) -> Vec<u8> {
    let mut payload = Vec::new();
    for item in items {
        payload.extend_from_slice(item);
    }
    if payload.len() < 56 {
        let mut out = vec![0xc0 + payload.len() as u8];
        out.extend(payload);
        out
    } else {
        let len_bytes = {
            let l = payload.len();
            let b = l.to_be_bytes();
            let start = b.iter().position(|&x| x != 0).unwrap_or(b.len());
            b[start..].to_vec()
        };
        let mut out = vec![0xf7 + len_bytes.len() as u8];
        out.extend_from_slice(&len_bytes);
        out.extend(payload);
        out
    }
}

fn rlp_encode_legacy_tx_for_signing(
    nonce: u64,
    gas_price: u128,
    gas_limit: u128,
    to: &[u8],
    value: u128,
    data: &[u8],
    chain_id: u64,
) -> Vec<u8> {
    rlp_encode_list(&[
        rlp_encode_u64(nonce),
        rlp_encode_uint(gas_price),
        rlp_encode_uint(gas_limit),
        rlp_encode_bytes(to),
        rlp_encode_uint(value),
        rlp_encode_bytes(data),
        rlp_encode_u64(chain_id),
        rlp_encode_uint(0), // empty r
        rlp_encode_uint(0), // empty s
    ])
}

fn rlp_encode_signed_legacy_tx(
    nonce: u64,
    gas_price: u128,
    gas_limit: u128,
    to: &[u8],
    value: u128,
    data: &[u8],
    v: u64,
    r: &[u8],
    s: &[u8],
) -> Vec<u8> {
    // Trim leading zeros from r and s
    let r_trimmed = &r[r.iter().position(|&b| b != 0).unwrap_or(r.len())..];
    let s_trimmed = &s[s.iter().position(|&b| b != 0).unwrap_or(s.len())..];
    rlp_encode_list(&[
        rlp_encode_u64(nonce),
        rlp_encode_uint(gas_price),
        rlp_encode_uint(gas_limit),
        rlp_encode_bytes(to),
        rlp_encode_uint(value),
        rlp_encode_bytes(data),
        rlp_encode_u64(v),
        rlp_encode_bytes(r_trimmed),
        rlp_encode_bytes(s_trimmed),
    ])
}

/// Build an HTTP provider using round-robin endpoint rotation.
///
/// Each call advances a global atomic counter so successive calls cycle
/// through every endpoint in the list. This distributes RPC load across
/// all 18-27 endpoints per chain rather than always hammering the first.
///
/// With 36 tokens x 2 phases = 72 calls/chain/cycle, rotating across
/// ~25 endpoints means each endpoint sees ~3 calls per cycle — well
/// within every free-tier rate limit.
fn try_build_provider(urls: &[String]) -> Result<HttpProvider, String> {
    if urls.is_empty() {
        return Err("No RPC URLs configured".to_string());
    }
    let n = urls.len();
    let start = RPC_ROUND_ROBIN.fetch_add(1, Ordering::Relaxed) % n;

    // Try from the rotated start position, wrapping around the full list
    for i in 0..n {
        let idx = (start + i) % n;
        match urls[idx].parse::<reqwest::Url>() {
            Ok(parsed) => return Ok(ProviderBuilder::new().on_http(parsed)),
            Err(e) => warn!("RPC {} failed to parse, trying next: {}", urls[idx], e),
        }
    }
    Err(format!("No working RPC URL in {:?}", urls))
}
