//! Fork/replay harness — §28 blindness test for GAP-1/GAP-6.
//!
//! Scans the append-only BSONL journal (`journal/*.bsonl`), reconstructs
//! the `execute()` calldata behind every FAILED/REVERTED ExecResult, and
//! replays each case through `simgate` (eth_simulateV1 → eth_call
//! fallback) against live chain state. Exit 1 if any known-reverting
//! case passes the gate (a proven blind spot). Cases replay on CURRENT
//! state — a "gate-pass" on an old revert may be honest state drift,
//! not gate blindness; the report says which.
//!
//! Journal truth: ExecResult rows carry `detail` strings (e.g.
//! "evm dry-run gate: execution reverted: CamelotPair: K"). The encoded
//! calldata is NOT journaled (`OpportunitySignal.calldata` is always
//! `None`, solver main.rs:587), so it is reconstructed exactly as the
//! executor did: `calldata::encode_execute_full` over each verified
//! static flash provider (mirror of relay.rs:44-96, cheapest-first) and
//! the daemon-side `encode_execute` variant. `from` is recovered
//! on-chain via `AllbrightExecutor.owner()` (the Kernel smart account
//! is the onlyOwner gate) — never guessed.
//!
//! Matching: the wire has no request-id; each ExecResult is paired with
//! the earliest unmatched prior ExecRequest on the same chain (FIFO).
//!
//! Anvil fork mode (alloy::node_bindings::Anvil) is NOT built:
//! `alloy-node-bindings` is not a workspace dep and needs a local anvil
//! binary. Live-RPC replay is the shipped mode.
//!
//! Usage (run from repo root):
//!   set REPLAY_RPC=https://bsc-rpc.publicnode.com   :: all chains
//!   set REPLAY_RPC_BSC=https://...                  :: per-chain override
//!   cargo run -p allbright-executor --example simgate_replay -- [journal_dir]
//! Env: REPLAY_RPC, REPLAY_RPC_<CHAIN>, REPLAY_JOURNAL, REPLAY_FROM.

use alloy::primitives::{hex, keccak256, Address, Bytes, U256};
use allbright_core::{bus, config::Config, events::*};
use allbright_executor::{
    calldata,
    simgate::{SimGate, SimOutcome},
};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::io::Read;

// Mirror of relay.rs FlashKind + static provider table (relay.rs:44-96).
// Candidates only — each is re-verified on-chain before use, exactly as
// submit_via_aa does. Keep in sync with relay.rs.
const KIND_AAVE_V3: u64 = 1;
const KIND_BALANCER_V2: u64 = 3;
const KIND_MORPHO: u64 = 4;

fn provider_candidates(chain: Chain) -> &'static [(&'static str, u64, &'static str)] {
    const BAL: &str = "0xBA12222222228d8Ba445958a75a0704d566BF2C8";
    const MORPHO: &str = "0xBBBBBbbBBb9cC5e90e3b3Af64BDaf62C37EEFFCb";
    match chain {
        Chain::Eth => &[
            (BAL, KIND_BALANCER_V2, "balancer_v2"),
            (MORPHO, KIND_MORPHO, "morpho"),
            ("0x87870Bca3F3fD6335C3F4ce8392D69350B4fA2", KIND_AAVE_V3, "aave_v3"),
        ],
        Chain::Bsc => &[
            (BAL, KIND_BALANCER_V2, "balancer_v2"),
            (MORPHO, KIND_MORPHO, "morpho"),
            ("0x6807dc923806fE8Fd134338EABCA509979a7e0cB", KIND_AAVE_V3, "aave_v3"),
        ],
        Chain::Base => &[
            (BAL, KIND_BALANCER_V2, "balancer_v2"),
            (MORPHO, KIND_MORPHO, "morpho"),
            ("0xA238Dd80C259a72e81d7e4664a9801593F98d1c5", KIND_AAVE_V3, "aave_v3"),
        ],
        Chain::Arb => &[
            (BAL, KIND_BALANCER_V2, "balancer_v2"),
            ("0x794a61358D6845594F94dc1DB02A252b5b4814aD", KIND_AAVE_V3, "aave_v3"),
        ],
        Chain::Op => &[
            (BAL, KIND_BALANCER_V2, "balancer_v2"),
            ("0x794a61358D6845594F94dc1DB02A252b5b4814aD", KIND_AAVE_V3, "aave_v3"),
        ],
        Chain::Poly => &[
            (BAL, KIND_BALANCER_V2, "balancer_v2"),
            ("0x794a61358D6845594F94dc1DB02A252b5b4814aD", KIND_AAVE_V3, "aave_v3"),
        ],
        Chain::Avax => &[
            (BAL, KIND_BALANCER_V2, "balancer_v2"),
            ("0x794a61358D6845594F94dc1DB02A252b5b4814aD", KIND_AAVE_V3, "aave_v3"),
        ],
        Chain::Linea => &[
            (BAL, KIND_BALANCER_V2, "balancer_v2"),
            ("0xc47b8C00b0f69a36fa203Ffeac0334874574a8Ac", KIND_AAVE_V3, "aave_v3"),
        ],
        Chain::Scroll => &[
            (BAL, KIND_BALANCER_V2, "balancer_v2"),
            ("0x11fCfe756c05AD438e312a7fd934381537D3cFfe", KIND_AAVE_V3, "aave_v3"),
        ],
        Chain::Blast => &[],
    }
}

async fn view_call(client: &reqwest::Client, rpc: &str, to: Address, data: &[u8]) -> Option<Bytes> {
    let v: Value = client
        .post(rpc)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"eth_call",
            "params":[{"to":format!("{to:?}"),"data":format!("0x{}",hex::encode(data))},"latest"]}))
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;
    let s = v["result"].as_str()?;
    hex::decode(s.trim_start_matches("0x")).ok().map(Bytes::from)
}

async fn verify_provider(client: &reqwest::Client, rpc: &str, addr: Address, kind: u64) -> bool {
    let probe: &[u8] = match kind {
        KIND_AAVE_V3 => &keccak256(b"getReservesList()")[..4],
        KIND_BALANCER_V2 => &keccak256(b"getProtocolFeesCollector()")[..4],
        KIND_MORPHO => &keccak256(b"owner()")[..4],
        _ => return false,
    };
    match view_call(client, rpc, addr, probe).await {
        Some(out) => out.len() >= 32 && out.iter().any(|b| *b != 0),
        None => false,
    }
}

/// `AllbrightExecutor.owner()` — the Kernel smart account; the `from`
/// every real submission uses (contract onlyOwner gate).
async fn owner_of(client: &reqwest::Client, rpc: &str, executor: Address) -> Option<Address> {
    let out = view_call(client, rpc, executor, &keccak256(b"owner()")[..4]).await?;
    if out.len() < 32 {
        return None;
    }
    Some(Address::from_slice(&out[12..32]))
}

async fn erc20_decimals(client: &reqwest::Client, rpc: &str, token: Address) -> Option<u8> {
    let out = view_call(client, rpc, token, &keccak256(b"decimals()")[..4]).await?;
    out.last().copied().filter(|d| *d > 0 && *d <= 36)
}

fn rpc_for(chain: Chain, cfg_urls: &HashMap<Chain, String>) -> Option<String> {
    std::env::var(format!("REPLAY_RPC_{}", chain.name().to_uppercase()))
        .ok()
        .or_else(|| std::env::var("REPLAY_RPC").ok())
        .or_else(|| cfg_urls.get(&chain).cloned())
}

/// Reconstruct every calldata variant the executor may have run through
/// the gate, in submission order (first verified provider wins in
/// relay.rs — index 0 of the "full:" variants is the primary replay).
async fn reconstruct(
    client: &reqwest::Client,
    rpc: &str,
    req: &ExecutionRequest,
) -> Vec<(String, Bytes)> {
    let mut out: Vec<(String, Bytes)> = Vec::new();
    // 1) Journaled calldata verbatim (currently never populated).
    if let Some(cd) = &req.signal.calldata {
        if let Ok(b) = hex::decode(cd.trim_start_matches("0x")) {
            out.push(("journaled".into(), Bytes::from(b)));
        }
    }
    // 2) The full gate-path encoding per verified provider.
    if let Some(tok) = req
        .signal
        .route
        .first()
        .and_then(|l| l.token_in.parse::<Address>().ok())
    {
        let decimals = erc20_decimals(client, rpc, tok).await.unwrap_or(18);
        let unit = 10f64.powi(decimals as i32);
        let amount_in = U256::from((req.signal.input_amount * unit).max(0.0) as u128);
        let min_profit = U256::from((req.signal.min_profit_usd * unit).max(0.0) as u128);
        for (addr_s, kind, name) in provider_candidates(req.signal.chain) {
            let Ok(addr) = addr_s.parse::<Address>() else { continue };
            if !verify_provider(client, rpc, addr, *kind).await {
                continue;
            }
            if let Ok(cd) = calldata::encode_execute_full(
                &req.signal,
                amount_in,
                min_profit,
                addr,
                Bytes::from(U256::from(*kind).to_be_bytes::<32>().to_vec()),
            ) {
                out.push((format!("full:{name}"), Bytes::from(cd)));
            }
        }
    }
    // 3) The daemon's dry-run validation encoding (zero flash provider).
    out.push((
        "daemon:encode_execute".into(),
        Bytes::from(calldata::encode_execute(&req.signal, &req.executor_address)),
    ));
    out
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let dir = std::env::args()
        .nth(1)
        .or_else(|| std::env::var("REPLAY_JOURNAL").ok())
        .unwrap_or_else(|| "journal".into());
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("bsonl"))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    if files.is_empty() {
        println!("{{\"warning\":\"no .bsonl files under {dir} — nothing to replay\"}}");
        return Ok(());
    }

    // Pair ExecResults with the earliest unmatched prior ExecRequest on
    // the same chain (FIFO — no request-id on the wire).
    let mut pending: HashMap<Chain, VecDeque<ExecutionRequest>> = HashMap::new();
    let mut cases: Vec<(ExecutionRequest, ExecutionResult)> = Vec::new();
    for path in &files {
        let Ok(mut f) = std::fs::File::open(path) else { continue };
        loop {
            let mut lb = [0u8; 4];
            if f.read_exact(&mut lb).is_err() {
                break;
            }
            let len = u32::from_le_bytes(lb) as usize;
            if len > 64 * 1024 * 1024 {
                break; // corrupt frame — stop scanning this file
            }
            let mut payload = vec![0u8; len];
            if f.read_exact(&mut payload).is_err() {
                break;
            }
            match bus::decode(&payload) {
                Ok(Event::ExecRequest(r)) => {
                    pending.entry(r.signal.chain).or_default().push_back(r);
                }
                Ok(Event::ExecResult(res))
                    if matches!(res.status, ExecStatus::Failed | ExecStatus::Reverted) =>
                {
                    if let Some(req) = pending.get_mut(&res.chain).and_then(|q| q.pop_front()) {
                        cases.push((req, res));
                    }
                }
                _ => {}
            }
        }
    }
    println!(
        "{{\"journal_files\":{},\"failure_cases\":{}}}",
        files.len(),
        cases.len()
    );

    let cfg_urls = Config::load().map(|c| c.rpc_urls).unwrap_or_default();
    let gate = SimGate::new();
    let client = reqwest::Client::new();
    let mut predicted = 0u64;
    let mut blind = 0u64;
    let mut inconclusive = 0u64;
    let mut skipped = 0u64;

    for (req, res) in &cases {
        let chain = req.signal.chain;
        let Some(rpc) = rpc_for(chain, &cfg_urls) else {
            skipped += 1;
            println!(
                "{}",
                json!({"chain":chain.name(),"verdict":"SKIP","reason":"no rpc (REPLAY_RPC/REPLAY_RPC_<CHAIN>)"})
            );
            continue;
        };
        let executor: Address = match req
            .executor_address
            .parse()
            .ok()
            .or_else(|| {
                std::env::var(format!(
                    "ALLBRIGHT_EXECUTOR_{}",
                    chain.name().to_uppercase()
                ))
                .ok()
                .and_then(|s| s.parse().ok())
            }) {
            Some(a) => a,
            None => {
                skipped += 1;
                println!("{}", json!({"chain":chain.name(),"verdict":"SKIP","reason":"no executor address"}));
                continue;
            }
        };
        let from = match owner_of(&client, &rpc, executor).await {
            Some(o) => o,
            None => match std::env::var("REPLAY_FROM").ok().and_then(|s| s.parse().ok()) {
                Some(f) => f,
                None => {
                    skipped += 1;
                    println!("{}", json!({"chain":chain.name(),"verdict":"SKIP","reason":"owner() unreadable; set REPLAY_FROM"}));
                    continue;
                }
            },
        };
        let variants = reconstruct(&client, &rpc, req).await;
        let mut rows = Vec::new();
        for (label, cd) in &variants {
            let outcome = gate.simulate(&rpc, from, executor, cd.clone()).await?;
            let (cls, det) = match &outcome {
                SimOutcome::EvmOk { .. } => ("EvmOk", String::new()),
                SimOutcome::EvmRevert { reason, .. } => ("EvmRevert", reason.clone()),
                SimOutcome::Transport(e) => ("Transport", e.to_string()),
            };
            rows.push(json!({"variant": label, "outcome": cls, "detail": det}));
        }
        // Primary = first "full:" variant (first verified provider — what
        // relay picked); fallback = daemon encoding when none verified.
        let primary = rows
            .iter()
            .find(|r| r["variant"].as_str().unwrap_or("").starts_with("full:"))
            .or_else(|| rows.iter().find(|r| r["variant"] == "daemon:encode_execute"));
        let primary_outcome = primary
            .and_then(|r| r["outcome"].as_str())
            .unwrap_or("Transport");
        let expected_revert = res.detail.to_lowercase().contains("revert")
            || res.detail.contains("Pair: K")
            || res.detail.contains("dry-run gate");
        let verdict = match (expected_revert, primary_outcome) {
            (true, "EvmRevert") => {
                predicted += 1;
                "PREDICTED-REVERT"
            }
            (true, "EvmOk") => {
                blind += 1;
                "BLIND-PASS (check state drift)"
            }
            (true, _) => {
                inconclusive += 1;
                "INCONCLUSIVE"
            }
            (false, "EvmRevert") => {
                predicted += 1;
                "REVERTS-TODAY"
            }
            (false, "EvmOk") => {
                inconclusive += 1;
                "PASSES (was pre-gate rejection)"
            }
            (false, _) => {
                inconclusive += 1;
                "INCONCLUSIVE"
            }
        };
        println!(
            "{}",
            json!({
                "chain": chain.name(), "strategy": res.strategy,
                "status": format!("{:?}", res.status),
                "expected": &res.detail[..res.detail.len().min(160)],
                "sim_method": gate.capability(&rpc).await.name(),
                "variants": rows, "verdict": verdict,
            })
        );
    }

    println!(
        "{}",
        json!({
            "summary": {"cases": cases.len(), "predicted": predicted,
                "blind": blind, "inconclusive": inconclusive, "skipped": skipped}
        })
    );
    if blind > 0 {
        eprintln!("FAIL-LOUD: {blind} known-reverting case(s) PASSED the sim gate");
        std::process::exit(1);
    }
    Ok(())
}
