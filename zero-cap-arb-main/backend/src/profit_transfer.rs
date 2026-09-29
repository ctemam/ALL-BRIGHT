//! Profit withdrawal: AUTO sweep and MANUAL on-demand transfer.
//!
//! # Safety model
//!
//! This module moves real funds, so it is deliberately hard to trigger by
//! accident:
//!
//!   * **MANUAL is the default.** `PROFIT_TRANSFER_MODE` must be `AUTO`
//!     explicitly; anything else (including unset) means no automatic sweep.
//!   * **The minimum is server-enforced.** A transfer below
//!     `PROFIT_TRANSFER_MIN_USD` is refused regardless of what the caller asks
//!     for, so a dust balance cannot drain gas on every tick.
//!   * **Destinations must be real addresses.** The placeholders that
//!     previously shipped (`0xYourWallet`, `0xReserve`, `0xCharity`) fail
//!     validation and are rejected.
//!   * **Every attempt is recorded**, including refusals.
//!
//! Broadcasting is delegated to a caller-supplied `TransferExecutor`, because
//! the scanner has no EOA signer wired in (see `api::NOT_IMPLEMENTED_EXECUTION`).
//! Without an executor, transfers are reported as `simulated` rather than
//! silently reported as sent.

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

/// An Ethereum address, validated as `0x` + 40 hex characters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Address(pub String);

impl Address {
    /// Reject anything that is not a well-formed address. This is what keeps
    /// the old `0xYourWallet` placeholders from being treated as real.
    pub fn parse(s: &str) -> Result<Self, String> {
        let t = s.trim();
        let ok = t.len() == 42
            && (t.starts_with("0x") || t.starts_with("0X"))
            && t[2..].chars().all(|c| c.is_ascii_hexdigit());
        if !ok {
            return Err(format!("not a 20-byte hex address: {s:?}"));
        }
        Ok(Self(t.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Where a share of profit goes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Destination {
    pub address: Address,
    pub label: String,
    /// Share of the transferred amount, 0-100. Enabled destinations must sum
    /// to at most 100; any remainder is reported, never silently reassigned.
    pub share_pct: f64,
    pub enabled: bool,
}

/// One planned share of a withdrawal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedShare {
    pub address: Address,
    pub label: String,
    pub share_pct: f64,
    pub amount_usd: f64,
}

/// Outcome of a transfer attempt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferResult {
    pub ok: bool,
    pub reason: String,
    pub total_usd: f64,
    pub shares: Vec<PlannedShare>,
    /// Unallocated remainder when shares sum to less than 100%.
    pub unallocated_usd: f64,
    /// True when no executor is configured, so nothing was broadcast. Callers
    /// must not present a simulated transfer as a completed one.
    pub simulated: bool,
    pub timestamp: u64,
}

/// Performs the on-chain send. Supplied by the caller because the scanner has
/// no signer; a dry-run deployment passes a simulator instead.
pub type TransferExecutor = Arc<dyn Fn(&[PlannedShare]) -> Result<String, String> + Send + Sync>;

/// Runtime configuration for the withdrawal service.
#[derive(Debug, Clone)]
pub struct ProfitTransferConfig {
    /// When true, a background task sweeps automatically at the threshold.
    pub auto: bool,
    /// Server-enforced floor. Transfers below this are refused.
    pub min_usd: f64,
    /// Optional cap per transfer, to bound blast radius if a config is wrong.
    pub max_usd: Option<f64>,
    pub destinations: Vec<Destination>,
    /// Interval for the AUTO sweep, in seconds.
    pub interval_secs: u64,
}

impl Default for ProfitTransferConfig {
    /// The fail-safe default: manual only, nothing moves without an operator.
    fn default() -> Self {
        Self {
            auto: false,
            min_usd: 25.0,
            max_usd: None,
            destinations: Vec::new(),
            interval_secs: 300,
        }
    }
}

impl ProfitTransferConfig {
    /// Build from environment. Destinations come from `PROFIT_DESTINATIONS` as
    /// `address:label:pct` triples separated by commas.
    pub fn from_env() -> Self {
        let auto = std::env::var("PROFIT_TRANSFER_MODE")
            .unwrap_or_default()
            .trim()
            .eq_ignore_ascii_case("AUTO");
        let min_usd = std::env::var("PROFIT_TRANSFER_MIN_USD")
            .ok()
            .and_then(|v| v.parse().ok())
            .filter(|v: &f64| v.is_finite() && *v > 0.0)
            .unwrap_or(25.0);
        let max_usd = std::env::var("PROFIT_TRANSFER_MAX_USD")
            .ok()
            .and_then(|v| v.parse().ok())
            .filter(|v: &f64| v.is_finite() && *v > 0.0);

        let mut destinations = Vec::new();
        if let Ok(raw) = std::env::var("PROFIT_DESTINATIONS") {
            for part in raw.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                let mut f = part.splitn(3, ':');
                let (Some(addr), Some(label), Some(pct)) = (f.next(), f.next(), f.next()) else {
                    tracing::warn!("ignoring malformed PROFIT_DESTINATIONS entry: {part:?}");
                    continue;
                };
                let address = match Address::parse(addr) {
                    Ok(a) => a,
                    // This is where `0xYourWallet` gets rejected.
                    Err(e) => {
                        tracing::warn!("ignoring destination {addr:?}: {e}");
                        continue;
                    }
                };
                let share_pct: f64 = match pct.parse() {
                    Ok(p) if (0.0..=100.0).contains(&p) => p,
                    _ => {
                        tracing::warn!("ignoring destination {addr:?}: bad share {pct:?}");
                        continue;
                    }
                };
                destinations.push(Destination {
                    address,
                    label: label.to_string(),
                    share_pct,
                    enabled: true,
                });
            }
        }

        let interval_secs = std::env::var("PROFIT_TRANSFER_INTERVAL_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .filter(|v: &u64| *v > 0)
            .unwrap_or(300);

        Self {
            auto,
            min_usd,
            max_usd,
            destinations,
            interval_secs,
        }
    }

    pub fn total_share_pct(&self) -> f64 {
        self.destinations
            .iter()
            .filter(|d| d.enabled)
            .map(|d| d.share_pct)
            .sum()
    }

    /// Reasons this config cannot be used, if any.
    pub fn validate(&self) -> Vec<String> {
        let mut errs = Vec::new();
        let total = self.total_share_pct();
        if total > 100.0 + f64::EPSILON {
            errs.push(format!(
                "destination shares total {total:.2}%, which exceeds 100%"
            ));
        }
        if self.destinations.is_empty() {
            errs.push("no valid destinations configured".to_string());
        }
        if self.destinations.iter().all(|d| !d.enabled) {
            errs.push("no enabled destinations".to_string());
        }
        if let Some(max) = self.max_usd {
            if max < self.min_usd {
                errs.push(format!(
                    "max_usd ({max}) is below min_usd ({}), so no transfer could ever run",
                    self.min_usd
                ));
            }
        }
        errs
    }
}

/// Withdrawal service shared by the AUTO task and the MANUAL endpoint.
pub struct ProfitTransferService {
    config: RwLock<ProfitTransferConfig>,
    executor: RwLock<Option<TransferExecutor>>,
    ledger: RwLock<Vec<TransferResult>>,
    /// Profit accrued but not yet withdrawn, in USD.
    accrued_usd: RwLock<f64>,
}

impl ProfitTransferService {
    pub fn new(config: ProfitTransferConfig) -> Self {
        Self {
            config: RwLock::new(config),
            executor: RwLock::new(None),
            ledger: RwLock::new(Vec::new()),
            accrued_usd: RwLock::new(0.0),
        }
    }

    pub async fn config(&self) -> ProfitTransferConfig {
        self.config.read().await.clone()
    }

    pub async fn set_executor(&self, e: TransferExecutor) {
        *self.executor.write().await = Some(e);
    }

    /// Record realised profit. AUTO sweeps once this crosses the threshold.
    pub async fn record_profit(&self, usd: f64) {
        if usd.is_finite() && usd > 0.0 {
            *self.accrued_usd.write().await += usd;
        }
    }

    pub async fn accrued_usd(&self) -> f64 {
        *self.accrued_usd.read().await
    }

    pub async fn history(&self) -> Vec<TransferResult> {
        self.ledger.read().await.clone()
    }

    /// Compute the split without executing anything.
    pub async fn preview(&self, amount_usd: f64) -> TransferResult {
        let cfg = self.config.read().await.clone();
        let enabled: Vec<&Destination> = cfg.destinations.iter().filter(|d| d.enabled).collect();
        let total_pct: f64 = enabled.iter().map(|d| d.share_pct).sum();

        let shares: Vec<PlannedShare> = enabled
            .iter()
            .map(|d| PlannedShare {
                address: d.address.clone(),
                label: d.label.clone(),
                share_pct: d.share_pct,
                amount_usd: amount_usd * (d.share_pct / 100.0),
            })
            .collect();

        let unallocated = if total_pct < 100.0 {
            amount_usd * ((100.0 - total_pct) / 100.0)
        } else {
            0.0
        };

        TransferResult {
            ok: true,
            reason: "preview only; nothing was sent".to_string(),
            total_usd: amount_usd,
            shares,
            unallocated_usd: unallocated,
            simulated: true,
            timestamp: now_secs(),
        }
    }

    /// Execute a withdrawal, enforcing the server-side rules.
    ///
    /// Refuses (and records the refusal) when the config is invalid, the
    /// amount is non-positive, below the enforced minimum, or above the cap.
    pub async fn transfer(&self, amount_usd: f64) -> TransferResult {
        let cfg = self.config.read().await.clone();

        let mut result = TransferResult {
            ok: false,
            reason: String::new(),
            total_usd: amount_usd,
            shares: Vec::new(),
            unallocated_usd: 0.0,
            simulated: true,
            timestamp: now_secs(),
        };

        let errs = cfg.validate();
        if !errs.is_empty() {
            result.reason = errs.join("; ");
            self.record(result.clone()).await;
            return result;
        }

        if !amount_usd.is_finite() || amount_usd <= 0.0 {
            result.reason = format!("refusing non-positive amount {amount_usd}");
            self.record(result.clone()).await;
            return result;
        }

        // Server-enforced floor: the caller's requested amount cannot bypass it.
        if amount_usd < cfg.min_usd {
            result.reason = format!(
                "refusing {amount_usd:.2} USD: below the enforced minimum of {:.2} USD",
                cfg.min_usd
            );
            self.record(result.clone()).await;
            return result;
        }

        if let Some(max) = cfg.max_usd {
            if amount_usd > max {
                result.reason = format!(
                    "refusing {amount_usd:.2} USD: above the configured cap of {max:.2} USD"
                );
                self.record(result.clone()).await;
                return result;
            }
        }

        let preview = self.preview(amount_usd).await;
        result.shares = preview.shares;
        result.unallocated_usd = preview.unallocated_usd;

        // Clone the executor out of the lock so it is not held across the await.
        let exec = self.executor.read().await.clone();
        match exec {
            Some(exec) => match exec(&result.shares) {
                Ok(tx_hash) => {
                    result.ok = true;
                    result.simulated = false;
                    result.reason = format!("submitted: {tx_hash}");
                    *self.accrued_usd.write().await -= amount_usd;
                }
                Err(e) => result.reason = format!("executor rejected the transfer: {e}"),
            },
            None => {
                // Be explicit: no signer is wired up, so nothing was sent.
                result.reason =
                    "no transfer executor configured; computed the split but sent nothing"
                        .to_string();
            }
        }

        self.record(result.clone()).await;
        result
    }

    async fn record(&self, r: TransferResult) {
        let mut l = self.ledger.write().await;
        // Bound the ledger so a long-running process cannot grow it unbounded.
        if l.len() >= 500 {
            l.remove(0);
        }
        l.push(r);
    }

    /// Background AUTO sweep. A manual transfer can be triggered at any time
    /// regardless of this mode.
    pub fn spawn_auto_sweep(self: &Arc<Self>) {
        let svc = Arc::clone(self);
        tokio::spawn(async move {
            loop {
                let interval = svc.config.read().await.interval_secs.max(1);
                tokio::time::sleep(std::time::Duration::from_secs(interval)).await;

                let cfg = svc.config.read().await.clone();
                if !cfg.auto {
                    continue;
                }
                let accrued = svc.accrued_usd().await;
                if accrued < cfg.min_usd {
                    continue;
                }
                tracing::info!(
                    "AUTO profit transfer: {accrued:.2} USD accrued, minimum {:.2} USD",
                    cfg.min_usd
                );
                let r = svc.transfer(accrued).await;
                if !r.ok {
                    tracing::warn!("AUTO profit transfer refused: {}", r.reason);
                }
            }
        });
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(n: u8) -> Address {
        Address(format!("0x{}", format!("{n:02x}").repeat(20)))
    }

    fn cfg(auto: bool, min: f64) -> ProfitTransferConfig {
        ProfitTransferConfig {
            auto,
            min_usd: min,
            max_usd: None,
            destinations: vec![
                Destination {
                    address: addr(0xAA),
                    label: "A".into(),
                    share_pct: 70.0,
                    enabled: true,
                },
                Destination {
                    address: addr(0xBB),
                    label: "B".into(),
                    share_pct: 30.0,
                    enabled: true,
                },
            ],
            interval_secs: 1,
        }
    }

    #[test]
    fn address_validation_accepts_real_addresses() {
        assert!(Address::parse("0x2eF34d88EC4EBBd5543fFF2784D5AdbC01f14D56").is_ok());
    }

    /// The old hardcoded placeholders must be rejected, not sent to.
    #[test]
    fn address_validation_rejects_placeholders() {
        for bad in [
            "0xYourWallet",
            "0xReserve",
            "0xCharity",
            "",
            "not-an-address",
            "0x1234",
        ] {
            assert!(Address::parse(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn default_config_is_manual_and_does_nothing() {
        let d = ProfitTransferConfig::default();
        assert!(!d.auto, "AUTO must not be the default");
        assert!(d.destinations.is_empty());
    }

    #[test]
    fn shares_over_100_percent_is_invalid() {
        let mut c = cfg(false, 25.0);
        c.destinations[0].share_pct = 80.0;
        c.destinations[1].share_pct = 40.0;
        assert!(!c.validate().is_empty());
    }

    #[test]
    fn no_destinations_is_invalid() {
        let mut c = cfg(false, 25.0);
        c.destinations.clear();
        assert!(!c.validate().is_empty());
    }

    #[test]
    fn cap_below_minimum_is_invalid() {
        let mut c = cfg(false, 100.0);
        c.max_usd = Some(50.0);
        assert!(!c.validate().is_empty());
    }

    /// The core safety property: a caller cannot bypass the server-side floor.
    #[tokio::test]
    async fn transfer_below_minimum_is_refused() {
        let svc = ProfitTransferService::new(cfg(false, 25.0));
        let r = svc.transfer(10.0).await;
        assert!(!r.ok);
        assert!(r.reason.contains("minimum"), "got: {}", r.reason);
        assert_eq!(svc.history().await.len(), 1, "refusal must be recorded");
    }

    #[tokio::test]
    async fn non_positive_amount_is_refused() {
        let svc = ProfitTransferService::new(cfg(false, 0.0));
        for amt in [0.0, -5.0] {
            let r = svc.transfer(amt).await;
            assert!(!r.ok);
        }
    }

    #[tokio::test]
    async fn above_cap_is_refused() {
        let mut c = cfg(false, 10.0);
        c.max_usd = Some(100.0);
        let svc = ProfitTransferService::new(c);
        let r = svc.transfer(500.0).await;
        assert!(!r.ok);
        assert!(r.reason.contains("cap"), "got: {}", r.reason);
    }

    /// With no executor the service must say so rather than claim success.
    #[tokio::test]
    async fn no_executor_reports_simulated_not_sent() {
        let svc = ProfitTransferService::new(cfg(false, 25.0));
        let r = svc.transfer(100.0).await;
        assert!(!r.ok, "nothing was actually sent");
        assert!(r.simulated, "must be flagged as simulated");
        assert!(r.reason.contains("no transfer executor"));
    }

    #[tokio::test]
    async fn executor_success_is_recorded_and_debits_accrued() {
        let svc = ProfitTransferService::new(cfg(false, 25.0));
        svc.set_executor(Arc::new(|_shares| Ok("0xdeadbeef".to_string())))
            .await;
        svc.record_profit(100.0).await;
        let r = svc.transfer(100.0).await;
        assert!(r.ok, "transfer refused: {}", r.reason);
        assert!(!r.simulated);
        assert_eq!(svc.accrued_usd().await, 0.0);
    }

    #[tokio::test]
    async fn executor_failure_does_not_debit_accrued() {
        let svc = ProfitTransferService::new(cfg(false, 25.0));
        svc.set_executor(Arc::new(|_s| Err("insufficient gas".to_string())))
            .await;
        svc.record_profit(100.0).await;
        let r = svc.transfer(100.0).await;
        assert!(!r.ok);
        assert_eq!(svc.accrued_usd().await, 100.0, "failed send must not debit");
    }

    #[tokio::test]
    async fn preview_splits_by_share() {
        let svc = ProfitTransferService::new(cfg(false, 25.0));
        let p = svc.preview(200.0).await;
        assert_eq!(p.shares.len(), 2);
        assert!((p.shares[0].amount_usd - 140.0).abs() < 1e-9);
        assert!((p.shares[1].amount_usd - 60.0).abs() < 1e-9);
        assert!(p.simulated);
    }

    /// Shares summing under 100% must report the remainder, not hide it.
    #[tokio::test]
    async fn preview_reports_unallocated_remainder() {
        let mut c = cfg(false, 25.0);
        c.destinations[1].share_pct = 10.0; // totals 80%
        let svc = ProfitTransferService::new(c);
        let p = svc.preview(100.0).await;
        assert!((p.unallocated_usd - 20.0).abs() < 1e-9);
    }

    #[tokio::test]
    async fn record_profit_ignores_non_positive() {
        let svc = ProfitTransferService::new(cfg(false, 25.0));
        svc.record_profit(-100.0).await;
        svc.record_profit(f64::NAN).await;
        assert_eq!(svc.accrued_usd().await, 0.0);
    }
}
