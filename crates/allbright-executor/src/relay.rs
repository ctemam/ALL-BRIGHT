//! BuilderRelay abstraction — per-chain private-orderflow adapters.
//! ToR rule: Ethereum Flashbots semantics are NEVER assumed portable; each
//! chain's relay/builder path is verified independently.

use alloy::primitives::{keccak256, Address, Bytes, U256};
use alloy::signers::local::PrivateKeySigner;
use anyhow::Result;
use secrecy::ExposeSecret;
use serde_json::json;

use crate::aa;
use crate::calldata;
use allbright_core::{config::Config, events::*};

/// Submit the encoded executor call.
/// Chains with a deployed executor + sponsorship → gasless UserOp through
/// the Kernel smart account (the only caller `AllbrightExecutor` accepts).
/// Ethereum without sponsorship → Flashbots `eth_sendBundle` (EOA gas).
/// Anything else → refuse rather than leak orderflow.
pub async fn submit(cfg: &Config, req: &ExecutionRequest, _calldata: &[u8]) -> Result<String> {
    match req.signal.chain {
        // Any chain with a deployed executor + Pimlico sponsorship →
        // gasless sponsored UserOp (Eth included — the EOA-only Flashbots
        // path can't pay gas on a zero-balance wallet).
        c if cfg.executor_address(c).is_some()
            && cfg.pimlico_api_key.is_some()
            && cfg.signing_key().is_some() =>
        {
            submit_via_aa(cfg, req).await
        }
        Chain::Eth => submit_flashbots(cfg, req).await,
        other => anyhow::bail!("no verified relay adapter for {}", other.name()),
    }
}

/// FlashKind mirror of AllbrightExecutor.sol's enum — keep in sync.
const KIND_AAVE_V3: u64 = 1;
const KIND_BALANCER_V2: u64 = 3;
const KIND_MORPHO: u64 = 4;
const KIND_UNIV3_FLASH: u64 = 5;
const KIND_ERC3156: u64 = 6;

/// Flash-loan provider candidates per chain, ordered cheapest-first
/// (Balancer V2 and Morpho charge 0% premium; Aave charges 0.05%).
/// Membership here is a candidate, not a trust — every address is
/// verified on-chain before each submission.
fn flash_providers(chain: Chain) -> &'static [(&'static str, u64)] {
    // Balancer V2 Vault — identical address on deployed chains. Verified
    // via getProtocolFeesCollector() on 9 chains on 2026-10-08; the Blast
    // address at this slot is ~1.4KB and fails the Vault interface — NOT
    // a real Balancer deployment, so Blast stays fail-closed below.
    const BAL: &str = "0xBA12222222228d8Ba445958a75a0704d566BF2C8";
    const MORPHO: &str = "0xBBBBBbbBBb9cC5e90e3b3Af64BDaf62C37EEFFCb";
    match chain {
        Chain::Eth => &[
            (BAL, KIND_BALANCER_V2),
            (MORPHO, KIND_MORPHO),
            ("0x87870Bca3F3fD6335C3F4ce8392D69350B4fA2", KIND_AAVE_V3),
        ],
        Chain::Bsc => &[
            (BAL, KIND_BALANCER_V2),
            (MORPHO, KIND_MORPHO),
            ("0x6807dc923806fE8Fd134338EABCA509979a7e0cB", KIND_AAVE_V3),
        ],
        Chain::Base => &[
            (BAL, KIND_BALANCER_V2),
            (MORPHO, KIND_MORPHO),
            ("0xA238Dd80C259a72e81d7e4664a9801593F98d1c5", KIND_AAVE_V3),
        ],
        Chain::Arb => &[
            (BAL, KIND_BALANCER_V2),
            ("0x794a61358D6845594F94dc1DB02A252b5b4814aD", KIND_AAVE_V3),
        ],
        Chain::Op => &[
            (BAL, KIND_BALANCER_V2),
            ("0x794a61358D6845594F94dc1DB02A252b5b4814aD", KIND_AAVE_V3),
        ],
        Chain::Poly => &[
            (BAL, KIND_BALANCER_V2),
            ("0x794a61358D6845594F94dc1DB02A252b5b4814aD", KIND_AAVE_V3),
        ],
        Chain::Avax => &[
            (BAL, KIND_BALANCER_V2),
            ("0x794a61358D6845594F94dc1DB02A252b5b4814aD", KIND_AAVE_V3),
        ],
        Chain::Linea => &[
            (BAL, KIND_BALANCER_V2),
            ("0xc47b8C00b0f69a36fa203Ffeac0334874574a8Ac", KIND_AAVE_V3),
        ],
        Chain::Scroll => &[
            (BAL, KIND_BALANCER_V2),
            ("0x11fCfe756c05AD438e312a7fd934381537D3cFfe", KIND_AAVE_V3),
        ],
        // Blast: Aave V3 absent, the 0xBA12 slot is not a real Vault
        // (~1.4KB, interface probes fail), DODO probe failed earlier.
        // No verified flash provider → fail-closed.
        Chain::Blast => &[],
    }
}

/// Canonical stablecoin membership — the list lives in
/// `allbright_core::chain` so solver and executor apply the identical
/// test (executor: USD→token minProfit conversion; solver: emit gate).
pub fn stable_addr(chain: Chain, token: Address) -> bool {
    allbright_core::chain::stable_addr_str(chain, &format!("{token:?}"))
}

// ── Hot-path transport (GAP-10/§36.9) ────────────────────────────────
// Shared client; each call races the top RACE_WIDTH pool endpoints
// concurrently — first definitive response wins, transport/provider
// failures fall through to the next completion. An `Evm` outcome (the
// node executed the call and it reverted) is authoritative chain truth
// — it wins the race immediately and is never retried on another
// endpoint (a second node would only echo the same revert).
const RACE_WIDTH: usize = 3;

enum CallOutcome {
    Ok(serde_json::Value),
    Evm(String),
    Transport(String),
}

fn shared_client() -> &'static reqwest::Client {
    static C: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    C.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap_or_default()
    })
}

async fn rpc_one(
    client: &reqwest::Client,
    url: &str,
    method: &str,
    params: serde_json::Value,
) -> CallOutcome {
    let send = client
        .post(url)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
        .send()
        .await;
    let Ok(resp) = send else {
        return CallOutcome::Transport(send.unwrap_err().to_string());
    };
    let v = match resp.json::<serde_json::Value>().await {
        Ok(v) => v,
        Err(e) => return CallOutcome::Transport(e.to_string()),
    };
    if let Some(err) = v.get("error") {
        let msg = err["message"].as_str().unwrap_or("evm error");
        let data = err["data"].as_str().unwrap_or_default();
        return CallOutcome::Evm(format!("{msg} {data}"));
    }
    match v.get("result") {
        Some(r) => CallOutcome::Ok(r.clone()),
        None => CallOutcome::Transport("empty response".into()),
    }
}

/// Race one JSON-RPC call across the top `RACE_WIDTH` endpoints.
/// Transport outcomes accumulate; the first Ok/Evm completes the call.
async fn race_rpc(rpcs: &[String], method: &str, params: serde_json::Value) -> CallOutcome {
    use futures_util::stream::{FuturesUnordered, StreamExt};
    let client = shared_client();
    let mut futs = FuturesUnordered::new();
    for url in rpcs.iter().take(RACE_WIDTH) {
        futs.push(rpc_one(client, url, method, params.clone()));
    }
    let mut last = CallOutcome::Transport("empty endpoint pool".into());
    while let Some(out) = futs.next().await {
        match out {
            CallOutcome::Transport(_) => last = out,
            definitive => return definitive,
        }
    }
    last
}

fn result_bytes(v: &serde_json::Value) -> Bytes {
    let s = v.as_str().unwrap_or("0x");
    Bytes::from(
        alloy::primitives::hex::decode(s.trim_start_matches("0x")).unwrap_or_default(),
    )
}

async fn eth_call(rpcs: &[String], to: Address, data: Bytes) -> Result<Bytes> {
    let params = json!([{"to":format!("{to:?}"),
        "data":format!("0x{}",alloy::primitives::hex::encode(&data))},"latest"]);
    match race_rpc(rpcs, "eth_call", params).await {
        CallOutcome::Ok(v) => Ok(result_bytes(&v)),
        CallOutcome::Evm(m) => anyhow::bail!("evm: {m}"),
        CallOutcome::Transport(m) => anyhow::bail!("transport: {m}"),
    }
}

/// `eth_call` with a spoofed `from` — used by the EVM dry-run gate
/// (GAP-1). Two failure classes are distinguished: `Evm` = the node
/// executed the call and it failed/reverted (the tx would fail
/// identically on-chain — always gates); `Transport` = HTTP/JSON
/// failure (node unreachable — retries, then inconclusive).
enum DryRunErr {
    Evm(String),
    Transport(anyhow::Error),
}

async fn eth_call_from(rpcs: &[String], from: Address, to: Address, data: Bytes) -> std::result::Result<Bytes, DryRunErr> {
    // U11: when REVM_DRYRUN is set, run the identical call in the local
    // EVM first — zero remote eth_call quota. A `Db` miss falls through
    // to the remote call (fail-closed, never guessed state).
    if crate::evm_dry::enabled() {
        if let Some(url) = rpcs.first() {
            match crate::evm_dry::dry_run(url.clone(), from, to, data.clone()).await {
                crate::evm_dry::DryVerdict::Ok(out) => return Ok(out),
                crate::evm_dry::DryVerdict::Revert(d) => {
                    return Err(DryRunErr::Evm(format!("revm: {d}")));
                }
                crate::evm_dry::DryVerdict::Db(e) => {
                    tracing::debug!(err = %e, "revm dry-run db miss — remote eth_call");
                }
            }
        }
    }
    let params = json!([{"from":format!("{from:?}"),"to":format!("{to:?}"),
        "data":format!("0x{}",alloy::primitives::hex::encode(&data))},"latest"]);
    match race_rpc(rpcs, "eth_call", params).await {
        CallOutcome::Ok(v) => Ok(result_bytes(&v)),
        CallOutcome::Evm(m) => Err(DryRunErr::Evm(m)),
        CallOutcome::Transport(m) => Err(DryRunErr::Transport(anyhow::anyhow!(m))),
    }
}

async fn eth_get_code(rpcs: &[String], addr: Address) -> Result<Bytes> {
    match race_rpc(rpcs, "eth_getCode", json!([format!("{addr:?}"), "latest"])).await {
        CallOutcome::Ok(v) => Ok(result_bytes(&v)),
        CallOutcome::Evm(m) => anyhow::bail!("evm: {m}"),
        CallOutcome::Transport(m) => anyhow::bail!("transport: {m}"),
    }
}

/// One Multicall3 `aggregate3` over independent view calls — N serial
/// RTTs collapse into a single raced request (canonical deployment on
/// all 10 chains). Per-call `allowFailure`: a reverting probe yields
/// `None`, never poisons the batch. Batch-level transport failure also
/// yields all-`None` (callers fail closed, same as a dropped probe).
async fn batch_calls(rpcs: &[String], calls: &[(Address, Bytes)]) -> Vec<Option<Bytes>> {
    use alloy::sol_types::SolCall;
    use allbright_core::multicall::{multicall3_addr, IMulticall3};
    let mc_calls: Vec<IMulticall3::Call3> = calls
        .iter()
        .map(|(target, data)| IMulticall3::Call3 {
            target: *target,
            allowFailure: true,
            callData: data.clone(),
        })
        .collect();
    let encoded = IMulticall3::aggregate3Call { calls: mc_calls }.abi_encode();
    let Ok(out) = eth_call(rpcs, multicall3_addr(), Bytes::from(encoded)).await else {
        return calls.iter().map(|_| None).collect();
    };
    match IMulticall3::aggregate3Call::abi_decode_returns(&out) {
        Ok(results) => results
            .into_iter()
            .map(|r| r.success.then_some(r.returnData))
            .collect(),
        Err(_) => calls.iter().map(|_| None).collect(),
    }
}

/// On-chain verification for a flash provider candidate — calldata
/// only; the probes run inside a single `batch_calls` aggregate3:
/// Aave V3 → `getReservesList()` returns a non-empty array.
/// Balancer V2 → `getProtocolFeesCollector()` returns an address (only the
/// real Vault exposes it).
/// Morpho → `owner()` returns an address (Morpho Blue is owned).
fn provider_probe(kind: u64) -> Option<Bytes> {
    let sel: &[u8] = match kind {
        KIND_AAVE_V3 => &keccak256(b"getReservesList()")[..4],
        KIND_BALANCER_V2 => &keccak256(b"getProtocolFeesCollector()")[..4],
        KIND_MORPHO => &keccak256(b"owner()")[..4],
        _ => return None,
    };
    Some(Bytes::from(sel.to_vec()))
}

/// `true` when an aggregate3 result carries a non-empty payload —
/// the shared verdict for every provider probe.
fn probe_verified(r: Option<&Bytes>) -> bool {
    r.map(|b| b.len() >= 32 && b.iter().any(|x| *x != 0))
        .unwrap_or(false)
}

fn addr_from_ret(b: &Bytes) -> Option<Address> {
    if b.len() < 32 {
        return None;
    }
    Some(Address::from_slice(&b[12..32]))
}

fn u256_from_ret(b: &Bytes) -> Option<U256> {
    if b.len() < 32 {
        return None;
    }
    Some(U256::from_be_slice(&b[..32]))
}

/// GAP-22 — live on-chain USD quote for a non-stable token. Route legs
/// are tried first; when none pair `token` with a stable, the verified
/// pool REGISTRY is scanned for `token↔stable` candidates (Phase C).
/// Price from `getReserves` (V2/Stable) or `slot0` (V3); `None` → the
/// caller refuses (no guessed prices).
async fn quote_token_usd(
    rpcs: &[String],
    chain: Chain,
    route: &[allbright_core::events::SwapLeg],
    token: Address,
) -> Option<f64> {
    let sel_t0 = Bytes::from(keccak256(b"token0()")[..4].to_vec());
    let sel_t1 = Bytes::from(keccak256(b"token1()")[..4].to_vec());
    let sel_res = Bytes::from(keccak256(b"getReserves()")[..4].to_vec());
    let sel_slot0 = Bytes::from(keccak256(b"slot0()")[..4].to_vec());
    let sel_dec = Bytes::from(keccak256(b"decimals()")[..4].to_vec());

    // Phase A — token0/token1 for every route leg in one aggregate3.
    let mut calls: Vec<(Address, Bytes)> = Vec::new();
    let mut leg_ix: Vec<usize> = Vec::new();
    for (i, leg) in route.iter().enumerate() {
        let Ok(pool) = leg.pool_address.parse::<Address>() else { continue };
        calls.push((pool, sel_t0.clone()));
        calls.push((pool, sel_t1.clone()));
        leg_ix.push(i);
    }
    let res_a = batch_calls(rpcs, &calls).await;

    // Phase B — decimals(token), decimals(stable), price read for each
    // leg whose pair puts `token` against a verified stable — one more
    // aggregate3 instead of three serial calls per leg.
    let mut calls_b: Vec<(Address, Bytes)> = Vec::new();
    let mut pending: Vec<(bool, DexKind)> = Vec::new(); // (is0, dex)
    for (k, i) in leg_ix.iter().enumerate() {
        let leg = &route[*i];
        let Ok(pool) = leg.pool_address.parse::<Address>() else { continue };
        let t0 = res_a.get(2 * k).and_then(|r| r.as_ref()).and_then(addr_from_ret);
        let t1 = res_a.get(2 * k + 1).and_then(|r| r.as_ref()).and_then(addr_from_ret);
        let (Some(t0), Some(t1)) = (t0, t1) else { continue };
        // `is0` = token is the pool's token0; the OTHER leg must be a
        // verified stable for the quote to be USD-denominated.
        let (stable, is0) = if t0 == token && stable_addr(chain, t1) {
            (t1, true)
        } else if t1 == token && stable_addr(chain, t0) {
            (t0, false)
        } else {
            continue;
        };
        calls_b.push((token, sel_dec.clone()));
        calls_b.push((stable, sel_dec.clone()));
        calls_b.push((
            pool,
            if leg.dex == DexKind::V3 { sel_slot0.clone() } else { sel_res.clone() },
        ));
        pending.push((is0, leg.dex));
    }
    let res_b = batch_calls(rpcs, &calls_b).await;

    for (k, (is0, dex)) in pending.iter().enumerate() {
        let d_tok = res_b.get(3 * k).and_then(|r| r.as_ref()).and_then(|b| b.last().copied())
            .filter(|d| *d > 0 && *d <= 36);
        let d_st = res_b.get(3 * k + 1).and_then(|r| r.as_ref()).and_then(|b| b.last().copied())
            .filter(|d| *d > 0 && *d <= 36);
        let data = res_b.get(3 * k + 2).and_then(|r| r.as_ref());
        let (Some(d_tok), Some(d_st), Some(data)) = (d_tok, d_st, data) else { continue };
        let price = match dex {
            DexKind::V3 => {
                // slot0 word 0 = sqrtPriceX96 → raw P = (sqrt/2^96)²
                let Some(sqrt) = u256_from_ret(data) else { continue };
                let s: f64 = sqrt.to_string().parse().ok()?;
                let p_raw = (s / 18446744073709551616.0).powi(2);
                // human t0-in-t1 price; token's USD is p01 if is0 else 1/p01
                let (d0, d1) = if *is0 { (d_tok, d_st) } else { (d_st, d_tok) };
                let p01 = p_raw * 10f64.powi(d0 as i32 - d1 as i32);
                if *is0 { p01 } else if p01 > 0.0 { 1.0 / p01 } else { continue }
            }
            _ => {
                if data.len() < 64 { continue }
                let r0 = U256::from_be_slice(&data[0..32]);
                let r1 = U256::from_be_slice(&data[32..64]);
                let (r_tok, r_st) = if *is0 { (r0, r1) } else { (r1, r0) };
                let rt: f64 = r_tok.to_string().parse().ok()?;
                let rs: f64 = r_st.to_string().parse().ok()?;
                if rt <= 0.0 { continue }
                (rs / 10f64.powi(d_st as i32)) / (rt / 10f64.powi(d_tok as i32))
            }
        };
        if price.is_finite() && price > 0.0 {
            return Some(price);
        }
    }

    // Phase C — universe fallback (pricing-blindness fix): no route leg
    // priced `token`. Scan the verified registry for pools pairing
    // `token` against a verified stable and quote the CHEAPEST usable
    // price — the conservative direction: a lower USD price yields a
    // LARGER token-denominated minProfit, so a bait pool quoting an
    // inflated price can't weaken the on-chain gate. Candidates are
    // filtered by a stable-side liquidity floor ($100) so dust pools
    // can't inject junk prices either way.
    let t = format!("{token:?}").to_lowercase();
    struct Cand { pool: Address, stable: Address, is0: bool, dex: DexKind }
    let mut cands: Vec<Cand> = Vec::new();
    for p in allbright_core::chain::load_pools() {
        if p.chain != chain || cands.len() >= 12 {
            continue;
        }
        let (other, is0) = if p.token0.address.eq_ignore_ascii_case(&t) {
            (p.token1.address.as_str(), true)
        } else if p.token1.address.eq_ignore_ascii_case(&t) {
            (p.token0.address.as_str(), false)
        } else {
            continue;
        };
        let (Ok(pool), Ok(st)) = (p.address.parse::<Address>(), other.parse::<Address>())
        else {
            continue;
        };
        if !stable_addr(chain, st) {
            continue;
        }
        cands.push(Cand { pool, stable: st, is0, dex: p.dex });
    }
    if cands.is_empty() {
        return None;
    }

    let mut calls_c: Vec<(Address, Bytes)> = Vec::with_capacity(3 * cands.len() + 1);
    calls_c.push((token, sel_dec.clone())); // decimals(token) once
    for c in &cands {
        calls_c.push((c.stable, sel_dec.clone()));
        calls_c.push((
            c.pool,
            if c.dex == DexKind::V3 { sel_slot0.clone() } else { sel_res.clone() },
        ));
    }
    let res_c = batch_calls(rpcs, &calls_c).await;
    let d_tok = res_c
        .first()
        .and_then(|r| r.as_ref())
        .and_then(|b| b.last().copied())
        .filter(|d| *d > 0 && *d <= 36)?;

    let mut best: Option<f64> = None;
    for (k, c) in cands.iter().enumerate() {
        let d_st = res_c
            .get(1 + 2 * k)
            .and_then(|r| r.as_ref())
            .and_then(|b| b.last().copied())
            .filter(|d| *d > 0 && *d <= 36);
        let data = res_c.get(1 + 2 * k + 1).and_then(|r| r.as_ref());
        let (Some(d_st), Some(data)) = (d_st, data) else { continue };
        let (price, st_liq) = match c.dex {
            DexKind::V3 => {
                let Some(sqrt) = u256_from_ret(data) else { continue };
                let Ok(s) = sqrt.to_string().parse::<f64>() else { continue };
                let p_raw = (s / 18446744073709551616.0).powi(2);
                let (d0, d1) = if c.is0 { (d_tok, d_st) } else { (d_st, d_tok) };
                let p01 = p_raw * 10f64.powi(d0 as i32 - d1 as i32);
                let price = if c.is0 { p01 } else if p01 > 0.0 { 1.0 / p01 } else { continue };
                // V3 slot0 gives price but not reserves — skip the
                // liquidity floor here (in-range quote is still a live
                // market price) and accept the quote.
                (price, f64::INFINITY)
            }
            _ => {
                if data.len() < 64 { continue }
                let r0 = U256::from_be_slice(&data[0..32]);
                let r1 = U256::from_be_slice(&data[32..64]);
                let (r_tok, r_st) = if c.is0 { (r0, r1) } else { (r1, r0) };
                let (Ok(rt), Ok(rs)) = (
                    r_tok.to_string().parse::<f64>(),
                    r_st.to_string().parse::<f64>(),
                ) else { continue };
                if rt <= 0.0 { continue }
                let st_liq = rs / 10f64.powi(d_st as i32);
                ((st_liq) / (rt / 10f64.powi(d_tok as i32)), st_liq)
            }
        };
        if price.is_finite() && price > 0.0 && st_liq >= 100.0 {
            best = Some(best.map_or(price, |b: f64| b.min(price)));
        }
    }
    if best.is_some() {
        return best;
    }

    // Phase D — two-hop quote: no token↔stable pool exists anywhere.
    // Price token→wrapped-native in one registry pool against a
    // wrapped-native→stable price in another. Both legs live-read;
    // min() over candidates keeps the conservative direction and the
    // same $100 stable-side floor filters bait quotes. A corrupt or
    // thin token/native pool can't inflate the result because a wrong
    // quote only makes the candidate LESS likely to be the minimum.
    let wn = allbright_core::chain::wnative_addr(chain);
    if wn.is_empty() || wn == t.as_str() {
        return None;
    }
    let Ok(wn_addr) = wn.parse::<Address>() else {
        return None;
    };
    let mut tn_cands: Vec<Cand> = Vec::new(); // token↔wnative
    let mut ns_cands: Vec<Cand> = Vec::new(); // wnative↔stable
    // Route legs first — a leg pairing token↔wnative is a live pool the
    // solver already simmed. token0/token1 ordering comes from the
    // Phase-A multicall results (leg swap direction is not the pool
    // ordering), so `is0` stays truthful.
    for (k, i) in leg_ix.iter().enumerate() {
        if tn_cands.len() >= 4 {
            break;
        }
        let leg = &route[*i];
        let t0 = res_a.get(2 * k).and_then(|r| r.as_ref()).and_then(addr_from_ret);
        let t1 = res_a.get(2 * k + 1).and_then(|r| r.as_ref()).and_then(addr_from_ret);
        let (Some(t0), Some(t1), Ok(pool)) =
            (t0, t1, leg.pool_address.parse::<Address>())
        else {
            continue;
        };
        let (t0s, t1s) = (
            format!("{t0:?}").to_lowercase(),
            format!("{t1:?}").to_lowercase(),
        );
        if t0s == t && t1s == wn {
            tn_cands.push(Cand { pool, stable: wn_addr, is0: true, dex: leg.dex });
        } else if t1s == t && t0s == wn {
            tn_cands.push(Cand { pool, stable: wn_addr, is0: false, dex: leg.dex });
        }
    }
    for p in allbright_core::chain::load_pools() {
        if p.chain != chain {
            continue;
        }
        let (t0, t1) = (p.token0.address.to_lowercase(), p.token1.address.to_lowercase());
        let (Ok(pool), Ok(a0), Ok(a1)) = (
            p.address.parse::<Address>(),
            t0.parse::<Address>(),
            t1.parse::<Address>(),
        ) else {
            continue;
        };
        if tn_cands.len() < 12 {
            if t0 == t && t1 == wn {
                tn_cands.push(Cand { pool, stable: wn_addr, is0: true, dex: p.dex });
            } else if t1 == t && t0 == wn {
                tn_cands.push(Cand { pool, stable: wn_addr, is0: false, dex: p.dex });
            }
        }
        if ns_cands.len() < 6 {
            if t0 == wn && stable_addr(chain, a1) {
                ns_cands.push(Cand { pool, stable: a1, is0: true, dex: p.dex });
            } else if t1 == wn && stable_addr(chain, a0) {
                ns_cands.push(Cand { pool, stable: a0, is0: false, dex: p.dex });
            }
        }
    }
    if tn_cands.is_empty() || ns_cands.is_empty() {
        return None;
    }
    let mut calls_d: Vec<(Address, Bytes)> = Vec::new();
    calls_d.push((token, sel_dec.clone()));
    calls_d.push((wn_addr, sel_dec.clone()));
    for c in tn_cands.iter().chain(ns_cands.iter()) {
        calls_d.push((c.stable, sel_dec.clone()));
        calls_d.push((
            c.pool,
            if c.dex == DexKind::V3 { sel_slot0.clone() } else { sel_res.clone() },
        ));
    }
    let res_d = batch_calls(rpcs, &calls_d).await;
    let read_dec = |i: usize| {
        res_d
            .get(i)
            .and_then(|r| r.as_ref())
            .and_then(|b| b.last().copied())
            .filter(|d| *d > 0 && *d <= 36)
    };
    let (Some(d_tok), Some(d_wn)) = (read_dec(0), read_dec(1)) else {
        return None;
    };
    // shared price reader: returns (token-in-terms-of-quote-asset,
    // quote-asset liquidity) for candidate k at result offset base
    let px = |base: usize, d_base: u8, d_q: u8, is0: bool, dex: DexKind| -> Option<(f64, f64)> {
        let data = res_d.get(base + 1).and_then(|r| r.as_ref())?;
        match dex {
            DexKind::V3 => {
                let sqrt = u256_from_ret(data)?;
                let s: f64 = sqrt.to_string().parse().ok()?;
                let p_raw = (s / 18446744073709551616.0).powi(2);
                let (d0, d1) = if is0 { (d_base, d_q) } else { (d_q, d_base) };
                let p01 = p_raw * 10f64.powi(d0 as i32 - d1 as i32);
                let p = if is0 { p01 } else { 1.0 / p01 };
                if p.is_finite() && p > 0.0 { Some((p, f64::INFINITY)) } else { None }
            }
            _ => {
                if data.len() < 64 {
                    return None;
                }
                let r0 = U256::from_be_slice(&data[0..32]);
                let r1 = U256::from_be_slice(&data[32..64]);
                let (r_t, r_q) = if is0 { (r0, r1) } else { (r1, r0) };
                let rt: f64 = r_t.to_string().parse().ok()?;
                let rq: f64 = r_q.to_string().parse().ok()?;
                if rt <= 0.0 {
                    return None;
                }
                let q_liq = rq / 10f64.powi(d_q as i32);
                Some(((q_liq) / (rt / 10f64.powi(d_base as i32)), q_liq))
            }
        }
    };
    // native→USD: max quote-asset liquidity is honest (deep pool = best
    // price discovery); stable-side floor applies here too.
    let mut wn_usd: Option<f64> = None;
    for (k, c) in ns_cands.iter().enumerate() {
        let base = 2 + 2 * (tn_cands.len() + k);
        let Some(d_st) = read_dec(base) else { continue };
        if let Some((p, liq)) = px(base, d_wn, d_st, c.is0, c.dex) {
            if p > 0.0 && liq >= 100.0 {
                wn_usd = Some(wn_usd.map_or(p, |b: f64| b.min(p)));
            }
        }
    }
    let wn_usd = wn_usd?;
    let mut best2: Option<f64> = None;
    for (k, c) in tn_cands.iter().enumerate() {
        let base = 2 + 2 * k;
        let Some(d_q) = read_dec(base) else { continue };
        if let Some((p, _)) = px(base, d_tok, d_q, c.is0, c.dex) {
            let usd = p * wn_usd;
            if usd.is_finite() && usd > 0.0 {
                best2 = Some(best2.map_or(usd, |b: f64| b.min(usd)));
            }
        }
    }
    best2
}

async fn max_flash_loan(rpcs: &[String], lender: Address, token: Address) -> Option<U256> {
    let mut d = keccak256(b"maxFlashLoan(address)")[..4].to_vec();
    d.extend_from_slice(&[0u8; 12]);
    d.extend_from_slice(token.as_slice());
    u256_from_ret(&eth_call(rpcs, lender, Bytes::from(d)).await.ok()?)
}

/// On-chain proof a UniV3 pool can source `token`'s flash:
/// token_in must be the pool's token0/token1 AND the pool's token
/// balance must cover the borrow. All three reads ride one aggregate3.
async fn univ3_pool_holds(rpcs: &[String], pool: Address, token: Address, amount: U256) -> bool {
    let mut bal = keccak256(b"balanceOf(address)")[..4].to_vec();
    bal.extend_from_slice(&[0u8; 12]);
    bal.extend_from_slice(pool.as_slice());
    let res = batch_calls(
        rpcs,
        &[
            (pool, Bytes::from(keccak256(b"token0()")[..4].to_vec())),
            (pool, Bytes::from(keccak256(b"token1()")[..4].to_vec())),
            (token, Bytes::from(bal)),
        ],
    )
    .await;
    let holds = matches!(
        (res.first(), res.get(1)),
        (Some(Some(a)), Some(Some(b)))
            if addr_from_ret(a) == Some(token) || addr_from_ret(b) == Some(token)
    );
    holds
        && res
            .get(2)
            .and_then(|r| r.as_ref())
            .and_then(u256_from_ret)
            .map(|b| b >= amount)
            .unwrap_or(false)
}

/// GAP-5 dynamic flash fallbacks — invoked only when the static
/// provider list verifies nothing. ERC-3156 lender comes from
/// `ERC3156_LENDER_<CHAIN>` env (never a guessed address); UniV3-flash
/// candidates come from the verified pool registry, cheapest fee tier
/// first. Every candidate is re-verified on-chain before selection.
async fn erc3156_or_univ3_flash(
    rpcs: &[String],
    chain: Chain,
    token_in: Address,
    amount_in: U256,
) -> Result<Option<(Address, u64)>> {
    if let Ok(lender) = std::env::var(format!("ERC3156_LENDER_{}", chain.name().to_uppercase()))
    {
        if let Ok(addr) = lender.parse::<Address>() {
            if max_flash_loan(rpcs, addr, token_in)
                .await
                .map(|m| m >= amount_in)
                .unwrap_or(false)
            {
                return Ok(Some((addr, KIND_ERC3156)));
            }
        }
    }
    let t = format!("{token_in:?}").to_lowercase();
    let mut cands: Vec<_> = allbright_core::chain::load_pools()
        .into_iter()
        .filter(|p| {
            p.chain == chain
                && p.dex == DexKind::V3
                && (p.token0.address.eq_ignore_ascii_case(&t)
                    || p.token1.address.eq_ignore_ascii_case(&t))
        })
        .collect();
    cands.sort_by_key(|p| p.fee_bps);
    for p in cands {
        let Ok(addr) = p.address.parse::<Address>() else { continue };
        if univ3_pool_holds(rpcs, addr, token_in, amount_in).await {
            tracing::info!(chain = %chain.name(), pool = %p.address,
                fee_bps = p.fee_bps, "univ3 flash provider selected (fallback)");
            return Ok(Some((addr, KIND_UNIV3_FLASH)));
        }
    }
    Ok(None)
}

/// Everything the two submission paths share: executor lookup, ordered
/// RPC pool, signer, code-existence check, input token, flash-provider
/// verification (cheapest-first), USD→token minProfit conversion and
/// the final `execute(...)` calldata. Extracted so the sponsored
/// UserOp path and the Ethereum signed-tx private path run the
/// IDENTICAL verification sequence — fail-closed at every step.
struct PreparedExecute {
    executor: Address,
    rpcs: Vec<String>,
    signer: PrivateKeySigner,
    encoded: Vec<u8>,
}

async fn prepare_execute(cfg: &Config, req: &ExecutionRequest) -> Result<PreparedExecute> {
    let chain = req.signal.chain;
    let executor: Address = cfg
        .executor_address(chain)
        .ok_or_else(|| anyhow::anyhow!("no executor on {}", chain.name()))?
        .parse()?;
    // Ordered endpoint pool (keyed providers first, §36.9) — every
    // hot-path call races the top endpoints instead of pinning one.
    let rpcs = cfg
        .rpc_pools
        .get(&chain)
        .cloned()
        .filter(|v| !v.is_empty())
        .or_else(|| cfg.rpc_urls.get(&chain).cloned().map(|u| vec![u]))
        .ok_or_else(|| anyhow::anyhow!("no rpc for {}", chain.name()))?;
    let signer: PrivateKeySigner = cfg
        .signing_key()
        .ok_or_else(|| anyhow::anyhow!("PRIVATE_KEY unset"))?
        .parse()?;

    // The configured executor must actually have code on-chain.
    let code = eth_get_code(&rpcs, executor).await?;
    if code.len() <= 2 {
        anyhow::bail!("executor {executor:?} has no code on {}", chain.name());
    }

    if req.signal.route.is_empty() {
        anyhow::bail!("empty route");
    }
    let token_in: Address = req
        .signal
        .route[0]
        .token_in
        .parse()
        .map_err(|_| anyhow::anyhow!("token_in not an address"))?;

    // USD→token conversion: verified stablecoins price at 1. Non-stable
    // inputs (WETH/BNB-class profit routes) get a LIVE on-chain quote
    // against a verified stable inside the route's own pools — still
    // fail-closed when no quote exists (GAP-22; AGENTS.md §9/§13).
    // Phase-1 batch — one aggregate3 carries decimals(token_in) plus
    // every static flash-provider probe (cheapest-first order preserved
    // in the metadata). Was: 1 + N sequential eth_call RTTs.
    let providers = flash_providers(chain);
    if providers.is_empty() {
        anyhow::bail!("no flash provider for {}", chain.name());
    }
    let mut calls: Vec<(Address, Bytes)> = Vec::new();
    // (provider addr, kind, batch-index of its vault-balance probe)
    // Balancer flash loans revert when the Vault's token balance <
    // amount (BAL#528 observed live) — probe `token.balanceOf(vault)`
    // and require balance ≥ amount_in before choosing.
    let mut probe_meta: Vec<(Address, u64, Option<usize>)> = Vec::new();
    let bal_of_sel = keccak256(b"balanceOf(address)")[..4].to_vec();
    for (addr_str, kind) in providers {
        let (Ok(addr), Some(probe)) = (addr_str.parse::<Address>(), provider_probe(*kind))
        else {
            continue;
        };
        calls.push((addr, probe));
        let bal_ix = if *kind == KIND_BALANCER_V2 {
            calls.push((
                token_in,
                Bytes::from([bal_of_sel.clone(), vec![0u8; 12], addr.to_vec()].concat()),
            ));
            Some(calls.len() - 1)
        } else {
            None
        };
        probe_meta.push((addr, *kind, bal_ix));
    }
    calls.push((
        token_in,
        Bytes::from(keccak256(b"decimals()")[..4].to_vec()),
    ));
    let batch = batch_calls(&rpcs, &calls).await;
    // probe results sit at even strides only when every provider carries
    // the extra call — resolve each provider's probe index explicitly.
    let mut probe_results: Vec<(Address, u64, Option<usize>, Option<Bytes>)> =
        Vec::with_capacity(probe_meta.len());
    {
        let mut ix = 0usize;
        for (addr, kind, bal_ix) in &probe_meta {
            probe_results.push((*addr, *kind, *bal_ix, batch.get(ix).cloned().flatten()));
            ix += 1 + usize::from(bal_ix.is_some());
        }
    }
    let decimals = batch
        .last()
        .and_then(|r| r.as_ref())
        .and_then(|b| b.last().copied())
        .filter(|d| *d > 0 && *d <= 36)
        .ok_or_else(|| anyhow::anyhow!("decimals() unreadable for {token_in:?}"))?;

    let unit = 10f64.powi(decimals as i32);
    let amount_in = U256::from((req.signal.input_amount * unit).max(0.0) as u128);
    let chosen: Option<(Address, u64)> = probe_results
        .iter()
        .find_map(|(addr, kind, bal_ix, r)| {
            if !probe_verified(r.as_ref()) {
                return None;
            }
            if let Some(bi) = bal_ix {
                let bal = batch
                    .get(*bi)
                    .and_then(|r| r.as_ref())
                    .and_then(u256_from_ret)?;
                if bal < amount_in {
                    return None;
                }
            }
            Some((*addr, *kind))
        });
    let min_profit = if stable_addr(chain, token_in) {
        U256::from((req.signal.min_profit_usd * unit).max(0.0) as u128)
    } else {
        match quote_token_usd(&rpcs, chain, &req.signal.route, token_in).await {
            Some(usd) if usd > 0.0 && usd.is_finite() => {
                U256::from((req.signal.min_profit_usd / usd * unit).max(0.0) as u128)
            }
            _ => anyhow::bail!(
                "token_in {token_in:?} non-stable and no on-chain USD quote in route — refuse"
            ),
        }
    };
    if amount_in.is_zero() {
        anyhow::bail!("zero amountIn");
    }

    // Flash funding: the static list is already probed in the phase-1
    // batch (Balancer V2 / Morpho = 0% premium, Aave = 0.05%). First
    // verified candidate in cheapest-first order wins.
    let (pool_addr, kind) = match chosen {
        Some(pk) => pk,
        // GAP-5: static list exhausted → dynamic fallbacks. An
        // env-configured ERC-3156 lender first, then any verified V3
        // pool holding token_in (fee = its tier; the contract adds the
        // fee on top of minProfit so economics stay fail-safe).
        None => erc3156_or_univ3_flash(&rpcs, chain, token_in, amount_in)
            .await?
            .ok_or_else(|| anyhow::anyhow!("no verified flash provider on {}", chain.name()))?,
    };

    let encoded = calldata::encode_execute_full(
        &req.signal,
        amount_in,
        min_profit,
        pool_addr,
        Bytes::from(U256::from(kind).to_be_bytes::<32>().to_vec()),
    )?;
    Ok(PreparedExecute { executor, rpcs, signer, encoded })
}

/// BSC-class submission: sponsored UserOp → smart account →
/// `AllbrightExecutor.execute(legs, amountIn, minProfit, aavePool, kind)`.
/// Fail-closed at every verification step.
async fn submit_via_aa(cfg: &Config, req: &ExecutionRequest) -> Result<String> {
    let chain = req.signal.chain;
    let PreparedExecute { executor, rpcs, signer, encoded } =
        prepare_execute(cfg, req).await?;
    let pim = aa::PimlicoClient::new(
        cfg.pimlico_api_key
            .as_ref()
            .unwrap()
            .expose_secret()
            .to_string(),
    );

    // Zero-Gas Loss gate (GAP-1): run the exact execute() call inside an
    // eth_call context — flash loan + callbacks + swaps + the on-chain
    // minProfit assert all execute off-chain against live state. A
    // revert here means the real tx would revert identically → drop
    // BEFORE spending gas or a UserOp. The bail message carries the
    // revert data so bait forensics convicts the pool upstream.
    // Transport failure after a retry does not gate (on-chain minProfit
    // still bounds the risk); reverts always gate.
    let kernel = pim.account_address(&rpcs, signer.address()).await?;
    let mut gated = false;
    let mut transport_err: Option<String> = None;
    for _ in 0..2 {
        match eth_call_from(&rpcs, kernel, executor, Bytes::from(encoded.clone())).await {
            Ok(_) => { gated = true; break; }
            // EVM-level failure — the real tx would fail identically.
            // Bail carrying the revert data so bait forensics convicts.
            Err(DryRunErr::Evm(d)) => anyhow::bail!("evm dry-run gate: {d}"),
            Err(DryRunErr::Transport(e)) => transport_err = Some(e.to_string()),
        }
    }
    if !gated {
        tracing::warn!(chain = %chain.name(),
            err = %transport_err.unwrap_or_default(),
            "dry-run gate inconclusive (transport) — proceeding under minProfit guard");
    }

    let inner = aa::kernel_execute_calldata(executor, U256::ZERO, Bytes::from(encoded));
    pim.sponsored_user_op(chain.name(), chain.chain_id(), &rpcs, &signer, inner, false)
        .await
}

// ── Ethereum private submission (U14/U18) ────────────────────────────
// mev-templates / Flashbots-Protect pattern: a signed executor tx sent
// to a private endpoint — never the public mempool. Two lanes:
//   1. `eth_sendPrivateTransaction` → rpc.flashbots.net (free, no
//      account, no key — the tx lands privately or not at all).
//   2. `eth_sendBundle` → relay.flashbots.net — only when
//      FLASHBOTS_AUTH_KEY is set (reputation relay requires the
//      X-Flashbots-Signature auth header; target = next block).
// Honest constraints: the EOA pays gas (Ethereum is not gasless —
// AllbrightExecutor enforces minProfit on-chain which bounds the
// loss), and inclusion is never claimed — the return is the relay's
// own hash only.
const ETH_PRIVATE_TX_URL: &str = "https://rpc.flashbots.net";
const FLASHBOTS_RELAY_URL: &str = "https://relay.flashbots.net";

/// Parse a `0x`-hex JSON-RPC quantity result into u128.
fn rpc_hex_u128(v: &serde_json::Value) -> Option<u128> {
    let s = v.as_str()?.trim_start_matches("0x");
    u128::from_str_radix(if s.is_empty() { "0" } else { s }, 16).ok()
}

async fn rpc_quantity(rpcs: &[String], method: &str, params: serde_json::Value) -> Result<u128> {
    match race_rpc(rpcs, method, params).await {
        CallOutcome::Ok(v) => rpc_hex_u128(&v)
            .ok_or_else(|| anyhow::anyhow!("{method}: bad quantity {v}")),
        CallOutcome::Evm(m) => anyhow::bail!("{method} evm: {m}"),
        CallOutcome::Transport(m) => anyhow::bail!("{method} transport: {m}"),
    }
}

async fn submit_flashbots(cfg: &Config, req: &ExecutionRequest) -> Result<String> {
    use alloy::consensus::{SignableTransaction, TxEip1559};
    use alloy::eips::Encodable2718;
    use alloy::primitives::TxKind;
    use alloy::signers::Signer;

    let PreparedExecute { executor, rpcs, signer, encoded } =
        prepare_execute(cfg, req).await?;
    let from = signer.address();

    // Same zero-gas-loss gate as the AA path — `from` is the paying EOA
    // here, not the Kernel account.
    let mut gated = false;
    let mut transport_err: Option<String> = None;
    for _ in 0..2 {
        match eth_call_from(&rpcs, from, executor, Bytes::from(encoded.clone())).await {
            Ok(_) => { gated = true; break; }
            Err(DryRunErr::Evm(d)) => anyhow::bail!("evm dry-run gate: {d}"),
            Err(DryRunErr::Transport(e)) => transport_err = Some(e.to_string()),
        }
    }
    if !gated {
        tracing::warn!(err = %transport_err.unwrap_or_default(),
            "eth dry-run gate inconclusive (transport) — proceeding under minProfit guard");
    }

    // Nonce / gas / fees. The priority fee IS the ordering bid (U18):
    // builders rank bundles by it. Default 2 gwei, tunable via env.
    let nonce = rpc_quantity(
        &rpcs,
        "eth_getTransactionCount",
        json!([format!("{from:?}"), "pending"]),
    )
    .await? as u64;
    let est = rpc_quantity(
        &rpcs,
        "eth_estimateGas",
        json!([{"from":format!("{from:?}"),"to":format!("{executor:?}"),
            "data":format!("0x{}",alloy::primitives::hex::encode(&encoded))}]),
    )
    .await?;
    let gas_limit = (est * 6 / 5).min(1_500_000) as u64;
    let prio: u128 = std::env::var("ETH_PRIORITY_WEI")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2_000_000_000); // 2 gwei — the default builder bid
    let basefee = match race_rpc(&rpcs, "eth_feeHistory", json!([1, "latest", []])).await {
        CallOutcome::Ok(v) => v["baseFeePerGas"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(rpc_hex_u128)
            .unwrap_or(0),
        _ => 0,
    };
    let max_fee = if basefee > 0 { basefee * 2 + prio } else { prio * 10 };

    let tx = TxEip1559 {
        chain_id: 1,
        nonce,
        gas_limit,
        max_fee_per_gas: max_fee,
        max_priority_fee_per_gas: prio,
        to: TxKind::Call(executor),
        value: U256::ZERO,
        access_list: Default::default(),
        input: Bytes::from(encoded),
    };
    let sig = signer.sign_hash(&tx.signature_hash()).await?;
    let raw = tx.into_signed(sig).encoded_2718();
    let raw_hex = format!("0x{}", alloy::primitives::hex::encode(&raw));

    let client = shared_client();
    if let Ok(auth_key) = std::env::var("FLASHBOTS_AUTH_KEY") {
        // eth_sendBundle — reputation relay, signed auth header,
        // next-block target. Returns a bundle hash, not a tx hash.
        let auth: PrivateKeySigner = auth_key
            .parse()
            .map_err(|_| anyhow::anyhow!("FLASHBOTS_AUTH_KEY invalid"))?;
        let target = rpc_quantity(&rpcs, "eth_blockNumber", json!([])).await? + 1;
        let body = json!({"jsonrpc":"2.0","id":1,"method":"eth_sendBundle",
            "params":[{"txs":[raw_hex],"blockNumber":format!("0x{target:x}")}]})
        .to_string();
        let sig = auth
            .sign_message(&keccak256(body.as_bytes()).0)
            .await?;
        let resp = client
            .post(std::env::var("FLASHBOTS_RELAY_URL").unwrap_or_else(|_| FLASHBOTS_RELAY_URL.into()))
            .header("Content-Type", "application/json")
            .header("X-Flashbots-Signature", format!("{:?}:{}", auth.address(), sig))
            .body(body)
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;
        if let Some(err) = resp.get("error") {
            anyhow::bail!("eth_sendBundle: {}", err["message"].as_str().unwrap_or("err"));
        }
        return Ok(format!("bundle:{}", resp["result"]["bundleHash"].as_str().unwrap_or("")));
    }

    // eth_sendPrivateTransaction — Flashbots Protect fast mode. Free,
    // keyless; lands privately or not at all.
    let url = std::env::var("ETH_PRIVATE_TX_URL")
        .unwrap_or_else(|_| ETH_PRIVATE_TX_URL.into());
    let resp = client
        .post(url)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"eth_sendPrivateTransaction",
            "params":[{"tx":raw_hex,"preferences":{"fast":true}}]}))
        .send()
        .await?
        .json::<serde_json::Value>()
        .await?;
    if let Some(err) = resp.get("error") {
        anyhow::bail!("eth_sendPrivateTransaction: {}",
            err["message"].as_str().unwrap_or("err"));
    }
    resp["result"].as_str()
        .map(|h| h.to_string())
        .ok_or_else(|| anyhow::anyhow!("private tx: empty hash"))
}
