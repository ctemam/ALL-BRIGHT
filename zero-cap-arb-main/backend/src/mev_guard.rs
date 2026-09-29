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

    /// Real MEV analysis is not implemented.
    ///
    /// This used to return `rand::random()` scores, sandwich/frontrun probabilities and a
    /// hard-coded bot list, which fed real risk decisions with noise. It now fails loudly
    /// until pending-transaction inspection and a transaction simulator are wired up
    /// (defect D-09 in `docs/ARBITRAGE-COMPARISON.md`).
    pub fn analyze_pending(
        &self,
        chain_id: u64,
        tx_data: &str,
    ) -> Result<MevDetectionResult, String> {
        let _ = (chain_id, tx_data);
        Err(
            "MEV analysis is not implemented: no pending-transaction inspection or \
             simulation backend is wired up, so no risk score can be produced. This \
             endpoint previously returned randomised values."
                .to_string(),
        )
    }

    pub fn is_safe_to_trade(&self, result: &MevDetectionResult) -> bool {
        if !self.config.enabled {
            return true;
        }
        let max_score = match self.config.max_risk_level {
            MevRiskLevel::Safe => 10.0,
            MevRiskLevel::LowRisk => 30.0,
            MevRiskLevel::MediumRisk => 50.0,
            MevRiskLevel::HighRisk => 70.0,
            MevRiskLevel::Critical => 100.0,
        };
        result.score <= max_score
    }

    /// Real honeypot detection is not implemented.
    ///
    /// This used to return a hard-coded `true`, which reported every token as safe. A real
    /// implementation must simulate a transfer against the token contract.
    pub fn check_honeypot(&self, token_address: &str) -> Result<bool, String> {
        let _ = token_address;
        Err(
            "Honeypot detection is not implemented: it requires a transfer simulation \
             against the token contract. This previously always reported `true`."
                .to_string(),
        )
    }
}
