//! Read-only flash-provider verification probe (GAP-5/GAP-7).
//! Runs the SAME eth_call interface probes relay::verify_provider runs
//! against live RPCs and reports provider×chain×verified×premium as
//! JSONL — plus each endpoint's eth_simulateV1 capability (GAP-1).
//! No state is written; nothing is submitted.
//!
//! Usage:
//!   cargo run -p allbright-executor --example flash_provider_probe -- [chain|all] [rpc_url]
//! RPC resolution: arg > PROBE_RPC_<CHAIN> > PROBE_RPC > Config <CHAIN>_RPC_URL.
//! Never pass keyed URLs on shared machines — env is preferred.

use alloy::primitives::{hex, keccak256, Address};
use allbright_core::{config::Config, events::Chain};
use allbright_executor::simgate::SimGate;
use serde_json::{json, Value};

const KIND_AAVE_V3: u64 = 1;
const KIND_BALANCER_V2: u64 = 3;
const KIND_MORPHO: u64 = 4;

/// Mirror of relay.rs:44-96 (address, kind, name, premium_bps).
fn candidates(chain: Chain) -> &'static [(&'static str, u64, &'static str, u64)] {
    const BAL: &str = "0xBA12222222228d8Ba445958a75a0704d566BF2C8";
    const MORPHO: &str = "0xBBBBBbbBBb9cC5e90e3b3Af64BDaf62C37EEFFCb";
    match chain {
        Chain::Eth => &[
            (BAL, KIND_BALANCER_V2, "balancer_v2", 0),
            (MORPHO, KIND_MORPHO, "morpho", 0),
            ("0x87870Bca3F3fD6335C3F4ce8392D69350B4fA2", KIND_AAVE_V3, "aave_v3", 5),
        ],
        Chain::Bsc => &[
            (BAL, KIND_BALANCER_V2, "balancer_v2", 0),
            (MORPHO, KIND_MORPHO, "morpho", 0),
            ("0x6807dc923806fE8Fd134338EABCA509979a7e0cB", KIND_AAVE_V3, "aave_v3", 5),
        ],
        Chain::Base => &[
            (BAL, KIND_BALANCER_V2, "balancer_v2", 0),
            (MORPHO, KIND_MORPHO, "morpho", 0),
            ("0xA238Dd80C259a72e81d7e4664a9801593F98d1c5", KIND_AAVE_V3, "aave_v3", 5),
        ],
        Chain::Arb => &[
            (BAL, KIND_BALANCER_V2, "balancer_v2", 0),
            ("0x794a61358D6845594F94dc1DB02A252b5b4814aD", KIND_AAVE_V3, "aave_v3", 5),
        ],
        Chain::Op => &[
            (BAL, KIND_BALANCER_V2, "balancer_v2", 0),
            ("0x794a61358D6845594F94dc1DB02A252b5b4814aD", KIND_AAVE_V3, "aave_v3", 5),
        ],
        Chain::Poly => &[
            (BAL, KIND_BALANCER_V2, "balancer_v2", 0),
            ("0x794a61358D6845594F94dc1DB02A252b5b4814aD", KIND_AAVE_V3, "aave_v3", 5),
        ],
        Chain::Avax => &[
            (BAL, KIND_BALANCER_V2, "balancer_v2", 0),
            ("0x794a61358D6845594F94dc1DB02A252b5b4814aD", KIND_AAVE_V3, "aave_v3", 5),
        ],
        Chain::Linea => &[
            (BAL, KIND_BALANCER_V2, "balancer_v2", 0),
            ("0xc47b8C00b0f69a36fa203Ffeac0334874574a8Ac", KIND_AAVE_V3, "aave_v3", 5),
        ],
        Chain::Scroll => &[
            (BAL, KIND_BALANCER_V2, "balancer_v2", 0),
            ("0x11fCfe756c05AD438e312a7fd934381537D3cFfe", KIND_AAVE_V3, "aave_v3", 5),
        ],
        Chain::Blast => &[],
    }
}

async fn probe(client: &reqwest::Client, rpc: &str, addr: Address, kind: u64) -> bool {
    let sel: &[u8] = match kind {
        KIND_AAVE_V3 => &keccak256(b"getReservesList()")[..4],
        KIND_BALANCER_V2 => &keccak256(b"getProtocolFeesCollector()")[..4],
        KIND_MORPHO => &keccak256(b"owner()")[..4],
        _ => return false,
    };
    let v: Value = match client
        .post(rpc)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"eth_call",
            "params":[{"to":format!("{addr:?}"),"data":format!("0x{}",hex::encode(sel))},"latest"]}))
        .send()
        .await
    {
        Ok(r) => r.json().await.unwrap_or(Value::Null),
        Err(_) => return false,
    };
    v["result"]
        .as_str()
        .map(|s| {
            hex::decode(s.trim_start_matches("0x"))
                .map(|b| b.len() >= 32 && b.iter().any(|x| *x != 0))
                .unwrap_or(false)
        })
        .unwrap_or(false)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let chain_arg = std::env::args().nth(1).unwrap_or_else(|| "all".into());
    let rpc_arg = std::env::args().nth(2);
    let cfg_urls = Config::load().map(|c| c.rpc_urls).unwrap_or_default();
    let chains: Vec<Chain> = if chain_arg == "all" {
        Chain::ALL.to_vec()
    } else {
        Chain::ALL
            .iter()
            .copied()
            .filter(|c| c.name() == chain_arg)
            .collect()
    };
    let client = reqwest::Client::new();
    let gate = SimGate::new();
    for chain in chains {
        let rpc = rpc_arg
            .clone()
            .or_else(|| std::env::var(format!("PROBE_RPC_{}", chain.name().to_uppercase())).ok())
            .or_else(|| std::env::var("PROBE_RPC").ok())
            .or_else(|| cfg_urls.get(&chain).cloned());
        let Some(rpc) = rpc else {
            println!("{}", json!({"chain": chain.name(), "error": "no rpc configured"}));
            continue;
        };
        let sim_cap = gate.capability(&rpc).await.name();
        for (addr_s, kind, name, premium_bps) in candidates(chain) {
            let verified = match addr_s.parse::<Address>() {
                Ok(a) => probe(&client, &rpc, a, *kind).await,
                Err(_) => false,
            };
            println!(
                "{}",
                json!({
                    "chain": chain.name(), "candidate": name, "address": addr_s,
                    "kind": kind, "premium_bps": premium_bps,
                    "verified": verified, "sim_method": sim_cap,
                })
            );
        }
    }
    Ok(())
}
