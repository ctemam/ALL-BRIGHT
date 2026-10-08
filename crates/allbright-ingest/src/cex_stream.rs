//! Binance bookTicker collector → `CexTick`.
//! Public market-data stream; independent of execution (per ToR).

use futures_util::StreamExt;
use serde::Deserialize;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

use allbright_core::events::{now_ms, CexTick, Event};
use allbright_core::lineage::{Component, FailureClass, FailureEvent};

// Majors for all configured chains' native assets → USD pricing.
const SYMBOLS: [&str; 5] = ["btcusdt", "ethusdt", "bnbusdt", "maticusdt", "avaxusdt"];

#[derive(Deserialize)]
struct Combined {
    data: BookTicker,
}

#[derive(Deserialize)]
struct BookTicker {
    s: String,
    b: String,
    a: String,
}

pub async fn run(tx: mpsc::Sender<Event>) {
    let streams = SYMBOLS
        .iter()
        .map(|s| format!("{s}@bookTicker"))
        .collect::<Vec<_>>()
        .join("/");
    let url = format!("wss://stream.binance.com:9443/stream?streams={streams}");

    loop {
        let (detail, res) = match run_once(&url, &tx).await {
            Ok(_) => ("stream closed".to_string(), Ok(())),
            Err(e) => (e.to_string(), Err(())),
        };
        tracing::warn!("binance stream ended: {detail}; reconnecting");
        let _ = res;
        let _ = tx
            .send(Event::Failure(
                FailureEvent::new(Component::CexStream, FailureClass::StreamDrop, "cex.binance", detail),
            ))
            .await;
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
}

async fn run_once(url: &str, tx: &mpsc::Sender<Event>) -> anyhow::Result<()> {
    let (ws, _) = tokio_tungstenite::connect_async(url).await?;
    tracing::info!("binance bookTicker connected");
    let (_, mut stream) = ws.split();

    while let Some(msg) = stream.next().await {
        match msg {
            Ok(Message::Text(t)) => {
                if let Ok(c) = serde_json::from_str::<Combined>(&t) {
                    if let (Ok(bid), Ok(ask)) =
                        (c.data.b.parse::<f64>(), c.data.a.parse::<f64>())
                    {
                        let _ = tx
                            .send(Event::CexTick(CexTick {
                                venue: "binance".into(),
                                symbol: c.data.s,
                                bid,
                                ask,
                                ts_ms: now_ms(),
                            }))
                            .await;
                    }
                }
            }
            Ok(Message::Close(_)) => break,
            Err(e) => return Err(e.into()),
            _ => {}
        }
    }
    Ok(())
}
