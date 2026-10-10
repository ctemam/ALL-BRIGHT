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
/// Ethereum → Flashbots `eth_sendBundle` (auth key required).
/// Chains with a deployed executor → sponsored UserOp through the Kernel
/// smart account (the only caller `AllbrightExecutor` accepts).
/// Anything else → refuse rather than leak orderflow.
pub async fn submit(cfg: &Config, req: &ExecutionRequest, _calldata: &[u8]) -> Result<String> {
    match req.signal.chain {
        Chain::Eth => submit_flashbots(cfg, req).await,
        c if cfg.executor_address(c).is_some()
            && cfg.pimlico_api_key.is_some()
            && cfg.signing_key().is_some() =>
        {
            submit_via_aa(cfg, req).await
        }
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

/// Canonical stablecoin addresses per chain — used solely for USD→token
/// conversion of `minProfit`. Membership is verified at runtime by a
/// successful `decimals()` call; unknown tokens fail closed.
fn stable_addr(chain: Chain, token: Address) -> bool {
    let t = format!("{token:?}").to_lowercase();
    let list: &[&str] = match chain {
        // Every entry verified on-chain via decimals() on 2026-10-08.
        Chain::Eth => &[
            "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48", // USDC
            "0xdac17f958d2ee523a2206206994597c13d831ec7", // USDT
            "0x6b175474e89094c44da98b954eedeac495271d0f", // DAI
        ],
        Chain::Arb => &[
            "0xaf88d065e77c8cc2239327c5edb3a432268e5831", // USDC (native)
            "0xff970a61a04b1ca14834a43f5de4533ebddb5cc8", // USDC.e
            "0xfd086bc7cd5c481dcc9c85ebe478a1c0b69fcbb9", // USD₮0
            "0xda10009cbd5d07dd0cecc66161fc93d7c9000da1", // DAI
        ],
        Chain::Op => &[
            "0x0b2c639c533813f4aa9d7837caf62653d097ff85", // USDC
            "0x94b008aa00579c1307b0ef2c499ad98a8ce58e58", // USDT
            "0xda10009cbd5d07dd0cecc66161fc93d7c9000da1", // DAI
        ],
        Chain::Poly => &[
            "0x3c499c542cef5e3811e1192ce70d8cc03d5c3359", // USDC (native)
            "0x2791bca1f2de4661ed88a30c99a7a9449aa84174", // USDC.e
            "0xc2132d05d31c914a87c6611c10748aeb04b58e8f", // USDT
            "0x8f3cf7ad23cd3cadbd9735aff958023239c6a063", // DAI
        ],
        Chain::Bsc => &[
            "0x55d398326f99059ff775485246999027b3197955", // USDT
            "0x8ac76a51cc950d9822d68b83fe1ad97b32cd580d", // USDC
            "0xc5f0f7b66764f6ec8c8dff7ba683102295e16409", // FDUSD
            "0x1af3f329e8be154074d8769d1ffa4ee058b1dbc3", // DAI
            "0x8d0d000ee44948fc98c9b98a4fa4921476f08b0d", // USD1
        ],
        Chain::Avax => &[
            "0xb97ef9ef8734c71904d8002f8b6bc66dd9c48a6e", // USDC
            "0x9702230a8ea53601f5cd2dc00fdbc13d4df4a8c7", // USDT
            "0xd586e7f844cea2f87f50152665bcbc2c279d8d70", // DAI.e
        ],
        Chain::Base => &[
            "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913", // USDC
            "0xfde4c96c8593536e31f229ea8f37b2ada2699bb2", // USDT
            "0x50c5725949a6f0c72e6c4a641f24049a917db0cb", // DAI
        ],
        Chain::Linea => &[
            "0x176211869ca2b568f2a7d4ee941e073a821ee1ff", // USDC
            "0xa219439258ca9da29e9cc4ce5596924745e12b93", // USDT
            "0x4af15ec2a0bd43db75dd04e62faa3b8ef36b00d5", // DAI
        ],
        Chain::Scroll => &[
            "0x06efdbff2a14a7c8e15944d1f4a48f9f95f663a4", // USDC
            "0xf55bec9cafdbe8730f096aa55dad6d22d44099df", // USDT
            "0xca77eb3fefe3725dc33bccb54edefc3d9f764f97", // DAI
        ],
        Chain::Blast => &[
            "0x4300000000000000000000000000000000000003", // USDB (native, rebasing)
        ],
    };
    list.iter().any(|a| *a == t)
}

async fn eth_call(rpc: &str, to: Address, data: Bytes) -> Result<Bytes> {
    let client = reqwest::Client::new();
    let v: serde_json::Value = client
        .post(rpc)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"eth_call",
            "params":[{"to":format!("{to:?}"),"data":format!("0x{}",alloy::primitives::hex::encode(&data))},"latest"]}))
        .send()
        .await?
        .json()
        .await?;
    let s = v["result"].as_str().unwrap_or("0x");
    Ok(Bytes::from(
        alloy::primitives::hex::decode(s.trim_start_matches("0x")).unwrap_or_default(),
    ))
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

async fn eth_call_from(rpc: &str, from: Address, to: Address, data: Bytes) -> std::result::Result<Bytes, DryRunErr> {
    let client = reqwest::Client::new();
    let v: serde_json::Value = client
        .post(rpc)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"eth_call",
            "params":[{"from":format!("{from:?}"),"to":format!("{to:?}"),
                "data":format!("0x{}",alloy::primitives::hex::encode(&data))},"latest"]}))
        .send()
        .await
        .map_err(|e| DryRunErr::Transport(e.into()))?
        .json()
        .await
        .map_err(|e| DryRunErr::Transport(e.into()))?;
    if let Some(err) = v.get("error") {
        let msg = err["message"].as_str().unwrap_or("evm error");
        let data = err["data"].as_str().unwrap_or_default();
        return Err(DryRunErr::Evm(format!("{msg} {data}")));
    }
    let s = v["result"].as_str().unwrap_or("0x");
    Ok(Bytes::from(
        alloy::primitives::hex::decode(s.trim_start_matches("0x")).unwrap_or_default(),
    ))
}

/// On-chain verification for a flash provider candidate:
/// Aave V3 → `getReservesList()` returns a non-empty array.
/// Balancer V2 → `getProtocolFeesCollector()` returns an address (only the
/// real Vault exposes it).
/// Morpho → `owner()` returns an address (Morpho Blue is owned).
async fn verify_provider(rpc: &str, addr: Address, kind: u64) -> bool {
    let probe: &[u8] = match kind {
        KIND_AAVE_V3 => &keccak256(b"getReservesList()")[..4],
        KIND_BALANCER_V2 => &keccak256(b"getProtocolFeesCollector()")[..4],
        KIND_MORPHO => &keccak256(b"owner()")[..4],
        _ => return false,
    };
    match eth_call(rpc, addr, Bytes::from(probe.to_vec())).await {
        Ok(out) => out.len() >= 32 && out.iter().any(|b| *b != 0),
        Err(_) => false,
    }
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

async fn erc20_balance_of(rpc: &str, token: Address, holder: Address) -> Option<U256> {
    let mut d = keccak256(b"balanceOf(address)")[..4].to_vec();
    d.extend_from_slice(&[0u8; 12]);
    d.extend_from_slice(holder.as_slice());
    u256_from_ret(&eth_call(rpc, token, Bytes::from(d)).await.ok()?)
}

async fn max_flash_loan(rpc: &str, lender: Address, token: Address) -> Option<U256> {
    let mut d = keccak256(b"maxFlashLoan(address)")[..4].to_vec();
    d.extend_from_slice(&[0u8; 12]);
    d.extend_from_slice(token.as_slice());
    u256_from_ret(&eth_call(rpc, lender, Bytes::from(d)).await.ok()?)
}

/// On-chain proof a UniV3 pool can source `token`'s flash:
/// token_in must be the pool's token0/token1 AND the pool's token
/// balance must cover the borrow.
async fn univ3_pool_holds(rpc: &str, pool: Address, token: Address, amount: U256) -> bool {
    let sel_t0 = Bytes::from(keccak256(b"token0()")[..4].to_vec());
    let sel_t1 = Bytes::from(keccak256(b"token1()")[..4].to_vec());
    let holds = match (
        eth_call(rpc, pool, sel_t0).await.ok().and_then(|b| addr_from_ret(&b)),
        eth_call(rpc, pool, sel_t1).await.ok().and_then(|b| addr_from_ret(&b)),
    ) {
        (Some(t0), Some(t1)) => t0 == token || t1 == token,
        _ => false,
    };
    if !holds {
        return false;
    }
    erc20_balance_of(rpc, token, pool)
        .await
        .map(|b| b >= amount)
        .unwrap_or(false)
}

/// GAP-5 dynamic flash fallbacks — invoked only when the static
/// provider list verifies nothing. ERC-3156 lender comes from
/// `ERC3156_LENDER_<CHAIN>` env (never a guessed address); UniV3-flash
/// candidates come from the verified pool registry, cheapest fee tier
/// first. Every candidate is re-verified on-chain before selection.
async fn erc3156_or_univ3_flash(
    rpc: &str,
    chain: Chain,
    token_in: Address,
    amount_in: U256,
) -> Result<Option<(Address, u64)>> {
    if let Ok(lender) = std::env::var(format!("ERC3156_LENDER_{}", chain.name().to_uppercase()))
    {
        if let Ok(addr) = lender.parse::<Address>() {
            if max_flash_loan(rpc, addr, token_in)
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
        if univ3_pool_holds(rpc, addr, token_in, amount_in).await {
            tracing::info!(chain = %chain.name(), pool = %p.address,
                fee_bps = p.fee_bps, "univ3 flash provider selected (fallback)");
            return Ok(Some((addr, KIND_UNIV3_FLASH)));
        }
    }
    Ok(None)
}

async fn erc20_decimals(rpc: &str, token: Address) -> Result<u8> {
    let sel = &keccak256(b"decimals()")[..4];
    let out = eth_call(rpc, token, Bytes::from(sel.to_vec())).await?;
    out.last()
        .copied()
        .filter(|d| *d > 0 && *d <= 36)
        .ok_or_else(|| anyhow::anyhow!("decimals() unreadable for {token:?}"))
}

/// BSC-class submission: sponsored UserOp → smart account →
/// `AllbrightExecutor.execute(legs, amountIn, minProfit, aavePool, kind)`.
/// Fail-closed at every verification step.
async fn submit_via_aa(cfg: &Config, req: &ExecutionRequest) -> Result<String> {
    let chain = req.signal.chain;
    let executor: Address = cfg
        .executor_address(chain)
        .ok_or_else(|| anyhow::anyhow!("no executor on {}", chain.name()))?
        .parse()?;
    let rpc = cfg
        .rpc_urls
        .get(&chain)
        .ok_or_else(|| anyhow::anyhow!("no rpc for {}", chain.name()))?
        .clone();
    let signer: PrivateKeySigner = cfg
        .signing_key()
        .ok_or_else(|| anyhow::anyhow!("PRIVATE_KEY unset"))?
        .parse()?;

    // The configured executor must actually have code on-chain.
    let client = reqwest::Client::new();
    let code_check: serde_json::Value = client
        .post(&rpc)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"eth_getCode",
            "params":[format!("{executor:?}"),"latest"]}))
        .send()
        .await?
        .json()
        .await?;
    if code_check["result"].as_str().unwrap_or("0x").len() <= 4 {
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

    // USD→token conversion is only defined for verified stablecoins —
    // anything else refuses (no guessed prices, AGENTS.md §9/§13).
    let decimals = erc20_decimals(&rpc, token_in).await?;
    if !stable_addr(chain, token_in) {
        anyhow::bail!("token_in {token_in:?} not a whitelisted stable — refuse");
    }
    let unit = 10f64.powi(decimals as i32);
    let amount_in = U256::from((req.signal.input_amount * unit).max(0.0) as u128);
    let min_profit = U256::from((req.signal.min_profit_usd * unit).max(0.0) as u128);
    if amount_in.is_zero() {
        anyhow::bail!("zero amountIn");
    }

    // Flash funding: walk the provider list cheapest-first (Balancer V2 /
    // Morpho = 0% premium, Aave = 0.05%) and use the first that verifies
    // on-chain. Every address is a candidate until the probe passes.
    let providers = flash_providers(chain);
    if providers.is_empty() {
        anyhow::bail!("no flash provider for {}", chain.name());
    }
    let mut chosen: Option<(Address, u64)> = None;
    for (addr_str, kind) in providers {
        let addr: Address = match addr_str.parse() {
            Ok(a) => a,
            Err(_) => continue,
        };
        if verify_provider(&rpc, addr, *kind).await {
            chosen = Some((addr, *kind));
            break;
        }
    }
    let (pool_addr, kind) = match chosen {
        Some(pk) => pk,
        // GAP-5: static list exhausted → dynamic fallbacks. An
        // env-configured ERC-3156 lender first, then any verified V3
        // pool holding token_in (fee = its tier; the contract adds the
        // fee on top of minProfit so economics stay fail-safe).
        None => erc3156_or_univ3_flash(&rpc, chain, token_in, amount_in)
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
    let kernel = pim.account_address(&rpc, signer.address()).await?;
    let mut gated = false;
    let mut transport_err: Option<String> = None;
    for _ in 0..2 {
        match eth_call_from(&rpc, kernel, executor, Bytes::from(encoded.clone())).await {
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
    pim.sponsored_user_op(chain.name(), chain.chain_id(), &rpc, &signer, inner, false)
        .await
}

async fn submit_flashbots(_cfg: &Config, _req: &ExecutionRequest) -> Result<String> {
    // GAP-4 (AGENTS Appendix B): this path previously returned a
    // fabricated "0xflashbots-pending" hash after a credential ping — a
    // truth violation (submission ≠ inclusion must never be faked).
    // Real eth_sendBundle (signed executor tx + bundle ordering) is not
    // implemented; fail closed until it is.
    anyhow::bail!("flashbots bundle path not implemented — refusing to fabricate submission")
}
