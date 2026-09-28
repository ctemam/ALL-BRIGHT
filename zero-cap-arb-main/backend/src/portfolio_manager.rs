use crate::types::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyAllocation {
    pub strategy: ArbitrageType,
    pub weight_pct: f64,
    pub max_concurrent: u32,
    pub min_profit_usd: f64,
    pub max_daily_trades: u32,
    pub daily_trades: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortfolioConfig {
    pub strategies: Vec<StrategyAllocation>,
    pub total_balance_usd: f64,
    pub risk_per_trade_pct: f64,
    pub max_daily_loss_usd: f64,
    pub daily_loss: f64,
    pub max_open_positions: u32,
    pub open_positions: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortfolioStatus {
    pub config: PortfolioConfig,
    pub total_pnl_usd: f64,
    pub total_pnl_pct: f64,
    pub daily_pnl_usd: f64,
    pub open_trades: u32,
    pub today_trades: u32,
    pub win_rate_pct: f64,
    pub strategy_breakdown: Vec<StrategyBreakdown>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyBreakdown {
    pub strategy: String,
    pub total_trades: u32,
    pub wins: u32,
    pub losses: u32,
    pub pnl_usd: f64,
    pub win_rate_pct: f64,
    pub allocation_pct: f64,
}

pub struct PortfolioManager {
    pub config: PortfolioConfig,
    pub total_pnl: f64,
    pub trades: Vec<String>,
    pub strategy_stats: HashMap<String, (u32, u32, u32, f64)>,
}

impl PortfolioManager {
    pub fn new(config: PortfolioConfig) -> Self {
        Self {
            config,
            total_pnl: 0.0,
            trades: Vec::new(),
            strategy_stats: HashMap::new(),
        }
    }

    pub fn can_trade(&self, strategy: &str, profit_usd: f64) -> bool {
        if self.config.open_positions >= self.config.max_open_positions { return false; }
        if self.config.daily_loss.abs() >= self.config.max_daily_loss_usd { return false; }
        if profit_usd < 0.0 && profit_usd.abs() > (self.config.total_balance_usd * self.config.risk_per_trade_pct / 100.0) { return false; }
        true
    }

    pub fn record_trade(&mut self, strategy: &str, profit_usd: f64) {
        self.total_pnl += profit_usd;
        self.config.open_positions += 1;
        self.config.daily_loss += profit_usd.min(0.0);
        self.trades.push(format!("{}:${:.2}", strategy, profit_usd));
        let entry = self.strategy_stats.entry(strategy.to_string()).or_insert((0, 0, 0, 0.0));
        entry.0 += 1;
        if profit_usd > 0.0 { entry.1 += 1; }
        else { entry.2 += 1; }
        entry.3 += profit_usd;
    }

    pub fn close_trade(&mut self) {
        self.config.open_positions = self.config.open_positions.saturating_sub(1);
    }

    pub fn get_status(&self) -> PortfolioStatus {
        let breakdown: Vec<StrategyBreakdown> = self.strategy_stats.iter().map(|(strat, &(total, wins, losses, pnl))| {
            StrategyBreakdown {
                strategy: strat.clone(),
                total_trades: total,
                wins,
                losses,
                pnl_usd: pnl,
                win_rate_pct: if total > 0 { (wins as f64 / total as f64) * 100.0 } else { 0.0 },
                allocation_pct: self.config.strategies.iter()
                    .find(|s| format!("{:?}", s.strategy) == *strat)
                    .map(|s| s.weight_pct).unwrap_or(0.0),
            }
        }).collect();

        let total_trades: u32 = breakdown.iter().map(|b| b.total_trades).sum();
        let total_wins: u32 = breakdown.iter().map(|b| b.wins).sum();
        let win_rate = if total_trades > 0 { (total_wins as f64 / total_trades as f64) * 100.0 } else { 0.0 };

        PortfolioStatus {
            config: self.config.clone(),
            total_pnl_usd: self.total_pnl,
            total_pnl_pct: if self.config.total_balance_usd > 0.0 { (self.total_pnl / self.config.total_balance_usd) * 100.0 } else { 0.0 },
            daily_pnl_usd: self.total_pnl,
            open_trades: self.config.open_positions,
            today_trades: total_trades,
            win_rate_pct: win_rate,
            strategy_breakdown: breakdown,
        }
    }

    pub fn reset_daily(&mut self) {
        self.config.daily_loss = 0.0;
        for s in &mut self.config.strategies {
            s.daily_trades = 0;
        }
    }
}
