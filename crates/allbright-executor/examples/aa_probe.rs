//! AA capability probe — free reads against the Pimlico bundler per chain.
//! Prints which chains expose EntryPoint v0.7 + gas-price support with the
//! configured API key. No secrets printed, no UserOps sent.

use anyhow::Result;
use secrecy::ExposeSecret;
use serde_json::json;

use allbright_core::config::Config;

const CHAINS: &[&str] = &[
    "ethereum", "arbitrum", "optimism", "polygon", "binance",
    "avalanche", "base", "linea", "scroll", "blast",
];

#[tokio::main]
async fn main() -> Result<()> {
    let cfg = Config::load()?;
    let key = cfg
        .pimlico_api_key
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("PIMLICO_API_KEY unset"))?
        .expose_secret()
        .to_string();
    let http = reqwest::Client::new();

    println!("{:-10} {:<12} {:<44} gasPrice", "chain", "entrypoints", "ep0.7");
    for chain in CHAINS {
        let url = format!("https://api.pimlico.io/v2/{chain}/rpc?apikey={key}");
        let eps = rpc(&http, &url, "eth_supportedEntryPoints", json!([])).await;
        let gp = rpc(&http, &url, "pimlico_getUserOperationGasPrice", json!([])).await;
        match (eps, gp) {
            (Ok(eps), Ok(gp)) => {
                let list: Vec<String> = eps
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default();
                let has07 = list
                    .iter()
                    .any(|e| e.eq_ignore_ascii_case("0x0000000071727De22E5E9d8BAf0edAc6f37da032"));
                let fast = gp["fast"]["maxFeePerGas"].as_str().unwrap_or("?");
                println!("{chain:<10} {:<12} {has07:<44} {fast}", list.len());
            }
            (Err(e), _) => println!("{chain:<10} ERROR {e}"),
            (_, Err(e)) => println!("{chain:<10} gasprice-err {e}"),
        }
    }
    Ok(())
}

async fn rpc(
    http: &reqwest::Client,
    url: &str,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value> {
    let body = json!({"jsonrpc":"2.0","id":1,"method":method,"params":params});
    let v: serde_json::Value = http.post(url).json(&body).send().await?.json().await?;
    if let Some(e) = v.get("error") {
        anyhow::bail!("{e}");
    }
    Ok(v["result"].clone())
}
