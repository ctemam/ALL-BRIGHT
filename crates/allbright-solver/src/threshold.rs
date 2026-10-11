//! Adaptive profit floor — EV-gate composite from the proven patterns:
//!
//!   floor(chain) = clamp(base + gas_mult × gas_ewma − credit, floor_min, cap)
//!
//!   gas_ewma — EWMA of live per-chain gas cost (searcher break-even
//!              pattern: the floor must clear the CURRENT cost regime with
//!              margin, not a static guess — ON1Builder/EvEstimator class).
//!   credit   — EWMA of challenger counterfactual-net on `challenged=true`
//!              verdicts (Freqtrade-protection-style lookback adaptation:
//!              rejects proven profitable by independent counterfactual
//!              economics mean the floor sits too high for this chain).
//!
//! The floor NEVER goes below `floor_min` (hard safety) or above `cap`.
//! Every Rejected/Opportunity record carries the floor used — attribution
//! stays honest; the standard `MIN_PROFIT_USD` remains the base, not a
//! hidden override.

use std::collections::HashMap;

use allbright_core::events::Chain;

/// EWMA smoothing factor — matches the challenger's per-verdict cadence;
/// ~20 verdicts to move the credit meaningfully (≈ Freqtrade's lookback
/// window role, applied continuously rather than in candles).
const ALPHA: f64 = 0.05;

/// Per-chain adaptive state.
#[derive(Default)]
struct Lane {
    /// EWMA of counterfactual net on challenged=true verdicts (USD).
    credit: f64,
    /// EWMA of simulated gas cost (USD) — tracks the live gas regime.
    gas_ewma: f64,
    /// Evidence count — diagnostics only.
    n: u64,
}

pub struct AdaptiveFloor {
    lanes: HashMap<Chain, Lane>,
    /// Base floor — the configured MIN_PROFIT_USD.
    base: f64,
    /// Multiplier on gas_ewma (cost-regime margin). Searcher convention:
    /// require ≥ ~1.5× the live attempt cost before firing.
    gas_mult: f64,
    /// Hard lower bound — floor never adapts below this.
    floor_min: f64,
    /// Hard upper bound — runaway credit can never push the bar up
    /// unboundedly, and a wild gas regime can never freeze trading.
    cap: f64,
    enabled: bool,
}

impl AdaptiveFloor {
    pub fn new(base: f64, gas_mult: f64, floor_min: f64, cap: f64, enabled: bool) -> Self {
        Self { lanes: HashMap::new(), base, gas_mult, floor_min, cap, enabled }
    }

    /// Challenger verdict arrived: challenged=true means a rejected
    /// candidate was counterfactually profitable — grow the credit that
    /// lowers this chain's floor. A false verdict decays it.
    pub fn note_verdict(&mut self, chain: Chain, challenged: bool, cf_net_usd: f64) {
        let l = self.lanes.entry(chain).or_default();
        l.n += 1;
        let signal = if challenged { cf_net_usd.max(0.0) } else { 0.0 };
        l.credit += ALPHA * (signal - l.credit);
    }

    /// Fresh gas-cost observation from the funnel's own cost model.
    pub fn note_gas(&mut self, chain: Chain, gas_usd: f64) {
        let l = self.lanes.entry(chain).or_default();
        l.gas_ewma += ALPHA * (gas_usd - l.gas_ewma);
    }

    /// The floor applied to net profit for `chain` right now.
    pub fn floor(&self, chain: Chain) -> f64 {
        if !self.enabled {
            return self.base;
        }
        let l = match self.lanes.get(&chain) {
            Some(l) => l,
            None => return self.base,
        };
        (self.base + self.gas_mult * l.gas_ewma - l.credit)
            .clamp(self.floor_min, self.cap)
    }

    /// Diagnostics for telemetry (wired on the next telemetry pass).
    #[allow(dead_code)]
    pub fn lane_stats(&self, chain: Chain) -> (f64, f64, u64) {
        self.lanes
            .get(&chain)
            .map(|l| (l.credit, l.gas_ewma, l.n))
            .unwrap_or((0.0, 0.0, 0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_returns_base() {
        let mut t = AdaptiveFloor::new(5.0, 1.5, 1.0, 50.0, false);
        t.note_verdict(Chain::Bsc, true, 20.0);
        t.note_gas(Chain::Bsc, 2.0);
        assert_eq!(t.floor(Chain::Bsc), 5.0);
    }

    #[test]
    fn challenged_verdicts_lower_floor() {
        let mut t = AdaptiveFloor::new(5.0, 0.5, 1.0, 50.0, true);
        for _ in 0..200 {
            t.note_verdict(Chain::Bsc, true, 10.0);
        }
        let f = t.floor(Chain::Bsc);
        assert!(f < 5.0, "floor {f} should drop under sustained challenges");
        assert!(f >= 1.0, "floor {f} never below floor_min");
    }

    #[test]
    fn gas_regime_raises_floor() {
        let mut t = AdaptiveFloor::new(5.0, 0.5, 1.0, 50.0, true);
        for _ in 0..200 {
            t.note_gas(Chain::Eth, 20.0);
        }
        let f = t.floor(Chain::Eth);
        assert!(f > 5.0, "floor {f} should rise with the gas regime");
    }

    #[test]
    fn floor_is_clamped() {
        let mut t = AdaptiveFloor::new(5.0, 0.5, 1.0, 8.0, true);
        for _ in 0..500 {
            t.note_gas(Chain::Eth, 100.0);
        }
        assert_eq!(t.floor(Chain::Eth), 8.0);
        let mut t2 = AdaptiveFloor::new(5.0, 0.5, 3.0, 50.0, true);
        for _ in 0..500 {
            t2.note_verdict(Chain::Bsc, true, 100.0);
        }
        assert_eq!(t2.floor(Chain::Bsc), 3.0);
    }
}
