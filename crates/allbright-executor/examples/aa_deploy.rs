//! LIVE: deploy `AllbrightExecutor` on BSC through the Kernel smart
//! account via a sponsored (gasless) UserOperation.
//!
//! Path: smart account → Safe Singleton Factory (EIP-2470,
//! 0x914d7Fec…43d7, verified live on BSC) → CREATE2 deploy of
//! `bytecode ‖ abi.encode(owner=smart account)`.

use alloy::primitives::{keccak256, Address, Bytes, B256, U256};
use alloy::signers::local::PrivateKeySigner;
use anyhow::{Context, Result};
use secrecy::ExposeSecret;

use allbright_core::config::Config;

#[path = "../src/aa.rs"]
mod aa;

const SINGLETON_FACTORY: &str = "0x914d7Fec6aaC8cd542e72Bca78B30650d45643d7";

fn read_initcode(owner: Address) -> Result<Bytes> {
    let artifact: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(
            "contracts/out/AllbrightExecutor.sol/AllbrightExecutor.json",
        )
        .context("run `forge build` in contracts/ first")?,
    )?;
    let hex_code = artifact["bytecode"]["object"]
        .as_str()
        .context("bytecode.object missing")?
        .trim_start_matches("0x");
    let mut init = alloy::primitives::hex::decode(hex_code)?;
    // constructor(address _owner) — abi-encoded arg appended.
    let mut arg = [0u8; 32];
    arg[12..].copy_from_slice(owner.as_slice());
    init.extend_from_slice(&arg);
    Ok(init.into())
}

fn create2_address(factory: Address, salt: B256, initcode: &[u8]) -> Address {
    let mut buf = Vec::with_capacity(1 + 20 + 32 + 32);
    buf.push(0xff);
    buf.extend_from_slice(factory.as_slice());
    buf.extend_from_slice(salt.as_slice());
    buf.extend_from_slice(keccak256(initcode).as_slice());
    Address::from_slice(&keccak256(&buf)[12..])
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
    println!("smart account: {acct:?}");

    let initcode = read_initcode(acct)?;
    let factory: Address = SINGLETON_FACTORY.parse()?;
    let salt = B256::ZERO;
    let predicted = create2_address(factory, salt, &initcode);
    println!("initcode: {}B  predicted executor: {predicted:?}", initcode.len());

    // Guard: refuse to redeploy over existing code.
    let client = reqwest::Client::new();
    let code: serde_json::Value = client.post(bsc_rpc).json(
        &serde_json::json!({"jsonrpc":"2.0","id":1,"method":"eth_getCode",
            "params":[format!("{predicted:?}"),"latest"]})
    ).send().await?.json().await?;
    if code["result"].as_str().unwrap_or("0x").len() > 4 {
        println!("ALREADY DEPLOYED at {predicted:?} — nothing sent");
        return Ok(());
    }

    // calldata to factory = salt ‖ initcode (Safe Singleton Factory ABI).
    let mut cd = Vec::with_capacity(32 + initcode.len());
    cd.extend_from_slice(salt.as_slice());
    cd.extend_from_slice(&initcode);
    let call = aa::kernel_execute_calldata(factory, U256::ZERO, Bytes::from(cd));

    let res = pim
        .sponsored_user_op("bsc", 56, bsc_rpc, &signer, call, false)
        .await?;
    println!("userOpHash: {res}");

    // Wait for inclusion, then verify owner() == smart account.
    let url = format!("https://api.pimlico.io/v2/bsc/rpc?apikey={}",
        cfg.pimlico_api_key.as_ref().unwrap().expose_secret());
    for _ in 0..30 {
        let v: serde_json::Value = client.post(&url).json(
            &serde_json::json!({"jsonrpc":"2.0","id":1,
                "method":"eth_getUserOperationReceipt","params":[res]})
        ).send().await?.json().await?;
        if let Some(r) = v.get("result").filter(|r| !r.is_null()) {
            println!("success={} gasUsed={} tx={}",
                r["success"], r["actualGasUsed"],
                r["receipt"]["transactionHash"]);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    }
    // owner() selector 0x8da5cb5b
    let owner_call: serde_json::Value = client.post(bsc_rpc).json(
        &serde_json::json!({"jsonrpc":"2.0","id":1,"method":"eth_call",
            "params":[{"to":format!("{predicted:?}"),"data":"0x8da5cb5b"},"latest"]})
    ).send().await?.json().await?;
    let raw = owner_call["result"].as_str().unwrap_or("0x");
    let onchain_owner = raw.get(26..).map(|s| format!("0x{s}")).unwrap_or_default();
    println!("executor at {predicted:?} owner()={onchain_owner} expected={acct:?}");
    Ok(())
}
