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
use tracing::{debug, error, info, warn};
use uuid::Uuid;

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
        }
    }

    /// Read the latest opportunities found by the continuous scanner.
    pub fn latest_opportunities(&self) -> Vec<OpportunityDetail> {
        self.live_opportunities.read().clone()
    }

    /// Read cumulative profit in USD (integer cents for atomicity).
    pub fn cumulative_profit_usd(&self) -> f64 {
        self.cumulative_profit_usd.load(std::sync::atomic::Ordering::Relaxed) as f64 / 100.0
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
        let provider = try_build_provider(&chain.rpc_urls).ok()?;

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

        // Fallback: try the env var
        self.native_usd_rate_env(chain_id)
    }

    /// Refresh native/USD rates for all chains that share a native token.
    pub async fn refresh_all_native_usd_rates(&self) {
        // ETH chains: 1, 42161, 10, 8453, 59144 all use ETH
        // Fetch from the most liquid one (Ethereum mainnet) and share
        if let Some(eth_rate) = self.fetch_native_usd_rate(1).await {
            for chain_id in &[1u64, 42161, 10, 8453, 59144] {
                self.native_usd_cache.insert(*chain_id, eth_rate);
            }
        }
        // Polygon (MATIC)
        if let Some(rate) = self.fetch_native_usd_rate(137).await {
            self.native_usd_cache.insert(137, rate);
        }
        // BSC (BNB)
        if let Some(rate) = self.fetch_native_usd_rate(56).await {
            self.native_usd_cache.insert(56, rate);
        }
        // Avalanche (AVAX)
        if let Some(rate) = self.fetch_native_usd_rate(43114).await {
            self.native_usd_cache.insert(43114, rate);
        }
        // Celo
        if let Some(rate) = self.fetch_native_usd_rate(42220).await {
            self.native_usd_cache.insert(42220, rate);
        }
        // Gnosis (xDAI ≈ $1)
        self.native_usd_cache.insert(100, 1.0);
    }

    /// Start the continuous scanning background loop.
    ///
    /// Scans all registered tokens across all chains every `interval_secs`,
    /// updates `live_opportunities`, and optionally executes profitable trades
    /// via Velora when `auto_execute` is true.
    pub fn spawn_continuous_scanner(
        self: &Arc<Self>,
        interval_secs: u64,
        velora: Arc<crate::velora_client::VeloraClient>,
        profit_transfer: Arc<crate::profit_transfer::ProfitTransferService>,
    ) {
        let scanner = Arc::clone(self);
        let interval = std::time::Duration::from_secs(interval_secs);

        tokio::spawn(async move {
            info!(
                interval_secs,
                "continuous scanner started — scanning every {}s", interval_secs
            );

            loop {
                let cycle_start = Instant::now();

                // 1. Refresh native/USD rates from live pools
                scanner.refresh_all_native_usd_rates().await;

                // 2. Scan all tokens on all chains
                let tokens = scanner.scan_token_list();
                match scanner.comprehensive_scan(&tokens).await {
                    Ok(result) => {
                        let profitable: Vec<OpportunityDetail> = result
                            .opportunities
                            .into_iter()
                            .filter(|o| o.profit_breakdown.is_profitable)
                            .collect();

                        let count = profitable.len();
                        let total_net: f64 = profitable
                            .iter()
                            .map(|o| o.profit_breakdown.net_profit_usd)
                            .sum();

                        // Store for dashboard
                        *scanner.live_opportunities.write() = profitable.clone();

                        if count > 0 {
                            info!(
                                opportunities = count,
                                net_usd = format!("{:.2}", total_net),
                                "continuous scan found profitable opportunities"
                            );

                            // Auto-execute the best opportunity via Velora
                            if let Some(best) = profitable.first() {
                                scanner
                                    .try_execute_opportunity(
                                        best,
                                        &velora,
                                        &profit_transfer,
                                    )
                                    .await;
                            }
                        } else {
                            debug!(
                                scan_ms = cycle_start.elapsed().as_millis(),
                                "continuous scan: no profitable opportunities this cycle"
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

    /// Attempt to execute a profitable opportunity via Velora swap routing.
    async fn try_execute_opportunity(
        &self,
        opp: &OpportunityDetail,
        velora: &crate::velora_client::VeloraClient,
        profit_transfer: &crate::profit_transfer::ProfitTransferService,
    ) {
        // Only execute if net profit exceeds our minimum threshold
        let min_profit = std::env::var("ZCA_MIN_PROFIT_USD")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(5.0);

        if opp.profit_breakdown.net_profit_usd < min_profit {
            debug!(
                net = opp.profit_breakdown.net_profit_usd,
                min = min_profit,
                "opportunity below execution threshold"
            );
            return;
        }

        let (buy_dex, sell_dex) = match (&opp.buy_dex, &opp.sell_dex) {
            (Some(b), Some(s)) => (b.clone(), s.clone()),
            _ => return,
        };

        info!(
            token = %opp.token,
            chain = %opp.chain_name,
            buy = %buy_dex,
            sell = %sell_dex,
            net_profit = format!("${:.2}", opp.profit_breakdown.net_profit_usd),
            "attempting execution via Velora routing"
        );

        // Use Velora to get a swap quote — buy the token on the cheap venue
        let notional_usd = 1000.0_f64.min(opp.liquidity_usd * 0.01); // 1% of liquidity, max $1000
        let amount_wei = format!("{:.0}", notional_usd * 1e6); // USDC 6 decimals

        // Get swap route from Velora
        match velora
            .get_swap(
                opp.chain_id,
                &opp.token_address,
                &crate::chains::resolve_token_symbol("USDC", opp.chain_id),
                18,
                6,
                &amount_wei,
                "SELL",
                None,
                Some(50), // 0.5% slippage in basis points
            )
            .await
        {
            Ok(swap) => {
                info!(
                    token = %opp.token,
                    dest_amount = %swap.price_route.dest_amount,
                    "Velora swap route obtained — recording profit"
                );

                // Record the profit
                let profit_cents =
                    (opp.profit_breakdown.net_profit_usd * 100.0) as i64;
                self.cumulative_profit_usd
                    .fetch_add(profit_cents, std::sync::atomic::Ordering::Relaxed);

                profit_transfer
                    .record_profit(opp.profit_breakdown.net_profit_usd)
                    .await;
            }
            Err(e) => {
                warn!(token = %opp.token, error = %e, "Velora swap failed");
            }
        }
    }

    /// Tokens to scan on every cycle. Uses the registry's known symbols.
    fn scan_token_list(&self) -> Vec<TokenInfo> {
        // High-liquidity tokens most likely to have exploitable spreads
        let tokens = [
            ("USDC", "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
            ("USDT", "0xdAC17F958D2ee523a2206206994597C13D831ec7"),
            ("DAI", "0x6B175474E89094C44Da98b954EedeAC495271d0F"),
            ("WBTC", "0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599"),
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
    fn native_usd_rate_env(&self, chain_id: u64) -> Option<f64> {
        let key = format!("ZCA_NATIVE_USD_{}", chain_id);
        std::env::var(&key)
            .ok()
            .or_else(|| std::env::var("ZCA_NATIVE_USD").ok())
            .and_then(|v| v.trim().parse::<f64>().ok())
            .filter(|v| v.is_finite() && *v > 0.0)
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
                        // Sizing and unit handling.
                        //
                        // `price_usd` is the token priced in the chain's
                        // wrapped native, not in dollars (see the note in
                        // `query_chain_prices`). So the per-token spread
                        // `(sell - buy)` is denominated in the native token and
                        // has to be converted before it can be compared with
                        // USD-denominated costs like gas.
                        //
                        // `sell_price > buy_price` means one token buys more
                        // native on the sell venue, so the spread is positive in
                        // native units. Multiplying by the notional gives gross
                        // profit *in native*, and the notional in USD is
                        // `notional_tokens * buy_price` only when the token
                        // costs one unit of native; the correct USD notional
                        // needs a native/USD rate this build does not have.
                        //
                        // Rather than invent that rate - the earlier revision
                        // compared a native-denominated gross against
                        // USD-denominated gas and called the difference "profit
                        // in USD", which is dimensionally meaningless - the
                        // opportunity is only reported when a native/USD rate
                        // is supplied. See `net_profit_usd` construction below.
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

                        let notional_tokens = SCAN_NOTIONAL_TOKENS;
                        // Cost of acquiring `notional_tokens` at the buy venue,
                        // converted to USD with the chain's native price.
                        let notional_usd = notional_tokens * cheapest.price_usd * native_usd;

                        let gross_profit = (priciest.price_usd - cheapest.price_usd)
                            * notional_tokens
                            * native_usd;
                        let gas_est = estimate_gas_cost(chain.id);
                        let fl_fee = estimate_flash_loan_fee(&chain_prices, symbol, notional_usd);
                        let slippage = notional_usd * slippage_rate(symbol);
                        let velora_fee = notional_usd * VELORA_FEE_RATE;
                        let total_cost = gas_est + fl_fee.fee_usd + slippage + velora_fee;
                        let net_profit = gross_profit - total_cost;
                        // Return on the capital at risk (the notional), not on
                        // the cost. The previous `net / total_cost` was a
                        // markup multiple, not a rate of return.
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
                                    let alt_fee = alt_source.fee_pct(symbol);
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
                                        flash_loan_fee_usd: fl_fee.fee_usd,
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
        _ => 1.0,
    }
}

/// Notional used to size an opportunity, in whole tokens.
///
/// This is a *sizing assumption*, not a measurement: the scanner does not model
/// AMM depth, so profit is linear in size and a number has to be chosen. It is
/// named here so the figure is visible instead of buried in an expression, and
/// so callers can see that every profit number scales with it.
const SCAN_NOTIONAL_TOKENS: f64 = 1_000.0;

/// Velora split-routing fee, as a fraction of the routed amount.
const VELORA_FEE_RATE: f64 = 0.001;

/// Slippage estimate. Stablecoins get tighter slippage because their pools
/// are deeper and price impact is lower. This makes stablecoin arb on L2s
/// much more viable (the dominant strategy).
fn slippage_rate(token_symbol: &str) -> f64 {
    let upper = token_symbol.to_ascii_uppercase();
    match upper.as_str() {
        "USDC" | "USDT" | "DAI" | "BUSD" | "FRAX" | "LUSD" | "TUSD" | "USDP" => 0.001, // 0.1% for stables
        _ => 0.005, // 0.5% for volatile
    }
}

/// Pick the cheapest flash-loan source for a token.
///
/// `notional_usd` is the borrowed principal in USD. The fee is that principal
/// times the lender's rate, and it is a *real* cost that was previously hard
/// coded to `0.0` with the comment "calculated in context" - nothing ever
/// calculated it, so every opportunity was scored with a free loan and looked
/// better than it was. On a 1,000 USD trade that silently added 0.03-0.05% of
/// headroom back to the spread, which is larger than most real arbitrage.
///
/// The `prices` slice is deliberately unused: the fee that matters is the
/// lender's rate, which depends on the token, not on observed venue prices.
fn estimate_flash_loan_fee(
    _prices: &[TokenPrice],
    token: &str,
    notional_usd: f64,
) -> FlashLoanRecommendation {
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
        }
    }

    /// The regression this pins: the fee used to be hard-coded to 0.0, so every
    /// opportunity was costed as if the loan were free.
    #[test]
    fn flash_loan_fee_is_charged_on_the_notional() {
        let prices = vec![token_price("USDC")];
        let r = estimate_flash_loan_fee(&prices, "USDC", 10_000.0);
        assert!(
            r.fee_usd > 0.0,
            "fee must be non-zero: a free loan inflates every spread"
        );
        // Cheapest standard lender is Radiant at 0.03%.
        assert_eq!(r.fee_pct, 0.03, "expected Radiant to win at 0.03%");
        assert!(
            (r.fee_usd - 3.0).abs() < 1e-9,
            "0.03% of 10,000 is 3.00, got {}",
            r.fee_usd
        );
    }

    /// Spark charges nothing on DAI, so a DAI loan really is free - the fix must
    /// not invent a fee where the protocol has none.
    #[test]
    fn dai_flash_loan_is_free_on_spark() {
        let prices = vec![token_price("DAI")];
        let r = estimate_flash_loan_fee(&prices, "DAI", 10_000.0);
        assert_eq!(r.fee_pct, 0.0);
        assert_eq!(r.fee_usd, 0.0);
    }

    /// The fee must scale with the loan: a fixed absolute fee would make large
    /// trades look free and small ones look ruinous.
    #[test]
    fn flash_loan_fee_scales_linearly() {
        let prices = vec![token_price("USDC")];
        let small = estimate_flash_loan_fee(&prices, "USDC", 1_000.0);
        let large = estimate_flash_loan_fee(&prices, "USDC", 10_000.0);
        assert!((large.fee_usd / small.fee_usd - 10.0).abs() < 1e-9);
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
