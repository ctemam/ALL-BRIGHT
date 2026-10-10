//! Token → USD pricing. Live CEX ticks are the primary oracle (Binance);
//! stablecoins are treated as $1; `NATIVE_USD` is the configured
//! fallback for wrapped-native units when no tick exists.

use std::collections::HashMap;

use allbright_core::events::{CexTick, Chain, PoolState};

const STABLES: [&str; 13] = [
    "USDT", "USDC", "BUSD", "DAI", "FDUSD", "USDe", "USDB", "USDC.e",
    "USDT.e", "USD₮0", "USDT0", "USDBC", "MIM",
];

/// CEX symbol that prices a token symbol (wrapped natives + majors).
fn cex_symbol(token: &str) -> Option<&'static str> {
    match token.to_ascii_uppercase().as_str() {
        "WETH" | "ETH" | "WETH.E" => Some("ETHUSDT"),
        "WBNB" | "BNB" => Some("BNBUSDT"),
        "WBTC" | "BTC" | "BTC.B" | "CBBTC" => Some("BTCUSDT"),
        "WSTETH" | "STETH" | "RETH" | "CBETH" | "WEETH" => Some("ETHUSDT"),
        // MATIC→POL migration: Binance delisted MATICUSDT; POLUSDT is the live quote.
        "WMATIC" | "MATIC" | "POL" => Some("POLUSDT"),
        "WAVAX" | "AVAX" | "WAVAX.E" => Some("AVAXUSDT"),
        _ => None,
    }
}

/// Native-token wrapper symbol for a chain (used for gas pricing later).
pub fn native_symbol(chain: Chain) -> &'static str {
    match chain {
        Chain::Eth | Chain::Arb | Chain::Op | Chain::Base | Chain::Linea
        | Chain::Scroll | Chain::Blast => "WETH",
        Chain::Bsc => "WBNB",
        Chain::Poly => "WMATIC",
        Chain::Avax => "WAVAX",
    }
}

/// USD per unit of `token`. Prefers the live CEX mid; falls back to the
/// configured native USD price for the chain's wrapped native.
pub fn usd_per_unit(
    token: &str,
    chain: Chain,
    cex: &HashMap<String, CexTick>,
    fallback_native_usd: f64,
) -> f64 {
    let up = token.to_ascii_uppercase();
    if STABLES.iter().any(|s| *s == up) {
        return 1.0;
    }
    if let Some(sym) = cex_symbol(&up) {
        if let Some(t) = cex.get(sym) {
            return (t.bid + t.ask) / 2.0;
        }
    }
    // Wrapped native without a tick → configured fallback.
    if up == native_symbol(chain) {
        return fallback_native_usd;
    }
    0.0
}

/// Pool-derived USD price for tokens without a CEX/stable mapping
/// (fixes the NoUsdPricing blindness — §7 data-blindness class).
/// One hop only: find pools on `chain` pairing `token` with a directly
/// priceable leg (stable or CEX/native-mapped), price the token off the
/// pool's marginal rate, and take the quote from the pool with the
/// deepest USD-side liquidity so a dust pool can't skew the result.
/// Returns 0.0 when no usable priced pool exists — the caller still
/// rejects with NoUsdPricing, preserving honest accounting.
pub fn usd_per_unit_via_pools(
    token: &str,
    chain: Chain,
    pools: &HashMap<String, PoolState>,
    cex: &HashMap<String, CexTick>,
    fallback_native_usd: f64,
) -> f64 {
    let up = token.to_ascii_uppercase();
    let mut best_px = 0.0f64;
    let mut best_depth_usd = 0.0f64;
    for p in pools.values() {
        if p.chain != chain {
            continue;
        }
        // Which leg is the target token, which is the pricing leg?
        let (tgt0, leg0) = if p.token0.symbol.eq_ignore_ascii_case(&up) {
            (true, true) // token0 = target, token1 = pricing leg
        } else if p.token1.symbol.eq_ignore_ascii_case(&up) {
            (false, true) // token1 = target, token0 = pricing leg
        } else {
            continue;
        };
        let _ = leg0;
        let leg = if tgt0 { &p.token1 } else { &p.token0 };
        let leg_usd = usd_per_unit(&leg.symbol, chain, cex, fallback_native_usd);
        if leg_usd <= 0.0 {
            continue; // leg itself unpriced — no recursion (bounded)
        }
        // spot_price() = token1 per token0 (human units).
        let px01 = p.spot_price();
        let tok_usd = if tgt0 {
            if px01 <= 0.0 {
                continue;
            }
            px01 * leg_usd
        } else {
            if px01 <= 0.0 {
                continue;
            }
            leg_usd / px01
        };
        if !tok_usd.is_finite() || tok_usd <= 0.0 {
            continue;
        }
        // Depth proxy: USD value of the pricing leg's side of the pool.
        let leg_res = if tgt0 { p.reserve1 } else { p.reserve0 };
        let depth_usd = leg_res * leg_usd;
        if depth_usd > best_depth_usd {
            best_depth_usd = depth_usd;
            best_px = tok_usd;
        }
    }
    best_px
}
