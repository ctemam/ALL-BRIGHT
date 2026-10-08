//! Gasless dry-run: assemble + sponsor a UserOp on BSC without sending.
//! callData is a harmless self-call (`execute(0,0,∅)`) — the point is to
//! prove: account derivation, gas estimation, and Pimlico paymaster
//! sponsorship all work with the configured key. Nothing is submitted.

use alloy::primitives::{Address, Bytes, U256};
use alloy::signers::local::PrivateKeySigner;
use anyhow::Result;
use secrecy::ExposeSecret;

use allbright_core::config::Config;

#[path = "../src/aa.rs"]
mod aa;

async fn probe(client: &reqwest::Client, rpc: &str, data: &[u8]) -> serde_json::Value {
    let cd = aa::factory_deploy_calldata(&Bytes::from(data.to_vec()));
    let body = serde_json::json!({"jsonrpc":"2.0","id":1,"method":"eth_call",
        "params":[{"to": aa::KERNEL_FACTORY, "data": format!("0x{}", alloy::primitives::hex::encode(&cd))}, "latest"]});
    client.post(rpc).json(&body).send().await.unwrap().json().await.unwrap()
}

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

    // Replay createAccount via eth_call to confirm the deploy simulates.
    let init = aa::kernel_init_data(signer.address());
    let client = reqwest::Client::new();
    let v = probe(&client, bsc_rpc, init.as_ref()).await;
    if v.get("error").is_some() {
        anyhow::bail!("createAccount simulation failed: {v}");
    }
    println!("createAccount simulates clean → {}", v["result"].as_str().unwrap_or(""));

    // execute(address(0), 0, "") — a no-op self-call, gasless-sponsored.
    let call = aa::kernel_execute_calldata(Address::ZERO, U256::ZERO, Bytes::new());
    let res = pim
        .sponsored_user_op("bsc", 56, bsc_rpc, &signer, call, true)
        .await?;
    println!("{res}");
    Ok(())
}
