//! Gasless executor deploy: the Kernel smart account CREATE2-deploys
//! `AllbrightExecutor` through the deterministic-deployment proxy
//! (0x4e59…956C — same address on virtually every EVM chain) inside a
//! Pimlico-sponsored UserOp. Zero native balance needed anywhere.
//! Contract invariant: owner = the Kernel account (only it can execute).
//!
//!   cargo run --release --example aa_deploy -- bsc          # dry-run
//!   cargo run --release --example aa_deploy -- bsc --send   # deploys

use alloy::primitives::{keccak256, Address, Bytes, B256, U256};
use alloy::signers::local::PrivateKeySigner;
use anyhow::Result;
use secrecy::ExposeSecret;

use allbright_core::config::Config;

#[path = "../src/aa.rs"]
mod aa;

/// Universal CREATE2 deployer (Nick's deterministic-deployment-proxy).
const DEPLOYER: &str = "0x4e59b44847b379578588920cA78FbF26c0B4956C";

#[tokio::main]
async fn main() -> Result<()> {
    let chain = std::env::args().nth(1).unwrap_or_else(|| "bsc".into());
    let send = std::env::args().any(|a| a == "--send");
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

    let artifact: serde_json::Value = serde_json::from_slice(&std::fs::read(
        "contracts/out/AllbrightExecutor.sol/AllbrightExecutor.json",
    )?)?;
    let creation = artifact["bytecode"]["object"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("no bytecode in artifact"))?;
    let mut initcode = alloy::primitives::hex::decode(creation.trim_start_matches("0x"))?;

    let c = allbright_core::events::Chain::ALL
        .into_iter()
        .find(|c| c.name() == chain)
        .ok_or_else(|| anyhow::anyhow!("unknown chain {chain}"))?;
    let rpcs = cfg
        .rpc_pools
        .get(&c)
        .cloned()
        .filter(|v| !v.is_empty())
        .or_else(|| cfg.rpc_urls.get(&c).cloned().map(|u| vec![u]))
        .ok_or_else(|| anyhow::anyhow!("no rpc for {chain}"))?;
    let chain_id = c.chain_id();

    let pim = aa::PimlicoClient::new(key);
    let acct = pim.account_address(&rpcs, signer.address()).await?;
    println!("kernel account: {acct:?} (owner of the deployed executor)");

    // initCode = creation bytecode ‖ abi.encode(owner) — constructor(address).
    let mut owner_word = [0u8; 32];
    owner_word[12..].copy_from_slice(acct.as_slice());
    initcode.extend_from_slice(&owner_word);

    // Deployer presence check.
    let deployer: Address = DEPLOYER.parse()?;
    let client = reqwest::Client::new();
    let code: serde_json::Value = client
        .post(&rpcs[0])
        .json(&serde_json::json!({"jsonrpc":"2.0","id":1,"method":"eth_getCode",
            "params":[DEPLOYER,"latest"]}))
        .send().await?.json().await?;
    let present = code["result"].as_str().map(|s| s.len() > 4).unwrap_or(false);
    anyhow::ensure!(present, "deterministic deployer absent on {chain}");

    let salt = B256::ZERO;
    let mut deploy_input = salt.to_vec();
    deploy_input.extend_from_slice(&initcode);

    // Predicted address = last20(keccak256(0xff ‖ deployer ‖ salt ‖ keccak(initCode))).
    let mut pre = vec![0xffu8];
    pre.extend_from_slice(deployer.as_slice());
    pre.extend_from_slice(salt.as_slice());
    pre.extend_from_slice(keccak256(&initcode).as_slice());
    let predicted = Address::from_slice(&keccak256(&pre)[12..]);
    println!("predicted executor: {predicted:?}");

    let call = aa::kernel_execute_calldata(
        deployer,
        U256::ZERO,
        Bytes::from(deploy_input),
    );

    let res = pim
        .sponsored_user_op(&chain, chain_id, &rpcs, &signer, call, !send)
        .await?;
    println!("{res}");
    if send {
        println!("ALLBRIGHT_EXECUTOR_{}={predicted:?}", chain.to_uppercase());
    }
    Ok(())
}
