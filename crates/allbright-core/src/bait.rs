//! Bait-pool conviction — pool-level suppression of fake-reserve pools.
//!
//! Forensic signature (evidence: 7,380 exec failures on arb):
//! a pool advertises reserves via getReserves()/slot0() so our simulation
//! shows a spread, but a real swap reverts inside the venue's own
//! invariant check (e.g. `Error("CamelotPair: K")` — 0x08c379a0).
//! The on-chain minProfit guard already bounds the loss to zero — but
//! feeding the same pools wastes the entire funnel. Conviction is at
//! POOL level and persists in `data/leaders/<chain>/_bait_pools.json`.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::events::Chain;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BaitConviction {
    pub reason: String,
    pub reverts: u64,
    pub first_ms: u64,
    pub last_ms: u64,
}

/// Per-chain conviction map — pool address (lowercased) → evidence.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BaitRegistry {
    pub pools: HashMap<String, BaitConviction>,
}

impl BaitRegistry {
    pub fn path_for(chain: Chain) -> PathBuf {
        PathBuf::from(format!("data/leaders/{}/_bait_pools.json", chain.name()))
    }

    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Atomic write (tmp + rename) — same pattern as the RPC registry.
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn convict(&mut self, pool: &str, reason: &str) {
        let key = pool.to_lowercase();
        let now = crate::events::now_ms();
        let e = self.pools.entry(key).or_insert_with(|| BaitConviction {
            reason: reason.to_string(),
            reverts: 0,
            first_ms: now,
            last_ms: now,
        });
        e.reverts += 1;
        e.last_ms = now;
        e.reason = reason.to_string();
    }

    /// Probation horizon — a conviction ages out after 24 h so a wrongly
    /// convicted pool recovers automatically. If still bait, the next
    /// Error(string) revert re-convicts it; the on-chain minProfit guard
    /// keeps that retry at zero capital cost.
    pub const CONVICTION_TTL_MS: u64 = 24 * 3600 * 1000;

    pub fn is_convicted(&self, pool: &str) -> bool {
        self.pools
            .get(&pool.to_lowercase())
            .map(|c| {
                crate::events::now_ms().saturating_sub(c.last_ms)
                    < Self::CONVICTION_TTL_MS
            })
            .unwrap_or(false)
    }

    /// Drop convictions past the probation horizon (call on reload).
    pub fn prune(&mut self, now_ms: u64) {
        self.pools
            .retain(|_, c| now_ms.saturating_sub(c.last_ms) < Self::CONVICTION_TTL_MS);
    }
}

/// Decode a Solidity `Error(string)` revert reason embedded anywhere in an
/// error blob (Pimlico wraps the hex inside a JSON message). Returns None
/// when no 0x08c379a0 payload is present or it fails ABI layout checks.
pub fn decode_error_string(detail: &str) -> Option<String> {
    let idx = detail.find("08c379a0")?;
    let hex: String = detail[idx + 8..]
        .chars()
        .take_while(|c| c.is_ascii_hexdigit())
        .collect();
    if hex.len() < 128 {
        return None;
    }
    let bytes: Vec<u8> = (0..hex.len() / 2)
        .filter_map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).ok())
        .collect();
    // ABI: 32B offset (==0x20) + 32B length + UTF-8 payload.
    let offset = u64::from_be_bytes(bytes[24..32].try_into().ok()?) as usize;
    if offset + 32 > bytes.len() {
        return None;
    }
    let len =
        u64::from_be_bytes(bytes[offset + 24..offset + 32].try_into().ok()?) as usize;
    if offset + 32 + len > bytes.len() || len == 0 || len > 512 {
        return None;
    }
    String::from_utf8(bytes[offset + 32..offset + 32 + len].to_vec()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn error_string_blob(reason: &str) -> String {
        // ABI Error(string): selector + offset(0x20) + len + padded data
        let data = reason.as_bytes();
        let pad = (32 - data.len() % 32) % 32;
        format!(
            "pimlico err: ...0x08c379a0{:064x}{:064x}{}0{}",
            32,
            data.len(),
            data.iter().map(|b| format!("{b:02x}")).collect::<String>(),
            "0".repeat(pad.saturating_sub(1))
        )
    }

    #[test]
    fn decodes_camelot_k_revert() {
        let blob = error_string_blob("CamelotPair: K");
        assert_eq!(
            decode_error_string(&blob).as_deref(),
            Some("CamelotPair: K")
        );
    }

    #[test]
    fn ignores_non_error_reverts() {
        assert_eq!(decode_error_string("execution reverted"), None);
        assert_eq!(decode_error_string("0x08c379a0"), None); // truncated
        // Panic(uint256) selector must not decode as Error(string)
        assert_eq!(
            decode_error_string(&format!("0x4e487b71{}", "0".repeat(64))),
            None
        );
    }

    #[test]
    fn convict_is_case_insensitive() {
        let mut r = BaitRegistry::default();
        r.convict("0xABCDEF", "CamelotPair: K");
        assert!(r.is_convicted("0xabcdef"));
        assert!(r.is_convicted("0xABCDEF"));
        assert!(!r.is_convicted("0x1234"));
        // second conviction accumulates evidence
        r.convict("0xabcdef", "CamelotPair: K");
        assert_eq!(r.pools["0xabcdef"].reverts, 2);
    }

    #[test]
    fn conviction_decays_after_ttl() {
        let mut r = BaitRegistry::default();
        r.convict("0xaaaa", "CamelotPair: K");
        assert!(r.is_convicted("0xaaaa"));
        // age it past the probation horizon
        r.pools.get_mut("0xaaaa").unwrap().last_ms =
            crate::events::now_ms() - BaitRegistry::CONVICTION_TTL_MS - 1;
        assert!(!r.is_convicted("0xaaaa"));
        r.prune(crate::events::now_ms());
        assert!(r.pools.is_empty());
    }

    #[test]
    fn registry_roundtrips_disk() {
        let dir = std::env::temp_dir().join(format!("bait-{}", std::process::id()));
        let path = dir.join("_bait_pools.json");
        let mut r = BaitRegistry::default();
        r.convict("0xDeadBeef", "CamelotPair: K");
        r.save(&path).unwrap();
        let loaded = BaitRegistry::load(&path);
        assert!(loaded.is_convicted("0xdeadbeef"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
