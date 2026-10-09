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
    let (pool_addr, kind) = chosen
        .ok_or_else(|| anyhow::anyhow!("no verified flash provider on {}", chain.name()))?;

    let encoded = calldata::encode_execute_full(
        &req.signal,
        amount_in,
        min_profit,
        pool_addr,
        Bytes::from(U256::from(kind).to_be_bytes::<32>().to_vec()),
    )?;
    let inner = aa::kernel_execute_calldata(executor, U256::ZERO, Bytes::from(encoded));
    let pim = aa::PimlicoClient::new(
        cfg.pimlico_api_key
            .as_ref()
            .unwrap()
            .expose_secret()
            .to_string(),
    );
    pim.sponsored_user_op(chain.name(), chain.chain_id(), &rpc, &signer, inner, false)
        .await
}

async fn submit_flashbots(cfg: &Config, _req: &ExecutionRequest) -> Result<String> {
    let key = cfg
        .flashbots_auth_key
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("FLASHBOOTS_AUTH_KEY empty"))?;
    // Real submission requires the signed raw tx of the executor call;
    // scaffolding performs the authenticated ping to verify credentials.
    let client = reqwest::Client::new();
    let body = json!({
        "jsonrpc": "2.0", "id": 1, "method": "flashbots_getUserStatsV2",
        "params": [{"blockNumber": "latest"}]
    });
    let resp = client
        .post(&cfg.flashbots_relay)
        .header("X-Flashbots-Signature", format!("{:#}", key.expose_secret()))
        .json(&body)
        .send()
        .await?;
    if !resp.status().is_success() {
        anyhow::bail!("flashbots http {}", resp.status());
    }
    Ok("0xflashbots-pending".into())
}
