use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SplitterConfig {
    pub wallets: Vec<SplitterWallet>,
    pub enabled: bool,
    pub min_split_profit_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SplitterWallet {
    pub address: String,
    pub label: String,
    pub share_pct: f64,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfitSplitResult {
    pub total_profit_usd: f64,
    pub splits: Vec<SplitEntry>,
    pub executed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SplitEntry {
    pub address: String,
    pub label: String,
    pub amount_usd: f64,
    pub percentage: f64,
}

pub struct ProfitSplitter {
    pub config: SplitterConfig,
}

impl ProfitSplitter {
    pub fn new(config: SplitterConfig) -> Self {
        Self { config }
    }

    pub fn calculate_split(&self, total_profit_usd: f64) -> ProfitSplitResult {
        let mut splits = Vec::new();
        for wallet in &self.config.wallets {
            if !wallet.enabled { continue; }
            splits.push(SplitEntry {
                address: wallet.address.clone(),
                label: wallet.label.clone(),
                amount_usd: total_profit_usd * (wallet.share_pct / 100.0),
                percentage: wallet.share_pct,
            });
        }
        let total_pct: f64 = splits.iter().map(|s| s.percentage).sum();
        if total_pct < 99.9 {
            splits.push(SplitEntry {
                address: "0xTreasury".into(),
                label: "Treasury (unallocated)".into(),
                amount_usd: total_profit_usd * ((100.0 - total_pct) / 100.0),
                percentage: 100.0 - total_pct,
            });
        }
        ProfitSplitResult {
            total_profit_usd,
            splits,
            executed: false,
        }
    }
}
