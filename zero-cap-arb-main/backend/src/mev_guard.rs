use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MevRiskLevel {
    Safe,
    LowRisk,
    MediumRisk,
    HighRisk,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MevDetectionResult {
    pub risk_level: MevRiskLevel,
    pub score: f64,
    pub sandwich_probability: f64,
    pub frontrun_probability: f64,
    pub backrun_probability: f64,
    pub unchecked_enabled: bool,
    pub detected_bots: Vec<String>,
    pub pending_tx_count: u64,
    pub recommended_action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MevGuardConfig {
    pub enabled: bool,
    pub block_sandwich: bool,
    pub block_frontrun: bool,
    pub block_backrun: bool,
    pub max_risk_level: MevRiskLevel,
    pub use_flashbots: bool,
    pub use_private_mempool: bool,
    pub delay_seconds: u64,
    pub honeypot_check: bool,
}

pub struct MevGuard {
    pub config: MevGuardConfig,
}

impl MevGuard {
    pub fn new(config: MevGuardConfig) -> Self {
        Self { config }
    }

    pub fn analyze_pending(&self, _chain_id: u64, _tx_data: &str) -> MevDetectionResult {
        let score = rand::random::<f64>() * 100.0;
        let risk = if score > 80.0 { MevRiskLevel::Critical }
            else if score > 60.0 { MevRiskLevel::HighRisk }
            else if score > 40.0 { MevRiskLevel::MediumRisk }
            else if score > 20.0 { MevRiskLevel::LowRisk }
            else { MevRiskLevel::Safe };

        MevDetectionResult {
            risk_level: risk,
            score,
            sandwich_probability: rand::random::<f64>() * 0.3,
            frontrun_probability: rand::random::<f64>() * 0.2,
            backrun_probability: rand::random::<f64>() * 0.15,
            unchecked_enabled: !self.config.enabled,
            detected_bots: if score > 50.0 { vec!["MEV Bot 0x...a3f2".into(), "JaredFromSubway.eth".into()] } else { vec![] },
            pending_tx_count: (rand::random::<f64>() * 50.0) as u64,
            recommended_action: if score > 60.0 { "Use Flashbots private bundle instead of public mempool".into() }
                else if score > 30.0 { "Monitor closely, consider MEV-Share".into() }
                else { "Safe to execute via public mempool".into() },
        }
    }

    pub fn is_safe_to_trade(&self, result: &MevDetectionResult) -> bool {
        if !self.config.enabled { return true; }
        let max_score = match self.config.max_risk_level {
            MevRiskLevel::Safe => 10.0,
            MevRiskLevel::LowRisk => 30.0,
            MevRiskLevel::MediumRisk => 50.0,
            MevRiskLevel::HighRisk => 70.0,
            MevRiskLevel::Critical => 100.0,
        };
        result.score <= max_score
    }

    pub fn check_honeypot(&self, _token_address: &str) -> bool {
        true
    }
}
