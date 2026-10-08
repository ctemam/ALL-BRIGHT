//! AMM math — independent reimplementation of publicly documented
//! Uniswap-V2 reserve math and V3 concentrated-liquidity math.
//! Signal-level precision (f64); the on-chain executor contract is the
//! authority on exact output.
//!
//! Units: reserves are human (decimal-adjusted) units; V3 liquidity/√P
//! arrive as raw on-chain values and are converted internally.

use crate::events::{DexKind, PoolState};

/// V2 constant-product swap with fee in bps.
/// `in`/`reserves` in human (decimal-adjusted) units.
pub fn v2_out(amount_in: f64, r_in: f64, r_out: f64, fee_bps: u32) -> f64 {
    if amount_in <= 0.0 || r_in <= 0.0 || r_out <= 0.0 {
        return 0.0;
    }
    let fee = (10_000 - fee_bps) as f64;
    let in_net = amount_in * fee;
    in_net * r_out / (r_in * 10_000.0 + in_net)
}

/// Solidly-style stable swap: invariant k = x³y + xy³ (Velodrome/Ramses/
/// Etherex sAMM). Reserves and amounts in human units — the degree-4
/// invariant is homogeneous, so human units scale identically to the
/// e18-normalized on-chain values. Fee applies to the input.
/// Newton-solves y' from x'·y'³ + x'³·y' = k.
pub fn solidly_out(amount_in: f64, r_in: f64, r_out: f64, fee_bps: u32) -> f64 {
    if amount_in <= 0.0 || r_in <= 0.0 || r_out <= 0.0 {
        return 0.0;
    }
    let in_net = amount_in * (10_000.0 - fee_bps as f64) / 10_000.0;
    let x = r_in + in_net;
    let k = r_in.powi(3) * r_out + r_in * r_out.powi(3);
    // f(y) = x·y³ + x³·y − k;  f'(y) = 3x·y² + x³.  Start at current y.
    let mut y = r_out;
    for _ in 0..64 {
        let f = x * y.powi(3) + x.powi(3) * y - k;
        let fp = 3.0 * x * y.powi(2) + x.powi(3);
        if fp <= 0.0 {
            return 0.0;
        }
        let next = y - f / fp;
        if (next - y).abs() < 1e-12 * y.max(1.0) {
            y = next;
            break;
        }
        y = next.max(0.0);
    }
    if y >= r_out || y <= 0.0 {
        return 0.0;
    }
    r_out - y
}

/// V3 single-tick-range approximation using in-range liquidity and √P.
/// `zero_for_one`: true when selling token0 for token1.
/// `amount_in` in human units; returns human units.
pub fn v3_out(pool: &PoolState, amount_in: f64, zero_for_one: bool) -> f64 {
    let (Some(sqrt_p), Some(l)) = (pool.sqrt_price_x96, pool.liquidity) else {
        return 0.0;
    };
    if l <= 0.0 || sqrt_p <= 0.0 || amount_in <= 0.0 {
        return 0.0;
    }
    let (d_in, d_out) = if zero_for_one {
        (pool.token0.decimals, pool.token1.decimals)
    } else {
        (pool.token1.decimals, pool.token0.decimals)
    };
    // The pool math operates on raw integer units — convert.
    let in_raw = amount_in * 10f64.powi(d_in as i32)
        * (10_000.0 - pool.fee_bps as f64) / 10_000.0;
    // √P as stored is Q96-fixed of the RAW price (token1_raw per token0_raw).
    let p = sqrt_p / 2f64.powi(96);
    let out_raw = if zero_for_one {
        // Sell token0: √P' = √P·L/(L + Δx·√P); out = L·(√P − √P')
        let denom = l + in_raw * p;
        if denom <= 0.0 {
            return 0.0;
        }
        l * (p - p * l / denom)
    } else {
        // Sell token1: √P' = √P + Δy/L; out = L·(1/√P − 1/√P')
        let p2 = p + in_raw / l;
        if p2 <= 0.0 {
            return 0.0;
        }
        l * (p2 - p) / (p * p2)
    };
    if out_raw <= 0.0 {
        return 0.0;
    }
    out_raw / 10f64.powi(d_out as i32)
}

/// Effective reserves of a pool for sizing/depth estimation, in human
/// units. For V3 the in-range virtual reserves are L/√P and L·√P.
/// Returns (r_in, r_out) given `token_in` (address or symbol).
pub fn effective_reserves(pool: &PoolState, token_in: &str) -> Option<(f64, f64)> {
    let zero_for_one = matches_token(pool, token_in, true);
    match pool.dex {
        DexKind::V2 => {
            let (a, b) = (pool.reserve0, pool.reserve1);
            if a <= 0.0 || b <= 0.0 {
                return None;
            }
            Some(if zero_for_one { (a, b) } else { (b, a) })
        }
        DexKind::V3 => {
            let (sqrt_p, l) = (pool.sqrt_price_x96?, pool.liquidity?);
            if l <= 0.0 || sqrt_p <= 0.0 {
                return None;
            }
            let p = sqrt_p / 2f64.powi(96);
            let r0 = l / p / 10f64.powi(pool.token0.decimals as i32);
            let r1 = l * p / 10f64.powi(pool.token1.decimals as i32);
            Some(if zero_for_one { (r0, r1) } else { (r1, r0) })
        }
        DexKind::Stable => {
            let (a, b) = (pool.reserve0, pool.reserve1);
            if a <= 0.0 || b <= 0.0 {
                return None;
            }
            Some(if zero_for_one { (a, b) } else { (b, a) })
        }
    }
}

/// Does `tok` (address or symbol, case-insensitive) name the pool's
/// token0 (zfo=true) or token1 (zfo=false)?
pub fn matches_token(pool: &PoolState, tok: &str, token0: bool) -> bool {
    let t = if token0 { &pool.token0 } else { &pool.token1 };
    tok.eq_ignore_ascii_case(&t.address) || tok.eq_ignore_ascii_case(&t.symbol)
}

/// Evaluate one swap leg. `token_in` may be an address or symbol.
/// Returns (out_amount, out_token_address) in human units.
pub fn leg_out(
    pool: &PoolState,
    token_in: &str,
    amount_in: f64,
) -> Option<(f64, String)> {
    let zero_for_one = matches_token(pool, token_in, true);
    if !zero_for_one && !matches_token(pool, token_in, false) {
        return None;
    }
    let out = match pool.dex {
        DexKind::V2 => {
            let (r_in, r_out) = effective_reserves(pool, token_in)?;
            v2_out(amount_in, r_in, r_out, pool.fee_bps)
        }
        DexKind::V3 => v3_out(pool, amount_in, zero_for_one),
        DexKind::Stable => {
            let (r_in, r_out) = effective_reserves(pool, token_in)?;
            solidly_out(amount_in, r_in, r_out, pool.fee_bps)
        }
    };
    if out <= 0.0 {
        return None;
    }
    let out_tok = if zero_for_one { &pool.token1 } else { &pool.token0 };
    Some((out, out_tok.address.clone()))
}

/// Fold a full route. Returns (final_amount, final_token_address).
pub fn route_out<'a, I>(
    pools: I,
    route: &[crate::events::SwapLeg],
    token_in: &str,
    amount_in: f64,
) -> Option<(f64, String)>
where
    I: Fn(&str) -> Option<&'a PoolState>,
{
    let mut amount = amount_in;
    let mut token = token_in.to_string();
    for leg in route {
        let pool = pools(&leg.pool_address)?;
        let (out, out_addr) = leg_out(pool, &token, amount)?;
        amount = out;
        token = out_addr;
    }
    Some((amount, token))
}

/// Gas estimate per leg (router-level heuristics; tuned by research loop).
pub fn gas_estimate(legs: usize, has_v3: bool) -> u64 {
    let base = 60_000u64;
    let per_leg_v2 = 90_000u64;
    let per_leg_v3 = 130_000u64;
    base + if has_v3 {
        legs as u64 * per_leg_v3
    } else {
        legs as u64 * per_leg_v2
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{Chain, Token};

    fn pool(r0: f64, r1: f64) -> PoolState {
        PoolState {
            chain: Chain::Bsc,
            dex: DexKind::V2,
            venue: "t".into(),
            address: "0x0".into(),
            token0: Token { symbol: "A".into(), address: "0xa".into(), decimals: 18 },
            token1: Token { symbol: "B".into(), address: "0xb".into(), decimals: 18 },
            reserve0: r0,
            reserve1: r1,
            sqrt_price_x96: None,
            liquidity: None,
            fee_bps: 30,
            block: 1,
            ts_ms: 0,
        }
    }

    #[test]
    fn v2_spot_and_impact() {
        let p = pool(1000.0, 1000.0);
        let small = v2_out(1.0, p.reserve0, p.reserve1, 30);
        assert!((small - 0.997).abs() < 0.001);
        let big = v2_out(500.0, p.reserve0, p.reserve1, 30);
        assert!(big < 500.0 * 0.997);
    }

    #[test]
    fn v3_conserves_value_at_spot() {
        // At √P=1 in raw units with equal decimals, selling 1 token0 of a
        // deep pool should yield ≈1 token1 minus fee.
        let mut p = pool(0.0, 0.0);
        p.dex = DexKind::V3;
        p.sqrt_price_x96 = Some(2f64.powi(96)); // √P = 1
        p.liquidity = Some(1e24); // deep: 1e18-unit input is negligible
        let out = v3_out(&p, 1.0, true);
        assert!((out - 0.997).abs() < 0.01, "got {out}");
        let back = v3_out(&p, 1.0, false);
        assert!((back - 0.997).abs() < 0.01, "got {back}");
    }

    #[test]
    fn v3_price_moves_correctly() {
        // Pool priced 2000 raw token1 per token0 (raw decimals equal).
        let mut p = pool(0.0, 0.0);
        p.dex = DexKind::V3;
        p.sqrt_price_x96 = Some(2f64.powi(96) * 2000f64.sqrt());
        p.liquidity = Some(1e24);
        // Selling one token0 should net ≈2000 token1 (minus fee/impact).
        let out = v3_out(&p, 1.0, true);
        assert!(out > 1900.0 && out < 2000.0, "got {out}");
        // Selling 2000 token1 should net ≈1 token0.
        let back = v3_out(&p, 2000.0, false);
        assert!(back > 0.95 && back < 1.0, "got {back}");
    }

    #[test]
    fn solidly_stable_near_peg() {
        // Balanced sAMM: a small swap should clear ≈1:1 minus the fee —
        // far tighter than constant-product would price the same pool.
        let mut p = pool(1_000_000.0, 1_000_000.0);
        p.dex = DexKind::Stable;
        p.fee_bps = 5; // Velodrome sAMM fees are low
        let (out, _) = leg_out(&p, "0xa", 1_000.0).unwrap();
        assert!((out - 999.5).abs() < 1.0, "got {out}");
        // And a large swap should still be much better than xy=k impact.
        let (big, _) = leg_out(&p, "0xa", 100_000.0).unwrap();
        let xy_k = v2_out(100_000.0, 1_000_000.0, 1_000_000.0, 5);
        assert!(big > xy_k, "stable {big} should beat xy=k {xy_k}");
    }

    #[test]
    fn route_folds() {
        let p = pool(1000.0, 2000.0);
        let legs = vec![crate::events::SwapLeg {
            venue: "t".into(),
            pool_address: "0x0".into(),
            token_in: "0xa".into(),
            token_out: "0xb".into(),
            dex: DexKind::V2,
        }];
        let (out, addr) = route_out(|_| Some(&p), &legs, "0xa", 10.0).unwrap();
        assert_eq!(addr, "0xb");
        assert!(out > 19.0 && out < 20.0);
    }
}
