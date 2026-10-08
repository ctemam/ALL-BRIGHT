//! Velora (ex-ParaSwap) quote collector → `DexQuote`.
//! Optional signal source: absent API key → collector idles gracefully.

use secrecy::ExposeSecret;
use std::time::Duration;
use tokio::sync::mpsc;

use allbright_core::{config::Config, events::Event};

pub async fn run(cfg: Config, tx: mpsc::Sender<Event>) {
    let Some(key) = cfg.velora_api_key.as_ref() else {
        tracing::info!("VELORA_API_KEY empty — velora collector idling");
        return;
    };
    let _ = key.expose_secret(); // used when real quotes are fetched

    let client = reqwest::Client::new();
    let base = std::env::var("VELORA_API_URL")
        .unwrap_or_else(|_| "https://api.velora.xyz".into());
    let interval = Duration::from_secs(cfg.scan_interval_secs.max(1));

    // Skeleton query set: WETH→USDC on mainnet, 1 native unit.
    loop {
        let url = format!(
            "{base}/prices?srcToken=0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2\
             &destToken=0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48\
             &amount=1000000000000000000&network=1&srcDecimals=18&destDecimals=6"
        );
        match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<serde_json::Value>().await {
                    Ok(v) => {
                        let dst = v
                            .pointer("/priceRoute/destAmount")
                            .and_then(|d| d.as_str())
                            .and_then(|s| s.parse::<f64>().ok())
                            .unwrap_or(0.0)
                            / 1e6;
                        let _ = tx
                            .send(Event::DexQuote(allbright_core::events::DexQuote {
                                chain: allbright_core::events::Chain::Eth,
                                src_token: "WETH".into(),
                                dst_token: "USDC".into(),
                                src_amount: 1.0,
                                dst_amount: dst,
                                ts_ms: allbright_core::events::now_ms(),
                            }))
                            .await;
                    }
                    Err(e) => tracing::debug!("velora decode: {e}"),
                }
            }
            Ok(resp) => tracing::debug!("velora http {}", resp.status()),
            Err(e) => tracing::debug!("velora request: {e}"),
        }
        tokio::time::sleep(interval).await;
    }
}
