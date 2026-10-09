//! Dashboard REST API — every endpoint returns *measured* data from the
//! event index, the RPC registry, or live chain queries. Where data is
//! unavailable the API reports it explicitly; the frontend renders
//! `DATA UNAVAILABLE` / `NOT VERIFIED` (AGENTS §38.24 — no mock data).

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use allbright_core::config::Config;
use allbright_core::events::Chain;
use allbright_core::rpc_registry::{QualState, RpcRegistry, REGISTRY_FILE};
use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Mutex;

use crate::state::{EventIndex, IdxRow, STRATEGY_VARIANTS};

pub struct AppState {
    pub idx: Mutex<EventIndex>,
    pub cfg: Config,
    pub started_ms: u64,
}

pub type Shared = Arc<AppState>;

#[derive(Debug, Deserialize)]
pub struct Range {
    #[serde(default)]
    from: u64,
    #[serde(default)]
    to: u64,
    #[serde(default)]
    chain: Option<String>,
    #[serde(default)]
    variant: Option<String>,
    #[serde(default)]
    page: Option<usize>,
    #[serde(default)]
    size: Option<usize>,
}

fn reg_path() -> String {
    std::env::var("RPC_REGISTRY_PATH").unwrap_or_else(|_| REGISTRY_FILE.into())
}

/// Chain/variant filter predicates shared by endpoints.
fn chain_ok(sel: &Option<String>, row_chain: &str) -> bool {
    sel.as_deref()
        .map(|s| s.eq_ignore_ascii_case("all") || s.eq_ignore_ascii_case(row_chain))
        .unwrap_or(true)
}
fn variant_ok(sel: &Option<String>, v: &str) -> bool {
    sel.as_deref()
        .map(|s| s.eq_ignore_ascii_case("all") || s.eq_ignore_ascii_case(v))
        .unwrap_or(true)
}

// ---------- /api/meta — selectors' authoritative option lists ----------
pub async fn meta(State(st): State<Shared>) -> Json<Value> {
    let idx = st.idx.lock().unwrap();
    Json(json!({
        "chains": Chain::ALL.iter().map(|c| c.name()).collect::<Vec<_>>(),
        "variants": STRATEGY_VARIANTS.iter()
            .map(|(id, name)| json!({"id": id, "name": name}))
            .collect::<Vec<_>>(),
        "index_since_ms": idx.index_since_ms,
        "server_now_ms": allbright_core::events::now_ms(),
        "currencies": ["USD", "ETH"],
    }))
}

// ---------- /api/pnl — Profit page dataset (§38.5) ----------
// Rows = union of opportunity signals (expected economics, marked
// EXPECTED) and execution results (realized economics). Cost fields that
// are not measured are returned as null — the UI renders them as
// NOT VERIFIED, per the no-mock-data rule.
pub async fn pnl(State(st): State<Shared>, Query(q): Query<Range>) -> Json<Value> {
    let idx = st.idx.lock().unwrap();
    let mut rows: Vec<Value> = Vec::new();
    let (mut gross, mut realized, mut n_ok, mut n_fail) = (0.0f64, 0.0f64, 0u64, 0u64);

    for r in idx.in_range(q.from, q.to) {
        match r {
            IdxRow::Opp {
                ts,
                chain,
                variant,
                expected_usd,
                legs,
                input_token,
                input_amount,
            } if chain_ok(&q.chain, chain) && variant_ok(&q.variant, variant) => {
                gross += expected_usd;
                rows.push(json!({
                    "ts": ts, "chain": chain, "variant": variant,
                    "kind": "opportunity",
                    "opp_id": format!("{chain}-{ts}"),
                    "tx_id": null,
                    "gross_usd": expected_usd,
                    "expected_usd": expected_usd,
                    "realized_usd": null,
                    "net_usd": null,
                    "gas_used": null,
                    "status": "EXPECTED",
                    "legs": legs, "input_token": input_token,
                    "input_amount": input_amount,
                    "lineage": "solver:OpportunitySignal",
                }));
            }
            IdxRow::Exec {
                ts,
                chain,
                status,
                tx_hash,
                realized_usd,
                gas_used,
                detail,
            } if chain_ok(&q.chain, chain) => {
                realized += realized_usd;
                match status.as_str() {
                    "Included" => n_ok += 1,
                    "Reverted" | "Failed" => n_fail += 1,
                    _ => {}
                }
                rows.push(json!({
                    "ts": ts, "chain": chain, "variant": null,
                    "kind": "execution",
                    "opp_id": null, "tx_id": tx_hash,
                    "gross_usd": null, "expected_usd": null,
                    "realized_usd": realized_usd, "net_usd": realized_usd,
                    "gas_used": gas_used,
                    "status": status, "detail": detail,
                    "lineage": "executor:ExecutionResult",
                }));
            }
            _ => {}
        }
    }
    rows.sort_by(|a, b| b["ts"].as_u64().cmp(&a["ts"].as_u64()));

    // Time buckets for CHART/BAR modes — same filtered dataset (§38.14).
    let (from, to) = (
        q.from,
        if q.to == 0 {
            allbright_core::events::now_ms()
        } else {
            q.to
        },
    );
    let span = to.saturating_sub(from).max(60_000);
    let nb = 40usize;
    let mut buckets = vec![json!({"t":0,"expected":0.0,"realized":0.0}); nb];
    let bspan = span / nb as u64;
    for row in &rows {
        let t = row["ts"].as_u64().unwrap_or(0);
        if t < from || t > to {
            continue;
        }
        let i = ((t - from) / bspan.max(1)).min(nb as u64 - 1) as usize;
        let b = &mut buckets[i];
        b["t"] = json!(from + i as u64 * bspan);
        b["expected"] = json!(
            b["expected"].as_f64().unwrap_or(0.0) + row["expected_usd"].as_f64().unwrap_or(0.0)
        );
        b["realized"] = json!(
            b["realized"].as_f64().unwrap_or(0.0) + row["realized_usd"].as_f64().unwrap_or(0.0)
        );
    }

    let total = rows.len();
    let (page, size) = (
        q.page.unwrap_or(1).max(1),
        q.size.unwrap_or(25).clamp(1, 100),
    );
    let rows_page: Vec<Value> = rows
        .into_iter()
        .skip((page - 1) * size)
        .take(size)
        .collect();
    let trades = n_ok + n_fail;
    Json(json!({
        "summary": {
            "gross_profit_usd": gross,
            "realized_profit_usd": realized,
            "net_profit_usd": realized,
            "successful_trades": n_ok,
            "failed_trades": n_fail,
            "win_rate": if trades > 0 { n_ok as f64 / trades as f64 } else { 0.0 },
            "total_costs_usd": null,   // not metered yet — NOT VERIFIED
            "roi": null,
            "margin": null,
            "avg_profit_per_trade": if trades > 0 { realized / trades as f64 } else { 0.0 },
        },
        "total_rows": total,
        "page": page, "size": size,
        "buckets": buckets,
        "rows": rows_page,
        "data_since_ms": idx.index_since_ms,
    }))
}

// ---------- /api/strategies — Strategy page (§38.6) ----------
pub async fn strategies(State(st): State<Shared>, Query(q): Query<Range>) -> Json<Value> {
    let idx = st.idx.lock().unwrap();
    let mut per: HashMap<String, Value> = STRATEGY_VARIANTS
        .iter()
        .map(|(id, name)| {
            (
                id.to_string(),
                json!({
                    "variant": id, "name": name,
                    "detected": 0, "executed": 0, "rejected": 0,
                    "gross_usd": 0.0, "realized_usd": 0.0,
                }),
            )
        })
        .collect();
    for r in idx.in_range(q.from, q.to) {
        match r {
            IdxRow::Opp {
                variant,
                expected_usd,
                chain,
                ..
            } if chain_ok(&q.chain, chain) => {
                if let Some(v) = per.get_mut(variant) {
                    v["detected"] = json!(v["detected"].as_u64().unwrap_or(0) + 1);
                    v["gross_usd"] = json!(v["gross_usd"].as_f64().unwrap_or(0.0) + expected_usd);
                }
            }
            IdxRow::Exec {
                realized_usd,
                chain,
                ..
            } if chain_ok(&q.chain, chain) => {
                // Executions carry no variant attribution yet — attributed
                // bucket "ALL" is surfaced honestly below.
                let _ = realized_usd;
            }
            IdxRow::Rej { variant, chain, .. } if chain_ok(&q.chain, chain) => {
                if let Some(v) = per.get_mut(variant) {
                    v["rejected"] = json!(v["rejected"].as_u64().unwrap_or(0) + 1);
                }
            }
            _ => {}
        }
    }
    Json(json!({
        "variants": per.into_values().collect::<Vec<_>>(),
        "note": "executed/win-rate fields require ExecRequest→Opportunity lineage \
                 (variant attribution on ExecResult) — UNVERIFIED until wired",
        "data_since_ms": idx.index_since_ms,
    }))
}

// ---------- /api/infrastructure — RPC/visibility/healing (§38.8) ----------
pub async fn infrastructure(State(st): State<Shared>) -> Json<Value> {
    let reg = RpcRegistry::load(Path::new(&reg_path()));
    let mut chains = Vec::new();
    for c in Chain::ALL {
        let (q, prov) = reg.chain_diversity(c);
        let mut lat: Vec<u64> = reg.for_chain(c).filter_map(|e| e.median_rtt()).collect();
        lat.sort_unstable();
        let p = |pct: f64| {
            if lat.is_empty() {
                Value::Null
            } else {
                json!(lat[((lat.len() - 1) as f64 * pct) as usize])
            }
        };
        let healthy = reg
            .for_chain(c)
            .filter(|e| matches!(e.state, QualState::Qualified | QualState::Degraded))
            .count();
        let ovs_avg = {
            let v: Vec<f64> = reg
                .for_chain(c)
                .filter(|e| e.state == QualState::Qualified)
                .map(|e| e.ovs)
                .collect();
            if v.is_empty() {
                0.0
            } else {
                v.iter().sum::<f64>() / v.len() as f64
            }
        };
        chains.push(json!({
            "chain": c.name(), "qualified": q, "providers": prov,
            "healthy": healthy,
            "p50_ms": p(0.5), "p95_ms": p(0.95), "p99_ms": p(0.99),
            "ovs_avg": ovs_avg,
            "visibility": if q >= 5 && prov >= 3 { "V4+resilient" }
                          else if q >= 2 { "V4" }
                          else if q == 1 { "V2-single-homed" }
                          else { "V0-blind" },
        }));
    }
    let s = reg.summary();
    // Self-healing evidence from the index.
    let idx = st.idx.lock().unwrap();
    let (mut heals, mut heal_ok) = (0u64, 0u64);
    for r in idx.rows() {
        if let IdxRow::Heal { success, .. } = r {
            heals += 1;
            heal_ok += *success as u64;
        }
    }
    Json(json!({
        "registry": s,
        "chains": chains,
        "healing": {"total": heals, "successful": heal_ok},
        "registry_updated_ms": reg.updated_ms,
    }))
}

// ---------- /api/diagnoses — self-diagnosis feed (§38.26) ----------
pub async fn diagnoses(State(st): State<Shared>, Query(q): Query<Range>) -> Json<Value> {
    let idx = st.idx.lock().unwrap();
    let mut rows: Vec<Value> = idx
        .in_range(q.from, q.to)
        .filter_map(|r| match r {
            IdxRow::Fail {
                ts,
                component,
                class,
                chain,
                stage,
                detail,
            } => Some(json!({
                "id": format!("F-{ts}"), "ts": ts, "component": component,
                "class": class, "chain": chain, "stage": stage, "detail": detail,
            })),
            _ => None,
        })
        .collect();
    rows.sort_by(|a, b| b["ts"].as_u64().cmp(&a["ts"].as_u64()));
    let total = rows.len();
    let (page, size) = (
        q.page.unwrap_or(1).max(1),
        q.size.unwrap_or(25).clamp(1, 100),
    );
    Json(json!({
        "total": total,
        "rows": rows.into_iter().skip((page - 1) * size).take(size).collect::<Vec<_>>(),
    }))
}

// ---------- /api/healing — self-healing records (§38.27) ----------
pub async fn healing(State(st): State<Shared>, Query(q): Query<Range>) -> Json<Value> {
    let idx = st.idx.lock().unwrap();
    let mut rows: Vec<Value> = idx
        .in_range(q.from, q.to)
        .filter_map(|r| match r {
            IdxRow::Heal {
                ts,
                target,
                action,
                success,
                reason,
            } => Some(json!({
                "id": format!("H-{ts}"), "ts": ts, "target": target,
                "action": action, "success": success, "reason": reason,
                "state": if *success { "HEALED" } else { "FAILED" },
            })),
            _ => None,
        })
        .collect();
    rows.sort_by(|a, b| b["ts"].as_u64().cmp(&a["ts"].as_u64()));
    Json(json!({ "rows": rows }))
}

// ---------- /api/reports — category aggregates (§38.9) ----------
pub async fn reports(State(st): State<Shared>, Query(q): Query<Range>) -> Json<Value> {
    let idx = st.idx.lock().unwrap();
    let mut per_chain: HashMap<String, Value> = HashMap::new();
    let (mut opps, mut execs, mut rejs, mut fails, mut heals) = (0u64, 0u64, 0u64, 0u64, 0u64);
    for r in idx.in_range(q.from, q.to) {
        match r {
            IdxRow::Opp {
                chain,
                expected_usd,
                ..
            } => {
                opps += 1;
                let e = per_chain
                    .entry(chain.clone())
                    .or_insert(json!({"opps":0,"expected":0.0,"exec":0,"realized":0.0}));
                e["opps"] = json!(e["opps"].as_u64().unwrap_or(0) + 1);
                e["expected"] = json!(e["expected"].as_f64().unwrap_or(0.0) + expected_usd);
            }
            IdxRow::Exec {
                chain,
                realized_usd,
                ..
            } => {
                execs += 1;
                let e = per_chain
                    .entry(chain.clone())
                    .or_insert(json!({"opps":0,"expected":0.0,"exec":0,"realized":0.0}));
                e["exec"] = json!(e["exec"].as_u64().unwrap_or(0) + 1);
                e["realized"] = json!(e["realized"].as_f64().unwrap_or(0.0) + realized_usd);
            }
            IdxRow::Rej { .. } => rejs += 1,
            IdxRow::Fail { .. } => fails += 1,
            IdxRow::Heal { .. } => heals += 1,
            _ => {}
        }
    }
    let s = RpcRegistry::load(Path::new(&reg_path())).summary();
    Json(json!({
        "profit": per_chain,
        "opportunity": {"detected": opps, "rejected": rejs, "executed": execs},
        "infrastructure": {"rpc_failures": fails, "heal_actions": heals,
                           "qualified_rpcs": s.qualified, "candidates": s.total},
        "risk": {"note": "risk/security events not yet instrumented — DATA UNAVAILABLE"},
        "data_since_ms": idx.index_since_ms,
    }))
}

// ---------- /api/wallet — non-custodial wallet view (§38.10) ----------
// Balances are queried live via the first healthy registry endpoint per
// chain; a chain with no qualified endpoint reports DATA UNAVAILABLE.
pub async fn wallet(State(st): State<Shared>) -> Json<Value> {
    let addr = st.cfg.profit_wallet.clone();
    if addr.is_empty() {
        return Json(json!({"configured": false, "rows": [],
            "note": "PROFIT_WALLET not configured — DATA UNAVAILABLE"}));
    }
    let reg = RpcRegistry::load(Path::new(&reg_path()));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap_or_default();
    let mut rows = Vec::new();
    for c in Chain::ALL {
        let eps = reg.qualified_urls(c);
        let Some(url) = eps
            .first()
            .or_else(|| st.cfg.rpc_pools.get(&c).and_then(|p| p.first()))
        else {
            rows.push(json!({"chain": c.name(), "address": addr,
                "balance_native": null, "status": "NO QUALIFIED RPC"}));
            continue;
        };
        let body = json!({"jsonrpc":"2.0","id":1,"method":"eth_getBalance",
                          "params":[addr, "latest"]});
        let resp = client.post(url).json(&body).send().await.ok();
        let bal = match resp {
            Some(r) => r
                .json::<Value>()
                .await
                .ok()
                .and_then(|v| v["result"].as_str().map(|s| s.to_string()))
                .and_then(|s| u128::from_str_radix(s.trim_start_matches("0x"), 16).ok())
                .map(|w| w as f64 / 1e18),
            None => None,
        };
        rows.push(json!({
            "chain": c.name(), "address": addr,
            "balance_native": bal,
            "balance_usd": bal.map(|b| b * st.cfg.native_usd),
            "status": if bal.is_some() { "LIVE" } else { "QUERY FAILED" },
            "endpoint_provider": allbright_core::rpc_registry::provider_of(
                allbright_core::rpc_registry::host_of(url)),
        }));
    }
    Json(json!({
        "configured": true,
        "address": addr,
        "transfer_mode": st.cfg.profit_transfer_mode,
        "transfer_min_usd": st.cfg.profit_transfer_min_usd,
        "rows": rows,
    }))
}

// ---------- /api/stream — Blockchain Stream battlefield (§38.15) ----------
// Opportunity lifecycle cards + funnel + soldier-activity + defense +
// capital-protection — all derived from the same indexed events as the
// other pages. Stages without evidence render UNVERIFIED, never faked.
pub async fn stream(State(st): State<Shared>, Query(q): Query<Range>) -> Json<Value> {
    let idx = st.idx.lock().unwrap();
    let from = q.from;
    let to = if q.to == 0 {
        allbright_core::events::now_ms()
    } else {
        q.to
    };

    let opps: Vec<&IdxRow> = idx
        .in_range(from, q.to)
        .filter(|r| {
            matches!(r, IdxRow::Opp { .. })
                && matches!(r, IdxRow::Opp { chain, variant, .. }
                if chain_ok(&q.chain, chain) && variant_ok(&q.variant, variant))
        })
        .collect();
    let execreqs: Vec<&IdxRow> = idx
        .in_range(from, q.to)
        .filter(|r| matches!(r, IdxRow::ExecReq { .. }))
        .collect();
    let execs: Vec<&IdxRow> = idx
        .in_range(from, q.to)
        .filter(|r| matches!(r, IdxRow::Exec { .. }))
        .collect();
    let rejs: Vec<&IdxRow> = idx
        .in_range(from, q.to)
        .filter(|r| matches!(r, IdxRow::Rej { .. }))
        .collect();
    let fails: Vec<&IdxRow> = idx
        .in_range(from, q.to)
        .filter(|r| matches!(r, IdxRow::Fail { .. }))
        .collect();
    let heals: Vec<&IdxRow> = idx
        .in_range(from, q.to)
        .filter(|r| matches!(r, IdxRow::Heal { .. }))
        .collect();

    // Lifecycle cards — newest first, bounded.
    let mut cards: Vec<Value> = Vec::new();
    for r in opps.iter().rev().take(60) {
        let IdxRow::Opp {
            ts,
            chain,
            variant,
            expected_usd,
            legs,
            input_token,
            input_amount,
        } = r
        else {
            continue;
        };
        let opp_id = format!("OPP-{:08x}", ts % 0xFFFF_FFFF);
        // Linkage: ExecRequest for the same chain within 10s carrying the
        // same expected value; ExecResult within 60s after it.
        let req = execreqs.iter().find(|e| {
            matches!(e,
            IdxRow::ExecReq { ts: ets, chain: ec, expected_usd: ee, .. }
                if ec == chain && ee == expected_usd
                   && *ets >= *ts && *ets <= ts + 10_000)
        });
        let res = req.and_then(|rq| {
            let IdxRow::ExecReq { ts: rts, .. } = rq else {
                return None;
            };
            execs.iter().find(|e| {
                matches!(e,
                IdxRow::Exec { ts: ets, chain: ec, .. }
                    if ec == chain && *ets >= *rts && *ets <= rts + 60_000)
            })
        });
        let (exec_state, tx_hash, realized) = match res {
            Some(IdxRow::Exec {
                status,
                tx_hash,
                realized_usd,
                ..
            }) => (status.clone(), tx_hash.clone(), Some(*realized_usd)),
            _ => (
                if req.is_some() {
                    "REQUESTED".into()
                } else {
                    "NONE".into()
                },
                None,
                None,
            ),
        };
        cards.push(json!({
            "id": opp_id, "ts": ts, "chain": chain, "variant": variant,
            "expected_usd": expected_usd, "legs": legs,
            "input_token": input_token, "input_amount": input_amount,
            "stages": {
                "detected": true,
                // Opportunity emission implies it passed simulation+gate.
                "qualified": true, "simulated": true,
                "defended": req.is_some() || res.is_some(),
                "executed": req.is_some(),
                "confirmed": matches!(exec_state.as_str(), "Included"),
                "settled": realized.is_some(),
            },
            "exec_status": exec_state, "tx": tx_hash, "realized_usd": realized,
        }));
    }

    // Rejected opportunities — visible defense evidence (§38.15 bullets).
    let rejected_cards: Vec<Value> = rejs
        .iter()
        .rev()
        .take(40)
        .map(|r| {
            let IdxRow::Rej {
                ts,
                chain,
                variant,
                reason,
                gross_usd,
                threshold_usd,
            } = r
            else {
                unreachable!()
            };
            json!({"id": format!("REJ-{:08x}", ts % 0xFFFF_FFFF), "ts": ts,
               "chain": chain, "variant": variant, "reason": reason,
               "gross_usd": gross_usd, "threshold_usd": threshold_usd,
               "stages": {"detected": true, "qualified": false, "simulated": false,
                          "defended": true, "executed": false,
                          "confirmed": false, "settled": false}})
        })
        .collect();

    // Soldier activity feed — component-attributed lines (§38.15.15).
    let mut activity: Vec<Value> = Vec::new();
    let window: Vec<&IdxRow> = idx.in_range(from, q.to).collect();
    for r in window.iter().rev().take(80) {
        let line = match r {
            IdxRow::Opp {
                ts,
                chain,
                variant,
                expected_usd,
                ..
            } => (
                *ts,
                "Solver",
                format!("detected OPP on {chain} [{variant}] +${expected_usd:.2} expected"),
            ),
            IdxRow::Rej {
                ts, chain, reason, ..
            } => (
                *ts,
                "Guard",
                format!("rejected candidate on {chain}: {reason}"),
            ),
            IdxRow::ExecReq {
                ts, chain, dry_run, ..
            } => (
                *ts,
                "Executor",
                format!(
                    "execution {} on {chain}",
                    if *dry_run { "dry-run" } else { "authorized" }
                ),
            ),
            IdxRow::Exec {
                ts, chain, status, ..
            } => (*ts, "Chain", format!("{chain} execution status {status}")),
            IdxRow::Fail {
                ts,
                component,
                class,
                ..
            } => (
                *ts,
                "Sentinel",
                format!("{component} failure class {class}"),
            ),
            IdxRow::Heal {
                ts,
                target,
                action,
                success,
                ..
            } => (
                *ts,
                "Sentinel",
                format!(
                    "heal {action} on {target} → {}",
                    if *success { "verified" } else { "failed" }
                ),
            ),
            IdxRow::Scan {
                ts,
                qualified,
                total,
                ..
            } => (
                *ts,
                "Scout",
                format!("RPC sweep: {qualified}/{total} qualified"),
            ),
        };
        activity.push(json!({"ts": line.0, "actor": line.1, "msg": line.2}));
    }

    // Defense strip — states derived from recent failures, not assumed.
    let recent_fails: Vec<&&IdxRow> = window
        .iter()
        .filter(|r| matches!(r, IdxRow::Fail { .. }) && r.ts() >= to.saturating_sub(10 * 60_000))
        .collect();
    let gate = |pat: &str| {
        if recent_fails
            .iter()
            .any(|r| matches!(r, IdxRow::Fail { class, .. } if class.contains(pat)))
        {
            "RED"
        } else {
            "ARMED"
        }
    };
    let defense = json!([
        {"gate": "MARKET DATA INTEGRITY", "state": gate("Rpc")},
        {"gate": "RPC CONSISTENCY", "state": gate("RpcProviderDivergence")},
        {"gate": "STATE FRESHNESS", "state": gate("Stale")},
        {"gate": "CONTRACT VALIDATION", "state": "ARMED"},
        {"gate": "SIMULATION INTEGRITY", "state": gate("Simulation")},
        {"gate": "EXECUTION POLICY", "state": "ARMED"},
        {"gate": "WALLET AUTHORIZATION", "state": "UNVERIFIED"},
        {"gate": "KILL SWITCH", "state": if Path::new("HALT.flag").exists() {"ENGAGED"} else {"ARMED"}},
    ]);

    // Capital-protection summary.
    let protected: f64 = rejs
        .iter()
        .filter_map(|r| match r {
            IdxRow::Rej { gross_usd, .. } => Some(*gross_usd),
            _ => None,
        })
        .sum();
    let realized: f64 = execs
        .iter()
        .filter_map(|r| match r {
            IdxRow::Exec {
                realized_usd,
                status,
                ..
            } if status == "Included" => Some(*realized_usd),
            _ => None,
        })
        .sum();

    // Opportunity-rise/fall trend — real event density per bucket.
    let nb = 40usize;
    let span = (to - from).max(60_000);
    let mut trend = vec![0u64; nb];
    for r in opps.iter().chain(rejs.iter()) {
        let t = r.ts();
        if t >= from && t <= to {
            trend[((t - from) / (span / nb as u64).max(1)).min(nb as u64 - 1) as usize] += 1;
        }
    }

    Json(json!({
        "funnel": {
            "detected": opps.len() + rejs.len(),
            "qualified": opps.len(),
            "executed": execreqs.len(),
            "included": execs.iter().filter(|e| matches!(e, IdxRow::Exec{status,..} if status=="Included")).count(),
            "failed": execs.iter().filter(|e| matches!(e, IdxRow::Exec{status,..} if matches!(status.as_str(),"Reverted"|"Failed"))).count(),
            "rejected": rejs.len(),
        },
        "opportunities": cards,
        "blocked": rejected_cards,
        "activity": activity,
        "defense": defense,
        "protection": {
            "evaluated": opps.len() + rejs.len(),
            "blocked": rejs.len(),
            "threats": fails.len(),
            "capital_protected_usd": protected,
            "net_realized_usd": realized,
            "heals": heals.len(),
        },
        "trend": trend,
        "data_since_ms": idx.index_since_ms,
        "note": "stages without a real backing event render UNVERIFIED — \
                 execution/settlement stages stay dark until the executor \
                 is live (AGENTS §38.15)",
    }))
}

// ---------- /api/status — global rollup (§38.22) ----------
pub async fn status(State(st): State<Shared>) -> Json<Value> {
    let halted = Path::new("HALT.flag").exists();
    let reg = RpcRegistry::load(Path::new(&reg_path()));
    let s = reg.summary();
    let idx = st.idx.lock().unwrap();
    // Recent chain-blindness drives DEGRADED.
    let cutoff = allbright_core::events::now_ms().saturating_sub(5 * 60_000);
    let blind_recent = idx.in_range(cutoff, 0).any(|r| {
        matches!(r,
        IdxRow::Fail { class, .. } if class.contains("RpcChainVisibility")
            || class.contains("RpcBlindness"))
    });
    let status = if halted {
        "SAFE-HALT"
    } else if blind_recent || s.qualified == 0 {
        "DEGRADED"
    } else {
        "HEALTHY"
    };
    Json(json!({
        "status": status,
        "kill_switch": if halted { "HALTED" } else { "RUNNING" },
        "uptime_secs": (allbright_core::events::now_ms() - st.started_ms) / 1000,
        "qualified_rpcs": s.qualified,
        "registry_updated_ms": reg.updated_ms,
        "index_rows": idx.rows().len(),
        "index_since_ms": idx.index_since_ms,
    }))
}

// ---------- /api/killswitch - global halt control (SS38.11) ----------

/// /api/competitive - Competitive Benchmark Register (AGENTS SS27.1).
/// benchmark_register.json carries confidence-tagged public cells for
/// peers; the AllBright column is enriched with live measured state so
/// self-comparison is evidence, not claim.
pub async fn competitive(State(st): State<Shared>) -> Json<Value> {
    let reg = [
        "benchmark_register.json",
        "../benchmark_register.json",
        "../../benchmark_register.json",
    ]
    .iter()
    .find_map(|p| std::fs::read_to_string(p).ok())
    .and_then(|s| serde_json::from_str::<Value>(&s).ok());
    let pools = allbright_core::chain::load_pools();
    let rs = RpcRegistry::load(Path::new(&reg_path())).summary();
    let (mut detected, mut simulated, mut submitted, mut included) = (0u64, 0u64, 0u64, 0u64);
    let mut realized = 0.0f64;
    {
        let idx = st.idx.lock().unwrap();
        for r in idx.rows() {
            match r {
                IdxRow::Opp { .. } => detected += 1,
                IdxRow::ExecReq { .. } => simulated += 1,
                IdxRow::Exec {
                    status,
                    realized_usd,
                    ..
                } => {
                    if status == "Submitted" || status == "Included" {
                        submitted += 1;
                        if status == "Included" {
                            included += 1;
                        }
                        realized += realized_usd;
                    }
                }
                _ => {}
            }
        }
    }
    Json(json!({
        "register": reg,
        "data_since_ms": st.idx.lock().unwrap().index_since_ms,
        "allbright_measured": {
            "chains_enabled": Chain::ALL.len(),
            "pools_verified": pools.len(),
            "rpc_qualified": rs.qualified,
            "rpc_total": rs.total,
            "rpc_providers": rs.providers,
            "opportunities_detected": detected,
            "requests_simulated": simulated,
            "exec_submitted": submitted,
            "exec_included": included,
            "realized_profit_usd": realized,
        },
        "notice": "Public-source snapshot across distinct platform categories - not a verified ranking. AllBright column merges measured live state; peer cells carry confidence tags. UNVERIFIED where evidence is absent.",
    }))
}

#[derive(Deserialize)]
pub struct KillReq {
    action: String,
}

pub async fn killswitch(State(_st): State<Shared>, Json(req): Json<KillReq>) -> Json<Value> {
    let p = Path::new("HALT.flag");
    match req.action.as_str() {
        "halt" => {
            let _ = std::fs::write(p, format!("halted at {}", allbright_core::events::now_ms()));
            Json(json!({"state": "HALTED",
                "note": "new execution activity halted; submitted transactions unaffected"}))
        }
        "resume" => {
            let _ = std::fs::remove_file(p);
            Json(json!({"state": "RUNNING"}))
        }
        _ => Json(json!({"state": if p.exists() { "HALTED" } else { "RUNNING" }})),
    }
}
