//! Token → USD pricing. Live CEX ticks are the primary oracle (Binance);
//! stablecoins are treated as $1; `ZCA_NATIVE_USD` is the configured
//! fallback for wrapped-native units when no tick exists.

use std::collections::HashMap;

use allbright_core::events::{CexTick, Chain};

const STABLES: [&str; 8] =
    ["USDT", "USDC", "BUSD", "DAI", "FDUSD", "USDe", "USDB", "USDC.e"];

/// CEX symbol that prices a token symbol (wrapped natives + majors).
fn cex_symbol(token: &str) -> Option<&'static str> {
    match token.to_ascii_uppercase().as_str() {
        "WETH" | "ETH" => Some("ETHUSDT"),
        "WBNB" | "BNB" => Some("BNBUSDT"),
        "WBTC" | "BTC" => Some("BTCUSDT"),
        "WMATIC" | "MATIC" | "POL" => Some("MATICUSDT"),
        "WAVAX" | "AVAX" => Some("AVAXUSDT"),
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
