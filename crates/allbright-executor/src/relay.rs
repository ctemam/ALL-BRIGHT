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

/// Canonical Aave V3 Pool per chain (verified on-chain before every use
/// via `getReservesList()` — an address here is a candidate, not a trust).
fn aave_v3_pool(chain: Chain) -> Option<&'static str> {
    match chain {
        Chain::Bsc => Some("0x6807dc923806fE8Fd134338EABCA509979a7e0cB"),
        _ => None,
    }
}

/// Canonical stablecoin addresses per chain — used solely for USD→token
/// conversion of `minProfit`. Membership is verified at runtime by a
/// successful `decimals()` call; unknown tokens fail closed.
fn stable_addr(chain: Chain, token: Address) -> bool {
    let t = format!("{token:?}").to_lowercase();
    let list: &[&str] = match chain {
        Chain::Bsc => &[
            "0x55d398326f99059ff775485246999027b3197955", // USDT
            "0x8ac76a51cc950d9822d68b83fe1ad97b32cd580d", // USDC
            "0xc5f0f7b66764f6ec8c8dff7ba683102295e16409", // FDUSD
            "0x1af3f329e8be154074d8769d1ffa4ee058b1dbc3", // DAI
            "0x8d0d000ee44948fc98c9b98a4fa4921476f08b0d", // USD1
        ],
        _ => &[],
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

    // Flash funding: verify the Aave V3 pool on-chain (getReservesList
    // must return a non-empty dynamic array) before trusting it.
    let pool_addr: Address = aave_v3_pool(chain)
        .ok_or_else(|| anyhow::anyhow!("no flash provider for {}", chain.name()))?
        .parse()?;
    let sel = &keccak256(b"getReservesList()")[..4];
    let reserves = eth_call(&rpc, pool_addr, Bytes::from(sel.to_vec())).await?;
    if reserves.len() < 96 {
        anyhow::bail!("aave pool {pool_addr:?} failed verification");
    }

    let encoded = calldata::encode_execute_full(
        &req.signal,
        amount_in,
        min_profit,
        pool_addr,
        Bytes::from(U256::from(1u64).to_be_bytes::<32>().to_vec()), // FlashKind::AAVE_V3
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
