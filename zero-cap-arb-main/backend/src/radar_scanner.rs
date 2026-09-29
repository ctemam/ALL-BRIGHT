use crate::chains::{get_chains, Protocol};
use crate::multicall;
use crate::types::*;
use alloy::primitives::Address;
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
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, error, warn};
use uuid::Uuid;

/// Core radar scanner that finds arbitrage opportunities across every
/// configured chain and every registry-listed venue on it.
pub struct RadarScanner {
    price_cache: Arc<DashMap<String, Vec<TokenPrice>>>,
    /// Minimum spread % to report (default 0.5%)
    min_spread_pct: f64,
    /// Cache TTL in seconds
    cache_ttl_secs: u64,
}

impl RadarScanner {
    pub fn new() -> Self {
        Self {
            price_cache: Arc::new(DashMap::new()),
            min_spread_pct: 0.5,
            cache_ttl_secs: 15,
        }
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

            // Resolve token address if not provided
            let addr = match token_address {
                Some(a) => a.to_string(),
                None => self.resolve_token_address(token_symbol, chain.id).await?,
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

        let provider = match try_build_provider(&chain.rpc_urls) {
            Ok(p) => p,
            Err(e) => {
                error!("All RPCs failed for {}: {}", chain.name, e);
                return Vec::new();
            }
        };

        let quotes = match quote_venues_on_chain(&provider, chain, token_address).await {
            Ok(q) => q,
            Err(e) => {
                warn!("venue quoting failed on {}: {}", chain.name, e);
                return Vec::new();
            }
        };

        let timestamp = Utc::now().timestamp() as u64;
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
            })
            .collect();

        self.price_cache.insert(cache_key, prices.clone());
        prices
    }
}

/// One venue's quote for a token, denominated in the chain's wrapped native.
#[derive(Debug, Clone, Copy)]
struct VenueQuote {
    /// Human price of the token in wrapped native.
    price: f64,
    /// Depth of the quote side at the current tick, in wrapped native.
    depth: f64,
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
            // A V3 venue with no resolution parameters cannot be resolved. The
            // registry test forbids this, so skip rather than guess a selector.
            (Protocol::V3, None) => continue,
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
            Protocol::V3 => multicall::SubCall::new(
                c.factory,
                multicall::factory_get_pool_call_data(token, weth, c.fee, c.pool_sig),
            ),
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
            Protocol::V2 => {
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
    for (i, (candidate_idx, _)) in pools.iter().enumerate() {
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
            Protocol::V2 => {
                let Some(reserves) = multicall::decode_reserves(primary) else {
                    continue;
                };
                let (reserve_token, reserve_weth) = if token_is_token0 {
                    (reserves.reserve0, reserves.reserve1)
                } else {
                    (reserves.reserve1, reserves.reserve0)
                };
                let price = multicall::price_from_reserves(
                    reserve_token,
                    reserve_weth,
                    token_decimals,
                    QUOTE_DECIMALS,
                );
                // Under the constant-product invariant the quote side is half
                // the pool, so two-sided depth is twice that reserve.
                let depth = multicall::scale_amount(reserve_weth, QUOTE_DECIMALS) * 2.0;
                VenueQuote { price, depth }
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
                let depth = liquidity_idx[i]
                    .and_then(|li| ok_data(&state, li))
                    .and_then(multicall::decode_liquidity)
                    .map(|l| {
                        multicall::v3_depth_in_quote(
                            slot0.sqrt_price_x96,
                            l,
                            QUOTE_DECIMALS,
                            token_is_token0,
                        )
                    })
                    .unwrap_or(0.0);
                VenueQuote { price, depth }
            }
        };

        // 0.0 means "no data" (empty or unreadable pool), not a cheap venue.
        if quote.price <= 0.0 || quote.depth <= 0.0 {
            continue;
        }
        quotes.push((candidate.label.clone(), quote));
    }

    Ok(quotes)
}

impl RadarScanner {
    /// Resolve a token symbol to its address on a given chain
    async fn resolve_token_address(
        &self,
        symbol: &str,
        chain_id: u64,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        // Production: use tokenlists, on-chain lookup, or CoinGecko API
        let chain_name = crate::chains::get_chain_name(chain_id);
        // This would be an API call to a token registry
        // For now, return a placeholder - real impl uses multiple sources
        Err(format!(
            "Token {} not resolved on {}. Provide address directly.",
            symbol, chain_name
        )
        .into())
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
            let spread_pct = if cheapest.price_usd > 0.0 {
                ((priciest.price_usd - cheapest.price_usd) / cheapest.price_usd) * 100.0
            } else {
                0.0
            };

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

            // A token address is required: symbol resolution is not implemented,
            // so a symbol alone cannot be quoted.
            let addr = match token_address {
                Some(a) => a.to_string(),
                None => continue,
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

        // 2. Simulated triangular opportunities
        let triangular = vec![TriangularOpportunity {
            id: Uuid::new_v4().to_string(),
            chain_id: 1,
            chain_name: "Ethereum".into(),
            legs: vec![
                TriangularLeg {
                    from_token: "DAI".into(),
                    to_token: "ETH".into(),
                    dex: "Uniswap V3".into(),
                    rate: 0.00041,
                    expected_output: "0.41 ETH".into(),
                },
                TriangularLeg {
                    from_token: "ETH".into(),
                    to_token: "USDC".into(),
                    dex: "Curve".into(),
                    rate: 3450.0,
                    expected_output: "1414.5 USDC".into(),
                },
                TriangularLeg {
                    from_token: "USDC".into(),
                    to_token: "DAI".into(),
                    dex: "Balancer".into(),
                    rate: 1.001,
                    expected_output: "1415.9 DAI".into(),
                },
            ],
            start_token: "DAI".into(),
            end_token: "DAI".into(),
            net_profit_pct: 1.59,
            estimated_profit_usd: 15.90,
        }];

        // 3. Simulated cross-chain opportunities
        let cross_chain = vec![CrossChainOpportunity {
            id: Uuid::new_v4().to_string(),
            token: token_symbol.to_string(),
            buy_chain_id: 137,
            buy_chain_name: "Polygon".into(),
            buy_price: 0.98,
            sell_chain_id: 1,
            sell_chain_name: "Ethereum".into(),
            sell_price: 1.02,
            bridge_fee_usd: 0.50,
            net_profit_pct: 3.57,
            estimated_profit_usd: 35.70,
        }];

        // 4. Simulated mint opportunities
        let mint = vec![MintOpportunity {
            id: Uuid::new_v4().to_string(),
            token: "DAI".into(),
            mint_platform: "Spark Protocol".into(),
            mint_cost_usd: 0.995,
            market_price_usd: 1.005,
            spread_pct: 1.01,
            estimated_profit_usd: 10.05,
        }];

        // 5. Simulated JIT liquidity
        let jit = vec![JitLiquidityOpportunity {
            id: Uuid::new_v4().to_string(),
            token: token_symbol.to_string(),
            pool: format!("{}/USDC 0.3%", token_symbol),
            dex: "Uniswap V3".into(),
            chain_id: 1,
            chain_name: "Ethereum".into(),
            expected_fee_usd: 125.0,
            capital_required_usd: 50_000.0,
        }];

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

    /// Set minimum spread percentage
    pub fn set_min_spread(&mut self, pct: f64) {
        self.min_spread_pct = pct;
    }

    /// Clear price cache
    pub fn clear_cache(&self) {
        self.price_cache.clear();
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

        for chain in chains {
            let venues = crate::chains::get_venues(chain.id);
            // A chain with no registry-listed venue is not "scanned"; recording it as
            // such was misleading on the four chains the DEX table omitted.
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

            for token in tokens {
                if !tokens_scanned.contains(&token.symbol) {
                    tokens_scanned.push(token.symbol.clone());
                }

                let addr = &token.address;
                let symbol = &token.symbol;

                // One batched MultiCall3 read covers every venue on the chain.
                let chain_prices = self.query_chain_prices(chain, symbol, addr).await;

                if chain_prices.len() < 2 {
                    continue;
                }

                // Find cheapest and most expensive on this chain
                if let (Some(cheapest), Some(priciest)) = (
                    chain_prices
                        .iter()
                        .min_by(|a, b| a.price_usd.partial_cmp(&b.price_usd).unwrap()),
                    chain_prices
                        .iter()
                        .max_by(|a, b| a.price_usd.partial_cmp(&b.price_usd).unwrap()),
                ) {
                    let spread_pct = if cheapest.price_usd > 0.0 {
                        ((priciest.price_usd - cheapest.price_usd) / cheapest.price_usd) * 100.0
                    } else {
                        0.0
                    };

                    if spread_pct >= self.min_spread_pct {
                        let gross_profit = (priciest.price_usd - cheapest.price_usd) * 1000.0; // assume 1000 tokens
                        let gas_est = estimate_gas_cost(chain.id);
                        let fl_fee = estimate_flash_loan_fee(&chain_prices, symbol);
                        let slippage = gross_profit * 0.005; // 0.5% slippage
                        let total_cost = gas_est + fl_fee.fee_usd + slippage;
                        let net_profit = gross_profit - total_cost;
                        let net_pct = if total_cost > 0.0 {
                            (net_profit / total_cost) * 100.0
                        } else {
                            0.0
                        };
                        let roi = if cheapest.price_usd > 0.0 {
                            (net_profit / (cheapest.price_usd * 1000.0)) * 100.0
                        } else {
                            0.0
                        };

                        if net_profit > 0.0 {
                            let mut recommendations = vec![fl_fee.clone()];
                            // Add alternative flash loan sources
                            for alt_source in &[
                                FlashLoanSource::AaveV3,
                                FlashLoanSource::RadiantV2,
                                FlashLoanSource::Spark,
                            ] {
                                if alt_source.as_str() != fl_fee.source.as_str() {
                                    let alt_fee = alt_source.fee_pct(symbol);
                                    recommendations.push(FlashLoanRecommendation {
                                        source: alt_source.clone(),
                                        fee_pct: alt_fee,
                                        fee_usd: gross_profit * (alt_fee / 100.0),
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
                                        flash_loan_fee_usd: fl_fee.fee_usd,
                                        slippage_estimated_usd: slippage,
                                        bridge_fee_usd: if cheapest.chain_id != priciest.chain_id {
                                            Some(0.50)
                                        } else {
                                            None
                                        },
                                        velora_fee_usd: gross_profit * 0.001,
                                        total_cost_usd: total_cost,
                                    },
                                    net_profit_usd: net_profit,
                                    net_profit_pct: net_pct,
                                    roi_pct: roi,
                                    is_profitable: true,
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
        1 => 15.0,     // Ethereum
        42161 => 0.30, // Arbitrum
        10 => 0.25,    // Optimism
        137 => 0.50,   // Polygon
        56 => 0.40,    // BSC
        43114 => 0.60, // Avalanche
        _ => 1.0,
    }
}

/// Pick the cheapest flash-loan source for a token.
///
/// The `prices` slice is deliberately unused: the fee that matters is the
/// lender's fee rate, which depends on the token, not on observed venue prices.
/// It is kept in the signature so the call site does not have to change.
fn estimate_flash_loan_fee(_prices: &[TokenPrice], token: &str) -> FlashLoanRecommendation {
    let spark_fee = FlashLoanSource::Spark.fee_pct(token);
    let aave_fee = FlashLoanSource::AaveV3.fee_pct(token);
    let radiant_fee = FlashLoanSource::RadiantV2.fee_pct(token);

    let mut sources = [
        (
            FlashLoanSource::Spark,
            spark_fee,
            "Spark Protocol - 0% on DAI, 0.05% on others",
        ),
        (
            FlashLoanSource::RadiantV2,
            radiant_fee,
            "Radiant V2 - 0.03% lowest standard fee",
        ),
        (
            FlashLoanSource::AaveV3,
            aave_fee,
            "Aave V3 - 0.05% standard fee",
        ),
    ];
    sources.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    FlashLoanRecommendation {
        source: sources[0].0.clone(),
        fee_pct: sources[0].1,
        fee_usd: 0.0, // calculated in context
        reason: sources[0].2.to_string(),
    }
}

fn calculate_confidence(spread_pct: f64, net_profit_usd: f64, liquidity_usd: f64) -> f64 {
    let spread_score = (spread_pct / 10.0).min(1.0);
    let profit_score = (net_profit_usd / 500.0).min(1.0);
    let liq_score = (liquidity_usd / 1_000_000.0).min(1.0);
    (spread_score * 0.4 + profit_score * 0.3 + liq_score * 0.3).min(1.0)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenInfo {
    pub symbol: String,
    pub address: String,
}

/// Try each RPC URL until one works
fn try_build_provider(urls: &[String]) -> Result<HttpProvider, String> {
    for url in urls {
        // NOTE: `ProviderBuilder::on_http` takes a parsed URL, not a transport, and `Http`
        // does not implement FromStr (both surfaced on the first successful cargo check).
        match url.parse::<reqwest::Url>() {
            Ok(parsed) => return Ok(ProviderBuilder::new().on_http(parsed)),
            Err(e) => warn!("RPC {} failed to parse, trying next: {}", url, e),
        }
    }
    Err(format!("No working RPC URL in {:?}", urls))
}
