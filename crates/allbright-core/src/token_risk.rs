//! Token risk gate (GAP-12, strategy P06) — GoPlus Security API client.
//!
//! GoPlus (https://api.gopluslabs.io) is a free, keyless token-security
//! index covering every EVM chain AllBright runs. One batched GET:
//!
//!   GET /api/v1/token_security/{chain_id}?contract_addresses={csv}
//!   {"code":1,"message":"OK","result":{"0x<addr>":{"is_honeypot":"0",…}}}
//!
//! Fail-closed semantics (missing data is never safe):
//! - unreachable / HTTP error / malformed body          → Unverified
//! - token absent from the index                        → Unverified
//! - risky flag set                                     → Flagged (+ reasons)
//! - a required field missing in a valid row            → Unverified
//!
//! Verdicts are cached in a bounded TTL map so the discovery loop never
//! hammers the API: assessed → `ttl` (6h; contracts are quasi-static),
//! Unverified → 60s (transient outages recover fast, no hammering).
//!
//! INTENDED CALL SITES (not wired — callers live outside allbright-core):
//! 1. crates/allbright-ingest/src/evm_stream.rs::discovery_loop (~L471):
//!    after verify_discovered(), before DiscoveredPool{verified:true}.
//!    Flagged on either leg → emit verified:false + classified failure.
//! 2. Quarantine→promotion path pools.unverified.json → pools.json
//!    (tools/fetch_pools.py + chain::load_pools).

use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

/// Default endpoint — keyless, rate-limited (be polite: batch + cache).
const DEFAULT_BASE: &str = "https://api.gopluslabs.io";
/// GoPlus accepts a CSV of addresses; ~30 keeps URLs comfortably short.
const BATCH: usize = 30;
/// Assessed verdicts are near-static for immutable contracts.
const DEFAULT_TTL: Duration = Duration::from_secs(6 * 3600);
/// Unverified expires fast — retry soon, don't pin a token out forever.
const UNVERIFIED_TTL: Duration = Duration::from_secs(60);
/// Hard cap on cached verdicts — oldest entries evict first.
const MAX_CACHE: usize = 10_000;
/// Any nonzero GoPlus tax fraction flags the token — a transfer tax
/// silently breaks atomic route math.
const TAX_FLAG_FRACTION: f64 = 0.0;

/// Tri-state for a boolean GoPlus field. `Unknown` is fail-closed: it
/// prevents `Clear` but does not itself accuse the token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tri {
    No,
    Yes,
    #[default]
    Unknown,
}

impl Tri {
    fn from_str01(s: Option<&str>) -> Self {
        match s {
            Some("1") => Tri::Yes,
            Some("0") => Tri::No,
            _ => Tri::Unknown,
        }
    }
}

/// Typed verdict surface required by GAP-12.
#[derive(Debug, Clone, Copy, Default)]
pub struct RiskFlags {
    pub honeypot: Tri,
    /// Tax as a fraction (GoPlus "0.05" → 5%). `None` = field absent.
    pub buy_tax: Option<f64>,
    pub sell_tax: Option<f64>,
    pub can_mint: Tri,
    pub is_blacklist: Tri,
    pub owner_can_change_balance: Tri,
    pub is_proxy: Tri,
    /// Contract source verified on the explorer (GoPlus `is_open_source`).
    pub verified: Tri,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskGrade {
    /// Assessed, no risky flags — eligible for promotion consideration.
    Clear,
    /// Assessed, ≥1 risky flag — reason-coded, never promote.
    Flagged,
    /// Could not be assessed — treat as unsafe, retry later.
    Unverified,
}

/// Machine-readable reason codes (stable `code()` for journaling).
#[derive(Debug, Clone, PartialEq)]
pub enum RiskReason {
    Honeypot,
    BuyTax(f64),
    SellTax(f64),
    Mintable,
    BlacklistCapable,
    OwnerCanChangeBalance,
    /// Upgradeable proxy whose source is NOT explorer-verified.
    ProxyUnverifiedSource,
    /// Source code not verified on the explorer — scan confidence low.
    SourceUnverified,
    /// A required GoPlus field was absent/unparseable in the row.
    FieldMissing(&'static str),
    /// Address absent from the index response.
    NotInIndex,
    /// Transport/HTTP failure — details for logs only.
    Unreachable(String),
    /// Envelope parsed but `code != 1` or shape unexpected.
    Malformed(String),
}

impl RiskReason {
    pub fn code(&self) -> &'static str {
        match self {
            RiskReason::Honeypot => "HONEYPOT",
            RiskReason::BuyTax(_) => "BUY_TAX",
            RiskReason::SellTax(_) => "SELL_TAX",
            RiskReason::Mintable => "MINTABLE",
            RiskReason::BlacklistCapable => "BLACKLIST_CAPABLE",
            RiskReason::OwnerCanChangeBalance => "OWNER_CAN_CHANGE_BALANCE",
            RiskReason::ProxyUnverifiedSource => "PROXY_UNVERIFIED_SOURCE",
            RiskReason::SourceUnverified => "SOURCE_UNVERIFIED",
            RiskReason::FieldMissing(_) => "FIELD_MISSING",
            RiskReason::NotInIndex => "NOT_IN_INDEX",
            RiskReason::Unreachable(_) => "UNREACHABLE",
            RiskReason::Malformed(_) => "MALFORMED",
        }
    }
}

#[derive(Debug, Clone)]
pub struct TokenVerdict {
    pub grade: RiskGrade,
    pub flags: RiskFlags,
    pub reasons: Vec<RiskReason>,
    pub source: &'static str,
    pub asof_ms: u64,
}

impl TokenVerdict {
    fn unverified(reason: RiskReason) -> Self {
        TokenVerdict {
            grade: RiskGrade::Unverified,
            flags: RiskFlags::default(),
            reasons: vec![reason],
            source: "goplus",
            asof_ms: crate::events::now_ms(),
        }
    }

    pub fn is_clear(&self) -> bool {
        self.grade == RiskGrade::Clear
    }

    /// Comma-joined reason codes for journaled rejection detail.
    pub fn reason_codes(&self) -> String {
        self.reasons
            .iter()
            .map(|r| r.code())
            .collect::<Vec<_>>()
            .join(",")
    }
}

/// Classify one GoPlus result row into a verdict — pure, testable.
fn classify(row: &Value) -> TokenVerdict {
    let flags = RiskFlags {
        honeypot: Tri::from_str01(row["is_honeypot"].as_str()),
        buy_tax: row["buy_tax"].as_str().and_then(|s| s.parse::<f64>().ok()),
        sell_tax: row["sell_tax"].as_str().and_then(|s| s.parse::<f64>().ok()),
        can_mint: Tri::from_str01(row["is_mintable"].as_str()),
        is_blacklist: Tri::from_str01(row["is_blacklisted"].as_str()),
        owner_can_change_balance: Tri::from_str01(row["owner_change_balance"].as_str()),
        is_proxy: Tri::from_str01(row["is_proxy"].as_str()),
        verified: Tri::from_str01(row["is_open_source"].as_str()),
    };

    let mut reasons: Vec<RiskReason> = Vec::new();
    if flags.honeypot == Tri::Yes {
        reasons.push(RiskReason::Honeypot);
    }
    if let Some(t) = flags.buy_tax {
        if t > TAX_FLAG_FRACTION {
            reasons.push(RiskReason::BuyTax(t));
        }
    }
    if let Some(t) = flags.sell_tax {
        if t > TAX_FLAG_FRACTION {
            reasons.push(RiskReason::SellTax(t));
        }
    }
    if flags.can_mint == Tri::Yes {
        reasons.push(RiskReason::Mintable);
    }
    if flags.is_blacklist == Tri::Yes {
        reasons.push(RiskReason::BlacklistCapable);
    }
    if flags.owner_can_change_balance == Tri::Yes {
        reasons.push(RiskReason::OwnerCanChangeBalance);
    }
    if flags.verified == Tri::No {
        reasons.push(RiskReason::SourceUnverified);
    }
    if flags.is_proxy == Tri::Yes && flags.verified != Tri::Yes {
        reasons.push(RiskReason::ProxyUnverifiedSource);
    }

    if !reasons.is_empty() {
        return TokenVerdict {
            grade: RiskGrade::Flagged,
            flags,
            reasons,
            source: "goplus",
            asof_ms: crate::events::now_ms(),
        };
    }

    // Fail-closed: a row we can't fully assess is not a clean row.
    let mut missing: Vec<RiskReason> = Vec::new();
    for (tri, name) in [
        (flags.honeypot, "is_honeypot"),
        (flags.can_mint, "is_mintable"),
        (flags.is_blacklist, "is_blacklisted"),
        (flags.owner_can_change_balance, "owner_change_balance"),
        (flags.is_proxy, "is_proxy"),
        (flags.verified, "is_open_source"),
    ] {
        if tri == Tri::Unknown {
            missing.push(RiskReason::FieldMissing(name));
        }
    }
    if flags.buy_tax.is_none() {
        missing.push(RiskReason::FieldMissing("buy_tax"));
    }
    if flags.sell_tax.is_none() {
        missing.push(RiskReason::FieldMissing("sell_tax"));
    }

    TokenVerdict {
        grade: if missing.is_empty() {
            RiskGrade::Clear
        } else {
            RiskGrade::Unverified
        },
        flags,
        reasons: missing,
        source: "goplus",
        asof_ms: crate::events::now_ms(),
    }
}

struct CacheEntry {
    verdict: TokenVerdict,
    expires_ms: u64,
}

/// Bounded GoPlus client + TTL verdict cache.
pub struct TokenRiskClient {
    http: reqwest::Client,
    base: String,
    ttl: Duration,
    cache: Mutex<HashMap<String, CacheEntry>>,
}

impl Default for TokenRiskClient {
    fn default() -> Self {
        Self::new()
    }
}

impl TokenRiskClient {
    /// `GOPLUS_BASE_URL` overrides the endpoint (tests / staging).
    pub fn new() -> Self {
        let base =
            std::env::var("GOPLUS_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE.to_string());
        TokenRiskClient {
            http: reqwest::Client::new(),
            base,
            ttl: DEFAULT_TTL,
            cache: Mutex::new(HashMap::new()),
        }
    }

    fn key(chain_id: u64, addr: &str) -> String {
        format!("{chain_id}:{}", addr.to_lowercase())
    }

    /// Cache-only lookup — never blocks, never fetches.
    pub fn cached(&self, chain_id: u64, addr: &str) -> Option<TokenVerdict> {
        let now = crate::events::now_ms();
        let mut m = self.cache.lock().ok()?;
        let key = Self::key(chain_id, addr);
        match m.get(&key) {
            Some(e) if e.expires_ms > now => Some(e.verdict.clone()),
            Some(_) => {
                m.remove(&key);
                None
            }
            None => None,
        }
    }

    fn store(&self, chain_id: u64, addr: &str, verdict: TokenVerdict) {
        let now = crate::events::now_ms();
        let ttl_ms = match verdict.grade {
            RiskGrade::Unverified => UNVERIFIED_TTL.as_millis() as u64,
            _ => self.ttl.as_millis() as u64,
        };
        if let Ok(mut m) = self.cache.lock() {
            if m.len() >= MAX_CACHE {
                m.retain(|_, e| e.expires_ms > now);
            }
            while m.len() >= MAX_CACHE {
                let oldest = m
                    .iter()
                    .min_by_key(|(_, e)| e.expires_ms)
                    .map(|(k, _)| k.clone());
                match oldest {
                    Some(k) => {
                        m.remove(&k);
                    }
                    None => break,
                }
            }
            m.insert(
                Self::key(chain_id, addr),
                CacheEntry {
                    verdict,
                    expires_ms: now + ttl_ms,
                },
            );
        }
    }

    /// Assess one token; cache hit short-circuits the HTTP call.
    pub async fn check(&self, chain_id: u64, addr: &str) -> TokenVerdict {
        let owned = addr.to_string();
        self.check_many(chain_id, std::slice::from_ref(&owned))
            .await
            .into_iter()
            .next()
            .map(|(_, v)| v)
            .unwrap_or_else(|| TokenVerdict::unverified(RiskReason::NotInIndex))
    }

    /// Assess a batch (e.g. both pool legs). Every requested address gets
    /// a verdict — absent rows come back `Unverified`, never `Clear`.
    pub async fn check_many(
        &self,
        chain_id: u64,
        addrs: &[String],
    ) -> Vec<(String, TokenVerdict)> {
        let mut out: Vec<(String, TokenVerdict)> = Vec::with_capacity(addrs.len());
        let mut misses: Vec<String> = Vec::new();
        for a in addrs {
            let lc = a.to_lowercase();
            match self.cached(chain_id, &lc) {
                Some(v) => out.push((lc, v)),
                None => misses.push(lc),
            }
        }

        for chunk in misses.chunks(BATCH) {
            let csv = chunk.join(",");
            let url = format!(
                "{}/api/v1/token_security/{chain_id}?contract_addresses={csv}",
                self.base
            );
            let body = self
                .http
                .get(&url)
                .timeout(Duration::from_secs(15))
                .send()
                .await
                .and_then(|r| r.error_for_status());
            let parsed = match body {
                Ok(resp) => resp.json::<Value>().await.map_err(|e| e.to_string()),
                Err(e) => Err(e.to_string()),
            };
            match parsed {
                Err(e) => {
                    for a in chunk {
                        let v = TokenVerdict::unverified(RiskReason::Unreachable(e.clone()));
                        self.store(chain_id, a, v.clone());
                        out.push((a.clone(), v));
                    }
                }
                Ok(v) => {
                    if v["code"].as_i64() != Some(1) {
                        let msg = v["message"]
                            .as_str()
                            .unwrap_or("envelope error")
                            .to_string();
                        for a in chunk {
                            let verdict =
                                TokenVerdict::unverified(RiskReason::Malformed(msg.clone()));
                            self.store(chain_id, a, verdict.clone());
                            out.push((a.clone(), verdict));
                        }
                        continue;
                    }
                    let result = v["result"].as_object().cloned().unwrap_or_default();
                    for a in chunk {
                        // Keys are lowercased addresses — match case-insensitively.
                        let row = result
                            .iter()
                            .find(|(k, _)| k.eq_ignore_ascii_case(a))
                            .map(|(_, r)| r.clone());
                        let verdict = match row {
                            Some(r) => classify(&r),
                            None => TokenVerdict::unverified(RiskReason::NotInIndex),
                        };
                        self.store(chain_id, a, verdict.clone());
                        out.push((a.clone(), verdict));
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(fields: &str) -> Value {
        serde_json::from_str(&format!("{{{fields}}}")).unwrap()
    }

    const CLEAN: &str = r#"
        "is_honeypot":"0","buy_tax":"0","sell_tax":"0","is_mintable":"0",
        "is_blacklisted":"0","owner_change_balance":"0","is_proxy":"0",
        "is_open_source":"1""#;

    #[test]
    fn clean_token_is_clear() {
        let v = classify(&row(CLEAN));
        assert_eq!(v.grade, RiskGrade::Clear);
        assert!(v.is_clear());
        assert!(v.reasons.is_empty());
    }

    #[test]
    fn honeypot_is_flagged_with_reason() {
        let v = classify(&row(&CLEAN.replace(
            "\"is_honeypot\":\"0\"",
            "\"is_honeypot\":\"1\"",
        )));
        assert_eq!(v.grade, RiskGrade::Flagged);
        assert!(v.reasons.contains(&RiskReason::Honeypot));
        assert!(v.reason_codes().contains("HONEYPOT"));
    }

    #[test]
    fn sell_tax_is_flagged() {
        let v = classify(&row(&CLEAN.replace(
            "\"sell_tax\":\"0\"",
            "\"sell_tax\":\"0.99\"",
        )));
        assert_eq!(v.grade, RiskGrade::Flagged);
        assert!(v.reasons.contains(&RiskReason::SellTax(0.99)));
    }

    #[test]
    fn mintable_and_blacklist_and_owner_flag() {
        let v = classify(&row(
            &CLEAN
                .replace("\"is_mintable\":\"0\"", "\"is_mintable\":\"1\"")
                .replace("\"is_blacklisted\":\"0\"", "\"is_blacklisted\":\"1\"")
                .replace(
                    "\"owner_change_balance\":\"0\"",
                    "\"owner_change_balance\":\"1\"",
                ),
        ));
        assert_eq!(v.grade, RiskGrade::Flagged);
        for r in [
            RiskReason::Mintable,
            RiskReason::BlacklistCapable,
            RiskReason::OwnerCanChangeBalance,
        ] {
            assert!(v.reasons.contains(&r), "missing {r:?}");
        }
    }

    #[test]
    fn proxy_without_verified_source_flags() {
        let v = classify(&row(
            &CLEAN
                .replace("\"is_proxy\":\"0\"", "\"is_proxy\":\"1\"")
                .replace("\"is_open_source\":\"1\"", "\"is_open_source\":\"0\""),
        ));
        assert_eq!(v.grade, RiskGrade::Flagged);
        assert!(v
            .reasons
            .contains(&RiskReason::ProxyUnverifiedSource));
        assert!(v.reasons.contains(&RiskReason::SourceUnverified));
    }

    #[test]
    fn missing_field_is_unverified_never_clear() {
        let v = classify(&row(&CLEAN.replace("\"is_honeypot\":\"0\",", "")));
        assert_eq!(v.grade, RiskGrade::Unverified);
        assert!(v
            .reasons
            .contains(&RiskReason::FieldMissing("is_honeypot")));
    }

    #[test]
    fn empty_tax_field_is_unverified() {
        let v = classify(&row(&CLEAN.replace(
            "\"buy_tax\":\"0\"",
            "\"buy_tax\":\"\"",
        )));
        assert_eq!(v.grade, RiskGrade::Unverified);
        assert!(v.reasons.contains(&RiskReason::FieldMissing("buy_tax")));
    }

    #[test]
    fn unverified_is_not_safe() {
        let v = TokenVerdict::unverified(RiskReason::Unreachable("down".into()));
        assert!(!v.is_clear());
        assert_eq!(v.grade, RiskGrade::Unverified);
    }
}
