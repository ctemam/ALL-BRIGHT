use crate::types::*;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use parking_lot::RwLock;
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PaperTradeMode {
    HistoricalBacktest(i64, i64),
    LiveSimulation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PaperTradeResult { Win, Loss, BreakEven }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TradeStatus { Simulated, Executed, Failed }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaperTrade {
    pub id: String,
    pub opportunity: OpportunityDetail,
    pub entry_time: i64,
    pub exit_time: Option<i64>,
    pub result: Option<PaperTradeResult>,
    pub profit_usd: f64,
    pub roi_pct: f64,
    pub gas_used_usd: f64,
    pub flash_loan_fee_usd: f64,
    pub status: TradeStatus,
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestConfig {
    pub start_timestamp: i64,
    pub end_timestamp: i64,
    pub initial_balance_usd: f64,
    pub min_spread_pct: f64,
    pub max_slippage_pct: f64,
    pub trade_size_pct: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonthlyReturn {
    pub month: String,
    pub return_pct: f64,
    pub trades: u64,
    pub profit_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EquityPoint {
    pub timestamp: i64,
    pub balance: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestResult {
    pub trades: Vec<PaperTrade>,
    pub start_time: i64,
    pub end_time: i64,
    pub initial_balance: f64,
    pub final_balance: f64,
    pub total_return_pct: f64,
    pub total_trades: u64,
    pub wins: u64,
    pub losses: u64,
    pub win_rate_pct: f64,
    pub largest_win_usd: f64,
    pub largest_loss_usd: f64,
    pub max_drawdown_pct: f64,
    pub profit_factor: f64,
    pub sharpe_ratio: f64,
    pub avg_profit_per_trade: f64,
    pub best_strategy: String,
    pub monthly_returns: Vec<MonthlyReturn>,
    pub equity_curve: Vec<EquityPoint>,
}

pub struct PaperTrader {
    pub trades: Vec<PaperTrade>,
    pub balance_usd: f64,
    pub initial_balance_usd: f64,
    pub total_trades: u64,
    pub wins: u64,
    pub losses: u64,
    pub total_profit_usd: f64,
    pub total_loss_usd: f64,
    pub largest_win_usd: f64,
    pub largest_loss_usd: f64,
    pub max_drawdown_pct: f64,
    pub win_rate_pct: f64,
    pub profit_factor: f64,
    pub sharpe_ratio: f64,
    pub avg_trade_duration_secs: f64,
    pub start_time: Option<i64>,
    pub end_time: Option<i64>,
    pub is_running: bool,
    pub mode: PaperTradeMode,
    equity_curve: Vec<f64>,
    returns: Vec<f64>,
}

impl PaperTrader {
    pub fn new(mode: PaperTradeMode, initial_balance: f64) -> Self {
        Self {
            trades: Vec::new(),
            balance_usd: initial_balance,
            initial_balance_usd: initial_balance,
            total_trades: 0,
            wins: 0,
            losses: 0,
            total_profit_usd: 0.0,
            total_loss_usd: 0.0,
            largest_win_usd: 0.0,
            largest_loss_usd: 0.0,
            max_drawdown_pct: 0.0,
            win_rate_pct: 0.0,
            profit_factor: 1.0,
            sharpe_ratio: 0.0,
            avg_trade_duration_secs: 0.0,
            start_time: None,
            end_time: None,
            is_running: false,
            mode,
            equity_curve: vec![initial_balance],
            returns: Vec::new(),
        }
    }

    pub fn start(&mut self) {
        self.is_running = true;
        self.start_time = Some(chrono::Utc::now().timestamp());
        info!("Paper trader started with balance ${:.2}", self.initial_balance_usd);
    }

    pub fn stop(&mut self) -> BacktestResult {
        self.is_running = false;
        self.end_time = Some(chrono::Utc::now().timestamp());
        self.calculate_metrics();
        self.to_result()
    }

    pub fn simulate_trade(&mut self, opportunity: &OpportunityDetail) -> PaperTrade {
        let now = chrono::Utc::now().timestamp();
        let net_profit = opportunity.profit_breakdown.net_profit_usd;
        let gas = opportunity.profit_breakdown.costs.gas_estimated_usd;
        let fl_fee = opportunity.profit_breakdown.costs.flash_loan_fee_usd;
        let roi = opportunity.profit_breakdown.roi_pct;

        let (result, profit) = if net_profit > 0.0 {
            self.balance_usd += net_profit;
            self.wins += 1;
            self.total_profit_usd += net_profit;
            if net_profit > self.largest_win_usd { self.largest_win_usd = net_profit; }
            (Some(PaperTradeResult::Win), net_profit)
        } else if net_profit < 0.0 {
            self.balance_usd += net_profit;
            self.losses += 1;
            self.total_loss_usd += net_profit.abs();
            if net_profit.abs() > self.largest_loss_usd { self.largest_loss_usd = net_profit.abs(); }
            (Some(PaperTradeResult::Loss), net_profit)
        } else {
            (Some(PaperTradeResult::BreakEven), 0.0)
        };

        self.total_trades += 1;
        self.returns.push(roi);
        self.equity_curve.push(self.balance_usd);

        let trade = PaperTrade {
            id: uuid::Uuid::new_v4().to_string(),
            opportunity: opportunity.clone(),
            entry_time: now,
            exit_time: Some(now + 2),
            result,
            profit_usd: profit,
            roi_pct: roi,
            gas_used_usd: gas,
            flash_loan_fee_usd: fl_fee,
            status: TradeStatus::Simulated,
            notes: String::new(),
        };
        self.trades.push(trade.clone());
        trade
    }

    pub fn get_metrics(&self) -> BacktestResult {
        let mut bt = self.to_result();
        bt.trades = self.trades.clone();
        bt
    }

    pub fn reset(&mut self) {
        self.trades.clear();
        self.balance_usd = self.initial_balance_usd;
        self.total_trades = 0;
        self.wins = 0;
        self.losses = 0;
        self.total_profit_usd = 0.0;
        self.total_loss_usd = 0.0;
        self.largest_win_usd = 0.0;
        self.largest_loss_usd = 0.0;
        self.max_drawdown_pct = 0.0;
        self.returns.clear();
        self.equity_curve = vec![self.initial_balance_usd];
    }

    fn calculate_metrics(&mut self) {
        let total = self.total_trades as f64;
        self.win_rate_pct = if total > 0.0 { (self.wins as f64 / total) * 100.0 } else { 0.0 };
        self.profit_factor = if self.total_loss_usd > 0.0 { self.total_profit_usd / self.total_loss_usd } else { self.total_profit_usd.max(1.0) };
        self.sharpe_ratio = calculate_sharpe_ratio(&self.returns, 2.0);
        self.max_drawdown_pct = calculate_max_drawdown(&self.equity_curve);
        self.avg_trade_duration_secs = 2.0;
    }

    fn to_result(&self) -> BacktestResult {
        let final_bal = self.balance_usd;
        let total_return = if self.initial_balance_usd > 0.0 { ((final_bal - self.initial_balance_usd) / self.initial_balance_usd) * 100.0 } else { 0.0 };
        let avg_profit = if self.total_trades > 0 { self.total_profit_usd / self.total_trades as f64 } else { 0.0 };

        BacktestResult {
            trades: self.trades.clone(),
            start_time: self.start_time.unwrap_or(0),
            end_time: self.end_time.unwrap_or(0),
            initial_balance: self.initial_balance_usd,
            final_balance: final_bal,
            total_return_pct: total_return,
            total_trades: self.total_trades,
            wins: self.wins,
            losses: self.losses,
            win_rate_pct: self.win_rate_pct,
            largest_win_usd: self.largest_win_usd,
            largest_loss_usd: self.largest_loss_usd,
            max_drawdown_pct: self.max_drawdown_pct,
            profit_factor: self.profit_factor,
            sharpe_ratio: self.sharpe_ratio,
            avg_profit_per_trade: avg_profit,
            best_strategy: self.find_best_strategy(),
            monthly_returns: vec![],
            equity_curve: self.equity_curve.iter().enumerate().map(|(i, &b)| EquityPoint {
                timestamp: self.start_time.unwrap_or(0) + (i as i64 * 30),
                balance: b,
            }).collect(),
        }
    }

    fn find_best_strategy(&self) -> String {
        let mut strat_profits: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
        for trade in &self.trades {
            let key = format!("{:?}", trade.opportunity.arbitrage_type);
            *strat_profits.entry(key).or_insert(0.0) += trade.profit_usd;
        }
        strat_profits.into_iter().max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(k, _)| k)
            .unwrap_or_else(|| "N/A".to_string())
    }
}

pub fn calculate_sharpe_ratio(returns: &[f64], risk_free_rate: f64) -> f64 {
    if returns.len() < 2 { return 0.0; }
    let mean = returns.iter().sum::<f64>() / returns.len() as f64;
    let variance = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / (returns.len() - 1) as f64;
    let std_dev = variance.sqrt();
    if std_dev == 0.0 { return 0.0; }
    (mean - risk_free_rate / 100.0) / std_dev
}

pub fn calculate_max_drawdown(balances: &[f64]) -> f64 {
    if balances.is_empty() { return 0.0; }
    let mut peak = balances[0];
    let mut max_dd = 0.0;
    for &b in balances {
        if b > peak { peak = b; }
        let dd = (peak - b) / peak;
        if dd > max_dd { max_dd = dd; }
    }
    max_dd * 100.0
}
