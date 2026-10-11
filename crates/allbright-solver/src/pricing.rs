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
/// STABLE LEGS ONLY: the executor converts the USD floor into a
/// token-denominated minProfit strictly through verified stable pairs —
/// a WETH-leg quote would inflate expected_usd while guaranteeing the
/// downstream refuse. Quote off the pool with the deepest USD-side
/// liquidity so a dust pool can't skew the result.
/// Returns 0.0 when no usable priced pool exists — the caller still
/// rejects with NoUsdPricing, preserving honest accounting.
pub fn usd_per_unit_via_pools(
    token: &str,
    chain: Chain,
    pools: &HashMap<String, PoolState>,
    _cex: &HashMap<String, CexTick>,
    _fallback_native_usd: f64,
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
        // Address-verified stable — a symbol match alone would let a fake
        // "USDC" token inject a bait quote (executor checks addresses too).
        let leg_usd = if allbright_core::chain::stable_addr_str(chain, &leg.address) {
            1.0
        } else {
            continue; // non-stable legs can't back an executable quote
        };
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

#[cfg(test)]
mod tests {
    use super::*;
    use allbright_core::events::{DexKind, Token};

    fn pool(tok_a: (&str, &str), tok_b: (&str, &str), r0: f64, r1: f64) -> PoolState {
        PoolState {
            chain: Chain::Base,
            dex: DexKind::V2,
            venue: "test_v2".into(),
            address: "0x00000000000000000000000000000000000000aa".into(),
            token0: Token { symbol: tok_a.0.into(), address: tok_a.1.into(), decimals: 18 },
            token1: Token { symbol: tok_b.0.into(), address: tok_b.1.into(), decimals: 6 },
            reserve0: r0,
            reserve1: r1,
            sqrt_price_x96: None,
            liquidity: None,
            fee_bps: 30,
            block: 1,
            ts_ms: 0,
        }
    }

    const WETH_BASE: &str = "0x4200000000000000000000000000000000000006";
    const USDC_BASE: &str = "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913";
    const GHOST: &str = "0x77870000000000000000000000000000000000aa";

    /// The $991M phantom class: a token priced ONLY through a WETH leg must
    /// be unpriceable — the executor can't mint a stable-backed minProfit
    /// for it, so solver must refuse at source (NoUsdPricing).
    #[test]
    fn weth_only_leg_yields_no_usd_price() {
        let mut pools = HashMap::new();
        pools.insert("p1".into(), pool(
            ("GHOST", GHOST), ("WETH", WETH_BASE), 80_000.0, 40.0,
        ));
        let cex = HashMap::new();
        let px = usd_per_unit_via_pools("GHOST", Chain::Base, &pools, &cex, 3_000.0);
        assert_eq!(px, 0.0, "WETH leg must not back a USD quote");
    }

    /// A canonical stable leg does back the quote — spot-derived price.
    #[test]
    fn stable_leg_yields_usd_price() {
        let mut pools = HashMap::new();
        pools.insert("p1".into(), pool(
            ("GHOST", GHOST), ("USDC", USDC_BASE), 1_000.0, 2_000.0,
        ));
        let cex = HashMap::new();
        let px = usd_per_unit_via_pools("GHOST", Chain::Base, &pools, &cex, 0.0);
        assert!((px - 2.0).abs() < 1e-9, "expected $2/unit, got {px}");
    }

    /// Symbol spoof: a token *named* USDC at a non-canonical address must
    /// not inject a quote — the executor verifies addresses, so do we.
    #[test]
    fn spoofed_stable_symbol_rejected() {
        let fake_usdc = "0xdead00000000000000000000000000000000beef";
        let mut pools = HashMap::new();
        pools.insert("p1".into(), pool(
            ("GHOST", GHOST), ("USDC", fake_usdc), 1_000.0, 2_000.0,
        ));
        let cex = HashMap::new();
        let px = usd_per_unit_via_pools("GHOST", Chain::Base, &pools, &cex, 0.0);
        assert_eq!(px, 0.0, "fake-USDC address must not back a quote");
    }
}
