//! Peer metrics collector (AGENTS §27.2–27.3) — the relay Data API
//! import that converts builder peer cells from hand-curated docs into
//! LIVE measured data. Same proven source lineage as relayscan.io:
//! `GET /relay/v1/data/bidtraces/proposer_payload_delivered`.
//!
//! Runs as a background task: polls public MEV-Boost relays, dedupes
//! delivered payloads by slot across a rolling 24h window, aggregates
//! per-builder share/value, and persists `data/peer_metrics.json`.
//! The competitive API overlays this file — peer cells marked
//! `confidence: HIGH` here are measured, not marketing.

use serde_json::{json, Value};
use std::collections::HashMap;
use tracing::{info, warn};

const FILE: &str = "data/peer_metrics.json";
/// Proposer-payload endpoint path (relay-specs BidTraceV3).
const BIDTRACES: &str = "/relay/v1/data/bidtraces/proposer_payload_delivered";
/// Public relay Data API hosts (submission-side hosts are irrelevant —
/// these expose the documented GET endpoints).
const RELAYS: &[(&str, &str)] = &[
    ("flashbots", "https://boost-relay.flashbots.net"),
    ("ultrasound", "https://relay.ultrasound.money"),
    ("aestus", "https://aestus.live"),
    ("titan", "https://rpc.titanrelay.xyz"),
    ("agnostic", "https://agnostic-relay.net"),
    ("ethgas", "https://relay.ethgas.com"),
    ("bloxroute", "https://bloxroute.max-profit.blxrbdn.com"),
];
/// Keep this many trailing slots (~24h at 12s slots).
const WINDOW_SLOTS: u64 = 7_200;
const POLL_SECS: u64 = 20 * 60;

/// Normalize a builder's `extra_data` tag / pubkey into an entity id.
/// Mirrors register ids so live data lands on the right peer.
fn entity_id(tag: &str) -> String {
    let t = tag.to_lowercase();
    for (pat, id) in [
        ("beaver", "beaverbuild"), ("titan", "titan"), ("rsync", "rsync"),
        ("flashbots", "flashbots"), ("bloxroute", "bloxroute"),
        ("builder0x69", "builder0x69"), ("buildernet", "buildernet"),
        ("quasar", "quasar"), ("ultrasound", "ultrasound"),
        ("aestus", "aestus"), ("agnostic", "agnostic"),
        ("ethgas", "ethgas"), ("jetbldr", "jetbldr"),
        ("loki", "lokibuilder"), ("boba", "bobthebuilder"),
        ("gambit", "gambit"), ("lightbeam", "lightbeam"),
        ("payload", "payload"), ("tatsu", "tatsu"),
        ("nimbus", "nimbus"),
    ] {
        if t.contains(pat) {
            return id.to_string();
        }
    }
    // Unknown tag — use it verbatim (truncated) so new builders surface.
    t.chars().take(24).filter(|c| c.is_ascii_alphanumeric()).collect()
}

async fn fetch_payloads(
    client: &reqwest::Client,
    base: &str,
) -> anyhow::Result<Vec<Value>> {
    let url = format!("{base}{BIDTRACES}?limit=200");
    let v: Value = client
        .get(&url)
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(v.as_array().cloned().unwrap_or_default())
}

fn load_seen() -> HashMap<String, Value> {
    std::fs::read_to_string(FILE)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| {
            v.get("slots")?.as_object().map(|m| {
                m.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
            })
        })
        .unwrap_or_default()
}

fn aggregate(seen: &HashMap<String, Value>, min_slot: u64, ok_relays: &[&str]) -> Value {
    let mut builders: HashMap<String, (u64, f64)> = HashMap::new();
    let mut relays: HashMap<String, u64> = HashMap::new();
    let mut total = 0u64;
    for e in seen.values() {
        let slot = e["slot"].as_u64().unwrap_or(0);
        if slot < min_slot {
            continue;
        }
        total += 1;
        let bid = e["builder"].as_str().unwrap_or("unknown").to_string();
        let wei = e["value_wei"].as_str().and_then(|s| s.parse::<u128>().ok()).unwrap_or(0);
        let b = builders.entry(bid).or_default();
        b.0 += 1;
        b.1 += wei as f64 / 1e18;
        *relays.entry(e["relay"].as_str().unwrap_or("?").to_string()).or_default() += 1;
    }
    let mut bj: Vec<Value> = builders
        .into_iter()
        .map(|(id, (n, eth))| {
            json!({"id": id, "payloads": n,
                   "share": if total > 0 { n as f64 / total as f64 } else { 0.0 },
                   "value_eth": eth})
        })
        .collect();
    bj.sort_by(|a, b| b["payloads"].as_u64().cmp(&a["payloads"].as_u64()));
    json!({
        "asof_ms": allbright_core::events::now_ms(),
        "window": "24h",
        "slots_seen": total,
        "sources_ok": ok_relays,
        "builders": bj,
        "relays": relays,
        "note": "Live-measured via public MEV-Boost relay Data API (§27.2). Share = delivered payloads over trailing window.",
    })
}

async fn poll_once(client: &reqwest::Client) {
    let mut seen = load_seen();
    let mut max_slot = 0u64;
    let mut ok_relays = Vec::new();
    let mut failures = Vec::new();
    for (name, base) in RELAYS {
        match fetch_payloads(client, base).await {
            Ok(rows) => {
                ok_relays.push(*name);
                for r in rows {
                    let Some(slot) = r.get("slot").and_then(|v| v.as_u64()) else { continue };
                    max_slot = max_slot.max(slot);
                    // extra_data = builder self-tag; pubkey tail as fallback.
                    let tag = r
                        .get("extra_data")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                        .map(|s| s.to_string())
                        .or_else(|| {
                            r.get("builder_pubkey")
                                .and_then(|v| v.as_str())
                                .map(|p| format!("pk:{}", &p[p.len().saturating_sub(8)..]))
                        })
                        .unwrap_or_else(|| "unknown".into());
                    seen.insert(
                        format!("{name}:{slot}"),
                        json!({
                            "slot": slot, "relay": name,
                            "builder": entity_id(&tag),
                            "tag": tag,
                            "value_wei": r.get("value").and_then(|v| v.as_str()).unwrap_or("0"),
                        }),
                    );
                }
            }
            Err(e) => {
                warn!(relay = name, err = %e, "peer metrics fetch failed");
                failures.push(json!({"relay": name, "err": e.to_string()}));
            }
        }
    }
    if max_slot == 0 {
        return; // all relays down — keep prior file untouched
    }
    let min_slot = max_slot.saturating_sub(WINDOW_SLOTS);
    seen.retain(|_, e| e["slot"].as_u64().unwrap_or(0) >= min_slot);
    let mut out = aggregate(&seen, min_slot, &ok_relays);
    if !failures.is_empty() {
        out["fetch_errors"] = json!(failures);
    }
    // slots map retained inside file for cross-poll dedup (bounded: ~7.2k).
    out["slots"] = json!(seen);
    match std::fs::write(FILE, serde_json::to_string(&out).unwrap_or_default()) {
        Ok(()) => info!(builders = out["builders"].as_array().map(|a| a.len()).unwrap_or(0),
                        slots = out["slots_seen"].as_u64().unwrap_or(0), "peer metrics refreshed"),
        Err(e) => warn!(err = %e, "peer metrics write failed"),
    }
}

pub async fn run() {
    let _ = std::fs::create_dir_all("data");
    let client = reqwest::Client::new();
    loop {
        poll_once(&client).await;
        tokio::time::sleep(std::time::Duration::from_secs(POLL_SECS)).await;
    }
}
