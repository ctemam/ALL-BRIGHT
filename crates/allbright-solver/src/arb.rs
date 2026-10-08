//! Candidate generation: cyclic arbitrage across pools quoting the same
//! token pair (2-hop), plus triangular 3-hop cycles through the pool
//! graph. Matching is by token ADDRESS — never symbol — so bridged
//! variants (USDC vs USDC.e) cannot collide.
//!
//! Every candidate is sized by ternary search on the concave profit
//! function, evaluated with the shared AMM math in allbright-core.

use std::collections::HashMap;

use allbright_core::amm;
use allbright_core::events::{Chain, PoolState, SwapLeg};

pub struct Candidate {
    pub chain: Chain,
    pub route: Vec<SwapLeg>,
    /// input token — contract address.
    pub input_token: String,
    /// display symbol for USD pricing.
    pub input_symbol: String,
    pub optimal_input: f64,
}

/// All profitable-ish cyclic candidates across the pool set.
/// Pools are keyed by address; only same-chain routes are generated.
pub fn candidates(pools: &HashMap<String, PoolState>) -> Vec<Candidate> {
    let mut out = Vec::new();
    for chain in allbright_core::events::Chain::ALL {
        let list: Vec<&PoolState> =
            pools.values().filter(|p| p.chain == chain).collect();
        two_hop(&list, pools, &mut out);
        three_hop(&list, pools, &mut out);
    }
    out
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

/// Size and, if profitable, push a candidate for `route`.
fn try_route(
    pools: &HashMap<String, PoolState>,
    chain: Chain,
    route: Vec<SwapLeg>,
    out: &mut Vec<Candidate>,
) {
    let Some(first_pool) = pools.get(&route[0].pool_address) else { return };
    let tin = route[0].token_in.clone();
    let Some((r_in, _)) = amm::effective_reserves(first_pool, &tin) else {
        return;
    };
    if r_in <= 0.0 {
        return;
    }

    let lookup = |a: &str| pools.get(a);
    let f = |x: f64| -> f64 {
        match amm::route_out(&lookup, &route, &tin, x) {
            Some((fin, _)) => fin - x,
            None => f64::NEG_INFINITY,
        }
    };

    // Cap at 2% of the first leg's input-side effective reserve.
    let (mut lo, mut hi) = (r_in * 1e-5, r_in * 0.02);
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
    if f(x) <= 0.0 {
        return;
    }
    let Some(input_symbol) = symbol_of(pools, &tin).map(str::to_string) else {
        return;
    };
    out.push(Candidate {
        chain,
        route,
        input_token: tin,
        input_symbol,
        optimal_input: x,
    });
}

/// 2-hop: pool A sells X→Y, pool B sells Y→X, different pool addresses.
fn two_hop(
    list: &[&PoolState],
    pools: &HashMap<String, PoolState>,
    out: &mut Vec<Candidate>,
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
                    out,
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
    out: &mut Vec<Candidate>,
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
                        out,
                    );
                }
            }
        }
    }
}
