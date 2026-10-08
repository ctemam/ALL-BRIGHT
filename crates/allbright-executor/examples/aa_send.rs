//! LIVE: send the sponsored UserOp on BSC — deploys the Kernel smart
//! account `0x6630…015b` and runs a no-op self-call inside it, gas paid
//! by Pimlico's verifying paymaster. Zero BNB spent.
//!
//! This is the first real on-chain write of the AA path.

use alloy::primitives::{Address, Bytes, U256};
use alloy::signers::local::PrivateKeySigner;
use anyhow::Result;
use secrecy::ExposeSecret;

use allbright_core::config::Config;

#[path = "../src/aa.rs"]
mod aa;

#[tokio::main]
async fn main() -> Result<()> {
    let cfg = Config::load()?;
    let key = cfg
        .pimlico_api_key
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("PIMLICO_API_KEY unset"))?
        .expose_secret()
        .to_string();
    let signer: PrivateKeySigner = cfg
        .signing_key()
        .ok_or_else(|| anyhow::anyhow!("PRIVATE_KEY unset"))?
        .parse()?;

    let pim = aa::PimlicoClient::new(key);
    let bsc_rpc = "https://bsc-rpc.publicnode.com";

    let acct = pim.account_address(bsc_rpc, signer.address()).await?;
    println!("smart account (Kernel v3.1, salt=0): {acct:?}");

    // execute(mode=single, dest=0, value=0, func=∅) — no-op proving the
    // account controls itself; deployment + execution happen atomically.
    let call = aa::kernel_execute_calldata(Address::ZERO, U256::ZERO, Bytes::new());
    let res = pim
        .sponsored_user_op("bsc", 56, bsc_rpc, &signer, call, false)
        .await?;
    println!("userOpHash: {res}");

    // Confirm inclusion + deployment.
    let client = reqwest::Client::new();
    let receipt = serde_json::json!({"jsonrpc":"2.0","id":1,
        "method":"eth_getUserOperationReceipt","params":[res]});
    let chain = aa::pimlico_chain("bsc");
    let url = format!("https://api.pimlico.io/v2/{chain}/rpc?apikey={}",
        cfg.pimlico_api_key.as_ref().unwrap().expose_secret());
    for _ in 0..20 {
        let v: serde_json::Value = client.post(&url).json(&receipt).send().await?.json().await?;
        if let Some(r) = v.get("result").filter(|r| !r.is_null()) {
            println!("INCLUDED: {}", serde_json::to_string_pretty(r).unwrap_or_default());
            break;
        }
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    }
    let code: serde_json::Value = client.post(bsc_rpc).json(
        &serde_json::json!({"jsonrpc":"2.0","id":1,"method":"eth_getCode",
            "params":[format!("{acct:?}"),"latest"]})
    ).send().await?.json().await?;
    let len = code["result"].as_str().unwrap_or("0x").len();
    println!("account code size: {}B (deployed={})", len.saturating_sub(2) / 2, len > 4);
    Ok(())
}
