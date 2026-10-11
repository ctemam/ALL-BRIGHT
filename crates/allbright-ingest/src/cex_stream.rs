//! Binance bookTicker collector → `CexTick`.
//! Public market-data stream; independent of execution (per ToR).

use futures_util::StreamExt;
use serde::Deserialize;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

use allbright_core::events::{now_ms, CexTick, Event};
use allbright_core::lineage::{Component, FailureClass, FailureEvent};

// Majors for all configured chains' native assets → USD pricing.
const SYMBOLS: [&str; 5] = ["btcusdt", "ethusdt", "bnbusdt", "polusdt", "avaxusdt"];

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
    // Primary: the global book. Fallback: binance.us — same stream
    // schema, smaller book — for regions where stream.binance.com is
    // geo-blocked (HTTP 451, seen live). Venue tag follows the source
    // so downstream attribution stays honest.
    let urls = [
        (
            format!("wss://stream.binance.com:9443/stream?streams={streams}"),
            "binance",
        ),
        (
            format!("wss://stream.binance.us:9443/stream?streams={streams}"),
            "binance.us",
        ),
    ];
    let mut idx = 0usize;
    let mut fails = 0u32;
    loop {
        let (ref url, venue) = urls[idx];
        let (detail, res) = match run_once(url, venue, &tx).await {
            Ok(_) => ("stream closed".to_string(), Ok(())),
            Err(e) => (e.to_string(), Err(())),
        };
        tracing::warn!(source = venue, "binance stream ended: {detail}; reconnecting");
        // Geo-block is permanent for this process lifetime — switch
        // source immediately; generic drops retry the same source twice
        // before rotating (a flaky regional endpoint shouldn't win).
        fails += 1;
        if detail.contains("451") || fails >= 3 {
            idx = (idx + 1) % urls.len();
            fails = 0;
            tracing::warn!(source = urls[idx].1, "cex stream rotating source");
        }
        let _ = res;
        let _ = tx
            .send(Event::Failure(
                FailureEvent::new(Component::CexStream, FailureClass::StreamDrop, "cex.binance", detail),
            ))
            .await;
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
}

async fn run_once(
    url: &str,
    venue: &'static str,
    tx: &mpsc::Sender<Event>,
) -> anyhow::Result<()> {
    let (ws, _) = tokio_tungstenite::connect_async(url).await?;
    tracing::info!(source = venue, "bookTicker connected");
    let (_, mut stream) = ws.split();

    // A half-dead TCP socket can sit connected-but-silent forever —
    // observed live: "connected" logged, zero ticks reached the bus for
    // hours → every chain priced gas at the ETH fallback. 60 s without a
    // message on a 5-symbol bookTicker means the stream is dead.
    loop {
        let msg = match tokio::time::timeout(
            std::time::Duration::from_secs(60),
            stream.next(),
        )
        .await
        {
            Ok(Some(m)) => m,
            Ok(None) => break,
            Err(_) => anyhow::bail!("binance stream silent 60s"),
        };
        match msg {
            Ok(Message::Text(t)) => {
                if let Ok(c) = serde_json::from_str::<Combined>(&t) {
                    if let (Ok(bid), Ok(ask)) =
                        (c.data.b.parse::<f64>(), c.data.a.parse::<f64>())
                    {
                        let _ = tx
                            .send(Event::CexTick(CexTick {
                                venue: venue.into(),
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
