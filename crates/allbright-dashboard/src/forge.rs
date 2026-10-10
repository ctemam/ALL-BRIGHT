//! Forge worker (AGENTS §27.5) — the fourth autonomous loop, after
//! scout (infrastructure), curator (universe), sentinel (healing).
//!
//! Industry pattern: SafeDeploy-style *stateful workflows* — gaps are
//! persistent tasks with lifecycle, not render-time badges. Tasks are
//! dedup'd by axis, updated on every evaluation, and **self-close as
//! MEASURED when the fitness function improves** — the loop closes
//! itself with evidence, not with a human click.
//!
//! Autonomy boundary (error-budget gating analogue):
//!   SAFE     — a running system loop already owns the fix
//!              (pool coverage -> curator, RPC freshness -> scout).
//!              Status IN_FLIGHT while the loop works.
//!   ESCALATE — inert proposal. Engineering/economics authority
//!              required. NEVER auto-executed: no threshold changes,
//!              no code changes, no risk weakening. Commander sees it,
//!              acknowledges it, work happens through engineering.

use serde_json::{json, Value};
use std::collections::HashMap;
use tracing::{info, warn};

use crate::api::{self, Shared};

const FILE: &str = "data/gap_tasks.json";
const POLL_SECS: u64 = 10 * 60;

/// Axes whose gaps are closable by an already-running autonomous loop.
/// Everything else is ESCALATE — an inert proposal, never an action.
fn safe_owner(axis: &str) -> Option<&'static str> {
    match axis {
        "Route optimization" => Some("allbright-pool-curator"),
        "Data speed & state freshness" => Some("allbright-rpc-scout"),
        _ => None,
    }
}

fn load_tasks() -> Vec<Value> {
    std::fs::read_to_string(FILE)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn status_of(t: &Value) -> &str {
    t["status"].as_str().unwrap_or("OPEN")
}

/// Upsert gap evidence into the task register. Returns change count.
fn sync(tasks: &mut Vec<Value>, gaps: &[Value], trend: &serde_json::Map<String, Value>) -> usize {
    let now = allbright_core::events::now_ms();
    let mut changed = 0;
    let mut by_axis: HashMap<String, usize> = tasks
        .iter()
        .enumerate()
        .filter_map(|(i, t)| t["axis"].as_str().map(|a| (a.to_string(), i)))
        .collect();
    for g in gaps {
        let Some(axis) = g["axis"].as_str() else { continue };
        let gap = g["gap"].as_i64().unwrap_or(0);
        let regressed = trend
            .get(axis)
            .and_then(|t| t["delta"].as_i64())
            .map(|d| d < 0)
            .unwrap_or(false);
        let peer = g["best_peer"]["id"].as_str().unwrap_or("").to_string();
        let peer_rank = g["best_peer"]["rank"].as_i64().unwrap_or(0);
        let basis = g["basis"].as_str().unwrap_or("").to_string();
        if let Some(&i) = by_axis.get(axis) {
            // Existing task: refresh evidence; close if measurement says so.
            let t = &mut tasks[i];
            let st = status_of(t).to_string();
            if gap <= 0 && st != "MEASURED" {
                t["status"] = json!("MEASURED");
                t["closed_ms"] = json!(now);
                t["note"] = json!("closed by measurement — peer par reached");
                changed += 1;
            } else if gap > 0 {
                t["gap"] = json!(gap);
                t["our_rank"] = g["our_rank"].clone();
                t["peer"] = json!(peer);
                t["peer_rank"] = json!(peer_rank);
                t["basis"] = json!(basis);
                t["updated_ms"] = json!(now);
                if regressed && st == "MEASURED" {
                    t["status"] = json!("OPEN"); // re-forged — regression reopened it
                    t["priority"] = json!("HIGH");
                    changed += 1;
                } else if regressed {
                    t["priority"] = json!("HIGH");
                    t["regression"] = json!(true);
                }
            }
        } else if gap > 0 {
            // New gap → new stateful task.
            let (cls, status, owner) = match safe_owner(axis) {
                Some(o) => ("SAFE", "IN_FLIGHT", json!(o)),
                None => ("ESCALATE", "ESCALATED", json!("engineering")),
            };
            tasks.push(json!({
                "axis": axis, "gap": gap,
                "our_rank": g["our_rank"], "our_rating": g["allbright"],
                "peer": peer, "peer_rank": peer_rank,
                "basis": basis,
                "action_class": cls, "status": status, "owner": owner,
                "priority": if regressed { "HIGH" } else { "NORMAL" },
                "regression": regressed,
                "created_ms": now, "updated_ms": now,
            }));
            by_axis.insert(axis.to_string(), tasks.len() - 1);
            changed += 1;
        }
    }
    changed
}

pub async fn run(st: Shared) {
    let _ = std::fs::create_dir_all("data");
    loop {
        let d = api::competitive_json(&st, None).await;
        let gaps = d["gap_matrix"].as_array().cloned().unwrap_or_default();
        let trend = d["axis_trend"].as_object().cloned().unwrap_or_default();
        let mut tasks = load_tasks();
        let changed = sync(&mut tasks, &gaps, &trend);
        if changed > 0 {
            match std::fs::write(FILE, serde_json::to_string_pretty(&tasks).unwrap_or_default()) {
                Ok(()) => info!(changed, tasks = tasks.len(), "forge tasks synced"),
                Err(e) => warn!(err = %e, "forge task write failed"),
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(POLL_SECS)).await;
    }
}
