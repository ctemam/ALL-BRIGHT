//! Candidate generation: cyclic arbitrage across pools quoting the same
//! token pair (2-hop), plus triangular 3-hop cycles through the pool
//! graph. Matching is by token ADDRESS — never symbol — so bridged
//! variants (USDC vs USDC.e) cannot collide.
//!
//! Every candidate is sized by ternary search on the concave profit
//! function, evaluated with the shared AMM math in allbright-core.

use std::collections::HashMap;

use allbright_core::amm;
use allbright_core::events::{now_ms, Chain, PoolState, SwapLeg};

/// State-coherence bounds for route legs (phantom-edge filter): a route
/// that mixes pool states measured blocks apart prices a divergence that
/// may never have existed at one instant. `block` is the Multicall read
/// height (0 = unknown); `ts_ms` bounds wall-clock staleness regardless.
const MAX_BLOCK_SKEW: u64 = 4;
const MAX_TS_SKEW_MS: u64 = 30_000;
const MAX_POOL_AGE_MS: u64 = 300_000;
/// Dispatch cap per update — flood control, not an economic gate.
const MAX_CANDIDATES_PER_SCAN: usize = 64;
/// Input sizing cap (fraction of first-leg input reserve). Correlated
/// pools get deeper sizing: near-pegged assets on low-fee curves slip
/// far less per unit, so the concave optimum sits further out.
const INPUT_FRAC: f64 = 0.02;
const CORR_INPUT_FRAC: f64 = 0.05;

pub struct Candidate {
    pub chain: Chain,
    /// Strategy ID owning this route — P01/P02 cyclic, P03 divergence,
    /// P04 correlated-universe, P05 reserve-shift backrun.
    pub strategy: &'static str,
    pub route: Vec<SwapLeg>,
    /// input token — contract address.
    pub input_token: String,
    /// display symbol for USD pricing.
    pub input_symbol: String,
    pub optimal_input: f64,
    /// Estimated gross edge in input-token units (ranking only).
    pub est_profit: f64,
}

/// Correlation class for a token symbol. Routes confined to one class
/// get the P04 sizing policy — near-pegged assets price each other with
/// thin spreads, and low-fee stable venues make small divergences
/// executable that a 0.3% CPMM lane can never reach.
fn corr_group(symbol: &str) -> Option<u8> {
    match symbol.to_ascii_uppercase().as_str() {
        "USDC" | "USDT" | "DAI" | "FDUSD" | "BUSD" | "USDE" | "USDB" | "USDBC"
        | "USDT0" | "USD₮0" | "USDC.E" | "USDT.E" | "MIM" | "USDS" | "USD1"
        | "PYUSD" | "FRAX" | "TUSD" | "LUSD" | "GHO" | "CRVUSD" => Some(0),
        "WETH" | "ETH" | "STETH" | "WSTETH" | "RETH" | "CBETH" | "WEETH" | "ETHX"
        | "ANKRETH" | "FRXETH" | "SFRXETH" | "WETH.E" => Some(1),
        "WBTC" | "CBBTC" | "BTC.B" | "TBTC" | "EBTC" | "BTCB" | "LBTC" | "SOLVBTC" => {
            Some(2)
        }
        _ => None,
    }
}

/// Some(group) iff every pool in the route pairs tokens of ONE
/// correlation class — a mixed-class hop disqualifies the route.
fn route_group(pools: &HashMap<String, PoolState>, route: &[SwapLeg]) -> Option<u8> {
    let mut g: Option<u8> = None;
    for leg in route {
        let p = pools.get(&leg.pool_address)?;
        for sym in [&p.token0.symbol, &p.token1.symbol] {
            let cg = corr_group(sym)?;
            match g {
                None => g = Some(cg),
                Some(x) if x == cg => {}
                Some(_) => return None,
            }
        }
    }
    g
}

/// All profitable-ish cyclic candidates for ONE chain — the caller
/// regenerates only the updated chain's subgraph per PoolUpdate.
/// `moved` = pool addresses whose reserves shifted materially since the
/// previous update; routes through them are tagged P05 (reserve-shift
/// backrun — the residual imbalance a real swap just left behind).
/// Returns (candidates, coherence_rejects): the second count is how many
/// structurally-valid routes were dropped because their legs' pool
/// states were incoherent (block/ts skew or staleness).
pub fn candidates(
    pools: &HashMap<String, PoolState>,
    chain: Chain,
    moved: &std::collections::HashSet<String>,
) -> (Vec<Candidate>, u64) {
    let list: Vec<&PoolState> =
        pools.values().filter(|p| p.chain == chain).collect();
    let mut out = Vec::new();
    let mut incoherent = 0u64;
    two_hop(&list, pools, moved, &mut out, &mut incoherent);
    three_hop(&list, pools, moved, &mut out, &mut incoherent);
    // Keep the best-K by estimated gross — prevents a pool-update burst
    // from flooding the sim link (broadcast lag wedges subscribers).
    if out.len() > MAX_CANDIDATES_PER_SCAN {
        out.sort_by(|a, b| {
            b.est_profit.partial_cmp(&a.est_profit).unwrap_or(std::cmp::Ordering::Equal)
        });
        out.truncate(MAX_CANDIDATES_PER_SCAN);
    }
    (out, incoherent)
}

/// P03 trigger subset: same cyclic machinery, but restricted to routes
/// touching a moved asset (any leg's pool contains a token whose symbol
/// is in `asset_syms`). Used when a sharp CEX move implies on-chain
/// pools are stale — re-scan before the next poll cycle.
pub fn candidates_for_asset(
    pools: &HashMap<String, PoolState>,
    chain: Chain,
    asset_syms: &std::collections::HashSet<String>,
) -> (Vec<Candidate>, u64) {
    let list: Vec<&PoolState> =
        pools.values().filter(|p| p.chain == chain).collect();
    let mut out = Vec::new();
    let mut incoherent = 0u64;
    let moved = std::collections::HashSet::new();
    two_hop(&list, pools, &moved, &mut out, &mut incoherent);
    three_hop(&list, pools, &moved, &mut out, &mut incoherent);
    out.retain(|c| {
        c.route.iter().any(|l| {
            pools.get(&l.pool_address).is_some_and(|p| {
                asset_syms.contains(&p.token0.symbol.to_ascii_uppercase())
                    || asset_syms.contains(&p.token1.symbol.to_ascii_uppercase())
            })
        })
    });
    for c in &mut out {
        c.strategy = "P03";
    }
    if out.len() > MAX_CANDIDATES_PER_SCAN {
        out.sort_by(|a, b| {
            b.est_profit.partial_cmp(&a.est_profit).unwrap_or(std::cmp::Ordering::Equal)
        });
        out.truncate(MAX_CANDIDATES_PER_SCAN);
    }
    (out, incoherent)
}

/// True when every leg's pool state is coherent: measured within
/// MAX_BLOCK_SKEW blocks and MAX_TS_SKEW_MS of each other, and no leg
/// older than MAX_POOL_AGE_MS. Block 0 (unknown height) falls back to
/// the wall-clock check only.
fn coherent(pools: &HashMap<String, PoolState>, route: &[SwapLeg]) -> bool {
    let now = now_ms();
    let (mut bmin, mut bmax) = (u64::MAX, 0u64);
    let (mut tmin, mut tmax) = (u64::MAX, 0u64);
    for leg in route {
        let Some(p) = pools.get(&leg.pool_address) else { return false };
        if p.block > 0 {
            bmin = bmin.min(p.block);
            bmax = bmax.max(p.block);
        }
        tmin = tmin.min(p.ts_ms);
        tmax = tmax.max(p.ts_ms);
        if now.saturating_sub(p.ts_ms) > MAX_POOL_AGE_MS {
            return false;
        }
    }
    tmax - tmin <= MAX_TS_SKEW_MS && (bmax == 0 || bmax - bmin <= MAX_BLOCK_SKEW)
}

fn leg(pool: &PoolState, tin: &str, tout: &str) -> SwapLeg {
    SwapLeg {
        venue: pool.venue.clone(),
        pool_address: pool.address.clone(),
        token_in: tin.to_string(),
        token_out: tout.to_string(),
        dex: pool.dex,
    }
}

fn same_pair(a: &PoolState, b: &PoolState) -> bool {
    let a0 = a.token0.address.to_ascii_lowercase();
    let a1 = a.token1.address.to_ascii_lowercase();
    let b0 = b.token0.address.to_ascii_lowercase();
    let b1 = b.token1.address.to_ascii_lowercase();
    (a0 == b0 && a1 == b1) || (a0 == b1 && a1 == b0)
}

fn symbol_of<'a>(pools: &'a HashMap<String, PoolState>, addr: &str) -> Option<&'a str> {
    pools.values().find_map(|p| {
        if p.token0.address.eq_ignore_ascii_case(addr) {
            Some(p.token0.symbol.as_str())
        } else if p.token1.address.eq_ignore_ascii_case(addr) {
            Some(p.token1.symbol.as_str())
        } else {
            None
        }
    })
}

/// Size and, if profitable, push a candidate for `route`. Strategy tag:
/// P05 when the route rides a just-shifted pool, else P04 when the whole
/// route stays inside one correlation class, else P01/P02 by hop count.
fn try_route(
    pools: &HashMap<String, PoolState>,
    chain: Chain,
    route: Vec<SwapLeg>,
    moved: &std::collections::HashSet<String>,
    out: &mut Vec<Candidate>,
    incoherent: &mut u64,
) {
    if !coherent(pools, &route) {
        *incoherent += 1;
        return;
    }
    let Some(first_pool) = pools.get(&route[0].pool_address) else { return };
    let tin = route[0].token_in.clone();
    let Some((r_in, _)) = amm::effective_reserves(first_pool, &tin) else {
        return;
    };
    if r_in <= 0.0 {
        return;
    }

    let moved_hit = route
        .iter()
        .any(|l| moved.contains(l.pool_address.as_str()));
    let correlated = route_group(pools, &route).is_some();
    let strategy = if moved_hit {
        "P05"
    } else if correlated {
        "P04"
    } else if route.len() == 2 {
        "P01"
    } else {
        "P02"
    };

    let lookup = |a: &str| pools.get(a);
    let f = |x: f64| -> f64 {
        match amm::route_out(&lookup, &route, &tin, x) {
            Some((fin, _)) => fin - x,
            None => f64::NEG_INFINITY,
        }
    };

    // Correlated curves tolerate deeper sizing; majors stay at 2%.
    let cap = if correlated { CORR_INPUT_FRAC } else { INPUT_FRAC };
    let (mut lo, mut hi) = (r_in * 1e-5, r_in * cap);
    if hi <= lo {
        return;
    }
    for _ in 0..40 {
        let m1 = lo + (hi - lo) / 3.0;
        let m2 = hi - (hi - lo) / 3.0;
        if f(m1) < f(m2) {
            lo = m1;
        } else {
            hi = m2;
        }
    }
    let x = (lo + hi) / 2.0;
    let est_profit = f(x);
    if est_profit <= 0.0 {
        return;
    }
    let Some(input_symbol) = symbol_of(pools, &tin).map(str::to_string) else {
        return;
    };
    out.push(Candidate {
        chain,
        strategy,
        route,
        input_token: tin,
        input_symbol,
        optimal_input: x,
        est_profit,
    });
}

/// 2-hop: pool A sells X→Y, pool B sells Y→X, different pool addresses.
fn two_hop(
    list: &[&PoolState],
    pools: &HashMap<String, PoolState>,
    moved: &std::collections::HashSet<String>,
    out: &mut Vec<Candidate>,
    incoherent: &mut u64,
) {
    for a in list {
        for b in list {
            if a.address == b.address || !same_pair(a, b) {
                continue;
            }
            // Both directions.
            for (tin, tout) in [
                (a.token0.address.as_str(), a.token1.address.as_str()),
                (a.token1.address.as_str(), a.token0.address.as_str()),
            ] {
                try_route(
                    pools,
                    a.chain,
                    vec![leg(a, tin, tout), leg(b, tout, tin)],
                    moved,
                    out,
                    incoherent,
                );
            }
        }
    }
}

/// 3-hop triangular: a→b on P1, b→c on P2, c→a on P3 — token edges must
/// exist as pools. Pool addresses must be pairwise distinct.
fn three_hop(
    list: &[&PoolState],
    pools: &HashMap<String, PoolState>,
    moved: &std::collections::HashSet<String>,
    out: &mut Vec<Candidate>,
    incoherent: &mut u64,
) {
    // Directed edges: token_in_addr -> Vec<(pool, token_out_addr)>
    let mut edges: HashMap<&str, Vec<(&PoolState, &str)>> = HashMap::new();
    for p in list {
        edges
            .entry(p.token0.address.as_str())
            .or_default()
            .push((p, p.token1.address.as_str()));
        edges
            .entry(p.token1.address.as_str())
            .or_default()
            .push((p, p.token0.address.as_str()));
    }

    for (a_addr, outs_ab) in &edges {
        for (p1, b_addr) in outs_ab {
            let Some(outs_bc) = edges.get(b_addr) else { continue };
            for (p2, c_addr) in outs_bc {
                if p2.address == p1.address || c_addr == a_addr {
                    continue;
                }
                let Some(outs_ca) = edges.get(c_addr) else { continue };
                for (p3, back) in outs_ca {
                    if p3.address == p1.address || p3.address == p2.address {
                        continue;
                    }
                    if *back != *a_addr {
                        continue;
                    }
                    try_route(
                        pools,
                        p1.chain,
                        vec![
                            leg(p1, a_addr, b_addr),
                            leg(p2, b_addr, c_addr),
                            leg(p3, c_addr, a_addr),
                        ],
                        moved,
                        out,
                        incoherent,
                    );
                }
            }
        }
    }
}
