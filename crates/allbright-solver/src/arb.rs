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
    negative_cycles(&list, pools, moved, &mut out, &mut incoherent);
    // Dedup across generators (BF can rediscover enumerated cycles),
    // then keep the best-K by estimated gross — prevents a pool-update
    // burst from flooding the sim link (broadcast lag wedges subscribers).
    let mut seen = std::collections::HashSet::new();
    out.retain(|c| seen.insert(route_sig(&c.route)));
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

/// Minimum repricing tailwind (bps): the buy leg's pool must price the
/// asset below CEX fair mid by this much — otherwise the entry is at or
/// above fair value and the strike buys the overpriced side.
const PRED_TAILWIND_BPS: f64 = 10.0;

/// P09 — predictive stale-quote strikes (CEX-leadership / whack-a-mole
/// loop): the CEX mid is the forecast of where DEX quotes are heading —
/// liquidity moves there first, DEX pools reprice after. A pool whose
/// implied USD price sits BELOW the mid is stale in the profitable
/// direction: buy it now, it reprices UP. The exit leg is a same-pair
/// pool on a different venue priced higher — the route still settles
/// atomically (no legged CEX hedge — that's outside allbright's model).
///
/// Unlike the P03 move-triggered rescan, this fires on the persistent
/// gap between pool price and fair value: a pool that never repriced is
/// a standing target even while the CEX sits still.
///
/// `usd_of` maps a quote-token symbol to its USD unit price (solver
/// pricing module). Candidates carry strategy "P09" for attribution.
pub fn predictive_strikes(
    pools: &HashMap<String, PoolState>,
    chain: Chain,
    asset_syms: &std::collections::HashSet<String>,
    cex_mid_usd: f64,
    usd_of: &dyn Fn(&str) -> f64,
) -> Vec<Candidate> {
    if cex_mid_usd <= 0.0 {
        return Vec::new();
    }
    // Implied USD price of the CEX-mapped asset per pool.
    let mut implied: HashMap<String, f64> = HashMap::new();
    for p in pools.values() {
        if p.chain != chain {
            continue;
        }
        let base_is_t0 = asset_syms.contains(&p.token0.symbol.to_ascii_uppercase());
        let base_is_t1 = asset_syms.contains(&p.token1.symbol.to_ascii_uppercase());
        if !base_is_t0 && !base_is_t1 {
            continue;
        }
        let quote_sym = if base_is_t0 { &p.token1.symbol } else { &p.token0.symbol };
        let quote_usd = usd_of(quote_sym);
        let px = p.spot_price(); // token1 per token0 (human units)
        if quote_usd <= 0.0 || px <= 0.0 {
            continue;
        }
        let usd = if base_is_t0 { px } else { 1.0 / px } * quote_usd;
        if usd.is_finite() && usd > 0.0 {
            implied.insert(p.address.clone(), usd);
        }
    }
    let tail = cex_mid_usd * (PRED_TAILWIND_BPS / 1e4);
    let mut out = Vec::new();
    let mut incoherent = 0u64;
    let moved = std::collections::HashSet::new();
    for (addr, &buy_px) in &implied {
        // Entry must be stale-LOW vs fair — the repricing tailwind is
        // the predicted edge, not just the measured spread.
        if buy_px >= cex_mid_usd - tail {
            continue;
        }
        let Some(bp) = pools.get(addr) else { continue };
        for (eaddr, &sell_px) in &implied {
            if eaddr == addr || sell_px <= buy_px {
                continue;
            }
            let Some(ep) = pools.get(eaddr) else { continue };
            // Atomicity: exit leg must pay out in the input token —
            // same token pair on a different pool.
            if !same_pair(bp, ep) {
                continue;
            }
            // Direction: input = quote side of the buy pool.
            let (tin, tout) = if asset_syms
                .contains(&bp.token0.symbol.to_ascii_uppercase())
            {
                (bp.token1.address.as_str(), bp.token0.address.as_str())
            } else {
                (bp.token0.address.as_str(), bp.token1.address.as_str())
            };
            try_route(
                pools,
                chain,
                vec![leg(bp, tin, tout), leg(ep, tout, tin)],
                &moved,
                &mut out,
                &mut incoherent,
            );
        }
    }
    for c in &mut out {
        c.strategy = "P09";
    }
    if out.len() > MAX_CANDIDATES_PER_SCAN {
        out.sort_by(|a, b| {
            b.est_profit.partial_cmp(&a.est_profit).unwrap_or(std::cmp::Ordering::Equal)
        });
        out.truncate(MAX_CANDIDATES_PER_SCAN);
    }
    out
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

/// Maximum legs in a Bellman-Ford-discovered cycle — deeper paths pay
/// more gas and drift further between quote and inclusion.
const MAX_BF_LEGS: usize = 4;
/// Negative-cycle extractions per scan: each round masks the found
/// cycle's edges so the next pass surfaces a different cycle.
const MAX_BF_CYCLES: usize = 8;

/// Canonical route signature — the token_in sequence rotated so its
/// smallest address leads. Cycles found at different starting tokens,
/// and BF/redundant scans of the same route, collapse to one key.
fn route_sig(route: &[SwapLeg]) -> String {
    let n = route.len();
    if n == 0 {
        return String::new();
    }
    let mut best = String::new();
    for r in 0..n {
        let mut sig = String::new();
        for k in 0..n {
            let l = &route[(r + k) % n];
            sig.push_str(&l.pool_address);
            sig.push(':');
            sig.push_str(&l.token_in);
            sig.push(';');
        }
        if best.is_empty() || sig < best {
            best = sig;
        }
    }
    best
}

/// U10 — Bellman-Ford-Moore negative-cycle discovery (DeFiPoser /
/// tycho-searcher pattern): tokens are vertices, each pool contributes
/// two directed edges weighted `-ln(spot_rate × (1 - fee))`; a
/// negative-sum cycle is a profitable closed route. Only cycle-FINDING
/// uses log weights — spot-rate candidates are hints; sizing and
/// viability still run through `try_route` and the shared funnel.
/// Catches cycles the brute-force scans structurally miss (4-hop and
/// non-enumerated paths).
fn negative_cycles(
    list: &[&PoolState],
    pools: &HashMap<String, PoolState>,
    moved: &std::collections::HashSet<String>,
    out: &mut Vec<Candidate>,
    incoherent: &mut u64,
) {
    struct Edge {
        from: usize,
        to: usize,
        w: f64,
        pool: usize,
        tin: String,
        tout: String,
    }
    let mut tidx: HashMap<&str, usize> = HashMap::new();
    let mut edges: Vec<Edge> = Vec::new();
    fn push_edge<'k>(
        tidx: &mut HashMap<&'k str, usize>,
        edges: &mut Vec<Edge>,
        pool: usize,
        tin: &'k str,
        tout: &'k str,
        rate: f64,
    ) {
        let n = tidx.len();
        let from = *tidx.entry(tin).or_insert(n);
        let n = tidx.len();
        let to = *tidx.entry(tout).or_insert(n);
        edges.push(Edge {
            from,
            to,
            w: -rate.ln(),
            pool,
            tin: tin.to_string(),
            tout: tout.to_string(),
        });
    }
    for (pi, p) in list.iter().enumerate() {
        let px = p.spot_price(); // token1 per token0 (human units)
        if !px.is_finite() || px <= 0.0 {
            continue;
        }
        let keep = 1.0 - p.fee_bps as f64 / 1e4;
        if keep <= 0.0 {
            continue;
        }
        push_edge(&mut tidx, &mut edges, pi, &p.token0.address, &p.token1.address, px * keep);
        push_edge(&mut tidx, &mut edges, pi, &p.token1.address, &p.token0.address, keep / px);
    }
    let v = tidx.len();
    if v < 2 || edges.is_empty() {
        return;
    }
    let mut masked = vec![false; edges.len()];
    let mut seen = std::collections::HashSet::new();

    for _ in 0..MAX_BF_CYCLES {
        // Super-source trick: all distances start at 0 — any negative
        // cycle in the graph is detected, not only root-reachable ones.
        let mut dist = vec![0.0f64; v];
        let mut prev: Vec<Option<usize>> = vec![None; v];
        let mut hit: Option<usize> = None;
        for _round in 0..v {
            hit = None;
            for (i, e) in edges.iter().enumerate() {
                if masked[i] {
                    continue;
                }
                if dist[e.from] + e.w < dist[e.to] - 1e-12 {
                    dist[e.to] = dist[e.from] + e.w;
                    prev[e.to] = Some(i);
                    hit = Some(e.to);
                }
            }
            if hit.is_none() {
                break;
            }
        }
        let Some(mut y) = hit else { break };
        // Walk V predecessors to land strictly inside the cycle.
        let mut ok = true;
        for _ in 0..v {
            match prev[y] {
                Some(ei) => y = edges[ei].from,
                None => { ok = false; break }
            }
        }
        if !ok {
            break;
        }
        // Collect the closed walk y → y in edge order.
        let mut cyc: Vec<usize> = Vec::new();
        let mut cur = y;
        let mut closed = false;
        while cyc.len() <= v {
            let Some(ei) = prev[cur] else { break };
            cyc.push(ei);
            cur = edges[ei].from;
            if cur == y {
                closed = true;
                break;
            }
        }
        if !closed || cyc.len() > MAX_BF_LEGS {
            break;
        }
        cyc.reverse();
        for &ei in &cyc {
            masked[ei] = true;
        }

        let route: Vec<SwapLeg> = cyc
            .iter()
            .map(|&ei| leg(list[edges[ei].pool], &edges[ei].tin, &edges[ei].tout))
            .collect();
        // Degenerate cycles reusing one pool price a phantom — require
        // ≥2 distinct pools, same as the brute-force scans.
        let distinct: std::collections::HashSet<&str> =
            route.iter().map(|l| l.pool_address.as_str()).collect();
        if distinct.len() < 2 || !seen.insert(route_sig(&route)) {
            continue;
        }
        let chain = list[edges[cyc[0]].pool].chain;
        try_route(pools, chain, route, moved, out, incoherent);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use allbright_core::events::{DexKind, PoolState, Token};

    fn mkpool(addr: &str, sym_a: (&str, &str), sym_b: (&str, &str), r0: f64, r1: f64) -> PoolState {
        PoolState {
            chain: Chain::Base,
            dex: DexKind::V2,
            venue: format!("v_{addr}"),
            address: addr.into(),
            token0: Token { symbol: sym_a.0.into(), address: sym_a.1.into(), decimals: 18 },
            token1: Token { symbol: sym_b.0.into(), address: sym_b.1.into(), decimals: 18 },
            reserve0: r0,
            reserve1: r1,
            sqrt_price_x96: None,
            liquidity: None,
            fee_bps: 30,
            block: 10,
            ts_ms: now_ms(),
        }
    }

    const WETH: (&str, &str) = ("WETH", "0x4200000000000000000000000000000000000006");
    const USDC: (&str, &str) = ("USDC", "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913");

    fn syms() -> std::collections::HashSet<String> {
        ["WETH".to_string(), "ETH".to_string()].into_iter().collect()
    }

    /// Stale-low pool + repriced pool → one directed P09 strike whose
    /// entry leg is the pool below the CEX fair mid.
    #[test]
    fn stale_low_pool_yields_directed_strike() {
        let mut pools = HashMap::new();
        // fair mid = 3000. Stale pool: 100 WETH / 270k USDC → $2700.
        // Repriced pool: 200 WETH / 620k USDC → $3100.
        pools.insert("0xstale".into(), mkpool("0xstale", USDC, WETH, 270_000.0, 100.0));
        pools.insert("0xfair".into(), mkpool("0xfair", USDC, WETH, 620_000.0, 200.0));
        let usd = |s: &str| if s == "USDC" { 1.0 } else { 0.0 };
        let out = predictive_strikes(&pools, Chain::Base, &syms(), 3000.0, &usd);
        assert!(!out.is_empty(), "stale-low + repriced pools must strike");
        let c = &out[0];
        assert_eq!(c.strategy, "P09");
        // Entry leg is the stale pool selling WETH for USDC.
        assert_eq!(c.route[0].pool_address, "0xstale");
        assert_eq!(c.route[0].token_out, WETH.1);
        assert!(c.est_profit > 0.0);
    }

    /// Entry above fair = buying the overpriced side → no strike even
    /// when a pool-vs-pool spread exists (direction is predictive).
    #[test]
    fn entry_above_mid_is_rejected() {
        let mut pools = HashMap::new();
        // mid = 3000. Pool A above mid ($3100), pool B higher ($3200) —
        // spread exists but the "cheap" entry is already above fair.
        pools.insert("0xa".into(), mkpool("0xa", USDC, WETH, 620_000.0, 200.0));
        pools.insert("0xb".into(), mkpool("0xb", USDC, WETH, 960_000.0, 300.0));
        let usd = |s: &str| if s == "USDC" { 1.0 } else { 0.0 };
        let out = predictive_strikes(&pools, Chain::Base, &syms(), 3000.0, &usd);
        assert!(out.is_empty(), "no strike when the cheapest entry is above mid");
    }
    /// Bellman-Ford finds a profitable 4-hop cycle the brute-force scans
    /// never enumerate — A→B→C→D→A with rates multiplying to ~1.46.
    #[test]
    fn bf_finds_four_hop_negative_cycle() {
        let mut pools = HashMap::new();
        pools.insert("0xp1".into(), mkpool("0xp1", ("TA","0xta"), ("TB","0xtb"), 1000.0, 1100.0));
        pools.insert("0xp2".into(), mkpool("0xp2", ("TB","0xtb"), ("TC","0xtc"), 1000.0, 1100.0));
        pools.insert("0xp3".into(), mkpool("0xp3", ("TC","0xtc"), ("TD","0xtd"), 1000.0, 1100.0));
        pools.insert("0xp4".into(), mkpool("0xp4", ("TD","0xtd"), ("TA","0xta"), 1000.0, 1100.0));
        let list: Vec<&PoolState> = pools.values().collect();
        let mut out = Vec::new();
        let mut incoh = 0u64;
        negative_cycles(&list, &pools, &std::collections::HashSet::new(), &mut out, &mut incoh);
        assert!(!out.is_empty(), "4-hop negative cycle must surface a candidate");
        let c = &out[0];
        assert_eq!(c.route.len(), 4);
        assert_eq!(c.strategy, "P02");
        // The route is closed: every leg's out feeds the next leg's in.
        for i in 0..4 {
            assert_eq!(c.route[i].token_out, c.route[(i + 1) % 4].token_in);
        }
    }

    /// No negative cycle → no candidates (honest empty, not a fabricated
    /// route).
    #[test]
    fn bf_empty_when_no_negative_cycle() {
        let mut pools = HashMap::new();
        pools.insert("0xp1".into(), mkpool("0xp1", ("TA","0xta"), ("TB","0xtb"), 1000.0, 1000.0));
        pools.insert("0xp2".into(), mkpool("0xp2", ("TB","0xtb"), ("TC","0xtc"), 1000.0, 1000.0));
        let list: Vec<&PoolState> = pools.values().collect();
        let mut out = Vec::new();
        let mut incoh = 0u64;
        negative_cycles(&list, &pools, &std::collections::HashSet::new(), &mut out, &mut incoh);
        assert!(out.is_empty());
    }
}
