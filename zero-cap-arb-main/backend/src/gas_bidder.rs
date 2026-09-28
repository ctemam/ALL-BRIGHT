use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GasBidConfig {
    pub strategy: GasBidStrategy,
    pub max_gas_price_gwei: f64,
    pub min_gas_price_gwei: f64,
    pub priority_pct: f64,
    pub adaptive_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GasBidStrategy {
    Fixed,
    Adaptive,
    Priority,
    MEVProtected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GasRecommendation {
    pub suggested_gwei: f64,
    pub strategy: String,
    pub estimated_cost_usd: f64,
    pub confidence: String,
    pub reasoning: Vec<String>,
}

pub struct GasBidder {
    pub config: GasBidConfig,
}

impl GasBidder {
    pub fn new(config: GasBidConfig) -> Self {
        Self { config }
    }

    pub fn recommend_gas(&self, chain_id: u64, profit_usd: f64, spread_pct: f64) -> GasRecommendation {
        let base_gas = match chain_id {
            1 => 25.0,
            42161 => 0.1,
            10 => 0.05,
            137 => 50.0,
            56 => 5.0,
            43114 => 25.0,
            _ => 10.0,
        };

        let (suggested, strategy_name, reasoning) = match self.config.strategy {
            GasBidStrategy::Fixed => {
                (self.config.max_gas_price_gwei.min(base_gas * 1.5),
                 "Fixed".into(),
                 vec!["Using fixed gas price from config".into()])
            }
            GasBidStrategy::Adaptive => {
                let profit_ratio = (profit_usd / 100.0).clamp(0.5, 3.0);
                let spread_bonus = (spread_pct / 5.0).clamp(1.0, 2.0);
                let gas = base_gas * profit_ratio * spread_bonus;
                (gas.min(self.config.max_gas_price_gwei).max(self.config.min_gas_price_gwei),
                 "Adaptive".into(),
                 vec![format!("Base: {:.1} gwei", base_gas),
                      format!("Profit ratio: {:.2}x", profit_ratio),
                      format!("Spread bonus: {:.2}x", spread_bonus)])
            }
            GasBidStrategy::Priority => {
                let gas = base_gas * (1.0 + self.config.priority_pct / 100.0);
                (gas.min(self.config.max_gas_price_gwei),
                 "Priority".into(),
                 vec![format!("Base + {}% priority", self.config.priority_pct)])
            }
            GasBidStrategy::MEVProtected => {
                let gas = base_gas * 1.5 + 5.0;
                (gas.min(self.config.max_gas_price_gwei),
                 "MEV-Protected".into(),
                 vec!["Higher gas to avoid MEV".into(),
                      format!("Base: {:.1} + 5 gwei protection", base_gas)])
            }
        };

        let estimated = suggested * 21000.0 / 1e9 * get_eth_price_usd(chain_id);
        let confidence = if profit_usd > estimated * 5.0 { "high" }
            else if profit_usd > estimated * 2.0 { "medium" }
            else { "low" };

        GasRecommendation {
            suggested_gwei: suggested,
            strategy: strategy_name,
            estimated_cost_usd: estimated,
            confidence: confidence.into(),
            reasoning,
        }
    }
}

fn get_eth_price_usd(chain_id: u64) -> f64 {
    match chain_id {
        1 | 42161 | 10 => 3450.0,
        137 => 0.72,
        56 => 580.0,
        43114 => 35.0,
        _ => 1000.0,
    }
}
