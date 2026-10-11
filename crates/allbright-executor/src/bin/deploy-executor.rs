//! Gasless executor deployment — `deploy-executor <chain|all> [--broadcast]`.
//!
//! Deploys `AllbrightExecutor` through the ERC-4337 smart account: the
//! Kernel account calls the CREATE2 proxy inside a Pimlico-sponsored
//! UserOp. No native-token balance is needed — the paymaster pays gas.
//!
//! The executor's `owner` constructor arg is the smart-account address,
//! so only sponsored UserOps can drive it — matching the BSC deployment.
//!
//! Default is a DRY RUN (sponsorship proof only). Pass `--broadcast` to
//! submit the UserOp on-chain. After broadcast the deployed address is
//! verified via eth_getCode and printed as `ALLBRIGHT_EXECUTOR_<CHAIN>`.

use alloy::primitives::{hex, Address, Bytes};
use alloy::signers::local::PrivateKeySigner;
use anyhow::{Context, Result};
use serde_json::json;

use allbright_core::{config::Config, events::Chain};
use allbright_executor::aa::{self, PimlicoClient};

fn initcode_with_owner(owner: Address) -> Result<Bytes> {
    // Creation bytecode from the forge artifact — never hand-maintained.
    let artifact = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../contracts/out/AllbrightExecutor.sol/AllbrightExecutor.json");
    let raw = std::fs::read_to_string(&artifact)
        .with_context(|| format!("read {}", artifact.display()))?;
    let v: serde_json::Value = serde_json::from_str(&raw)?;
    let object = v["bytecode"]["object"]
        .as_str()
        .context("bytecode.object missing")?;
    let mut code = hex::decode(object.trim_start_matches("0x"))?;
    // constructor(address _owner) — abi.encode single address arg.
    let mut arg = [0u8; 32];
    arg[12..].copy_from_slice(owner.as_slice());
    code.extend_from_slice(&arg);
    Ok(Bytes::from(code))
}

async fn get_code(rpc: &str, addr: Address) -> Result<usize> {
    let client = reqwest::Client::new();
    let v: serde_json::Value = client
        .post(rpc)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"eth_getCode",
            "params":[format!("{addr:?}"),"latest"]}))
        .send()
        .await?
        .json()
        .await?;
    Ok(v["result"].as_str().unwrap_or("0x").len().saturating_sub(2) / 2)
}

async fn deploy_chain(
    cfg: &Config,
    pim: &PimlicoClient,
    signer: &PrivateKeySigner,
    chain: Chain,
    broadcast: bool,
) -> Result<()> {
    let rpcs = cfg
        .rpc_pools
        .get(&chain)
        .cloned()
        .filter(|v| !v.is_empty())
        .or_else(|| cfg.rpc_urls.get(&chain).cloned().map(|u| vec![u]))
        .with_context(|| format!("no RPC configured for {}", chain.name()))?;
    let rpc = rpcs[0].clone();

    // Executor owner = the Kernel account on this chain (deterministic).
    let account = pim.account_address(&rpcs, signer.address()).await?;
    let initcode = initcode_with_owner(account)?;
    let predicted = aa::create2_address(alloy::primitives::B256::ZERO, &initcode);

    let existing = get_code(&rpc, predicted).await.unwrap_or(0);
    if existing > 0 {
        println!(
            "{}: executor already deployed at {predicted:?} ({}B) — set ALLBRIGHT_EXECUTOR_{}={predicted:?}",
            chain.name(), existing, chain.name().to_uppercase()
        );
        return Ok(());
    }

    let (addr, res) = pim
        .deploy_via_sponsored_userop(
            chain.name(),
            chain.chain_id(),
            &rpcs,
            signer,
            initcode,
            !broadcast,
        )
        .await?;

    if !broadcast {
        println!("{}: DRY-RUN sponsored deploy ok → predicted {addr:?} | {res}", chain.name());
        return Ok(());
    }

    println!("{}: userop submitted {res} → verifying {addr:?}…", chain.name());
    // Poll up to ~60s for the code to land.
    for _ in 0..20 {
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        if get_code(&rpc, addr).await.unwrap_or(0) > 0 {
            println!(
                "{}: DEPLOYED {addr:?} — set ALLBRIGHT_EXECUTOR_{}={addr:?}",
                chain.name(),
                chain.name().to_uppercase()
            );
            return Ok(());
        }
    }
    anyhow::bail!("{}: op submitted but no code at {addr:?} after 60s — check the bundler receipt", chain.name());
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env()
            .add_directive("allbright=info".parse().unwrap()))
        .init();
    allbright_core::init_crypto_provider();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let broadcast = args.iter().any(|a| a == "--broadcast");
    let target = args.iter().find(|a| !a.starts_with("--")).cloned();

    let cfg = Config::load()?;
    let key = cfg.pimlico_api_key.as_ref().context("PIMLICO_API_KEY unset")?;
    let pim = PimlicoClient::new(secrecy::ExposeSecret::expose_secret(key).to_string());
    let signer: PrivateKeySigner = cfg
        .signing_key()
        .context("PRIVATE_KEY unset")?
        .parse()?;

    let chains: Vec<Chain> = match target.as_deref() {
        None | Some("all") => Chain::ALL.to_vec(),
        Some(name) => vec![Chain::ALL
            .into_iter()
            .find(|c| c.name() == name)
            .with_context(|| format!("unknown chain {name}"))?],
    };

    for chain in chains {
        if let Err(e) = deploy_chain(&cfg, &pim, &signer, chain, broadcast).await {
            eprintln!("{}: FAILED — {e:#}", chain.name());
        }
    }
    Ok(())
}
