//! Process bus: BSON-serialized `Event`s over localhost WebSocket.
//!
//! `BusServer` — one per producer process; fans every event out to all
//! connected subscribers and re-publishes anything it receives.
//! `BusClient` — connects to a server, yields incoming events, sends
//! outgoing ones. Reconnect-with-backoff lives on the caller side.

use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tokio::sync::{broadcast, mpsc};
use tokio_tungstenite::tungstenite::Message;

use crate::events::Event;

const CAP: usize = 4096;

pub fn encode(ev: &Event) -> Result<Vec<u8>> {
    Ok(bson::to_vec(ev)?)
}

pub fn decode(bytes: &[u8]) -> Result<Event> {
    Ok(bson::from_slice::<Event>(bytes)?)
}

/// Broadcast server: every accepted socket gets all events; anything a
/// client sends is fanned out to everyone else too.
pub struct BusServer {
    pub addr: SocketAddr,
    tx: broadcast::Sender<Vec<u8>>,
}

impl BusServer {
    pub async fn bind(addr: SocketAddr) -> Result<Self> {
        let listener = TcpListener::bind(addr).await?;
        let (tx, _) = broadcast::channel(CAP);
        let srv = BusServer { addr, tx: tx.clone() };
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, peer)) => {
                        let tx = tx.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_conn(stream, tx).await {
                                tracing::debug!(%peer, "bus conn closed: {e}");
                            }
                        });
                    }
                    Err(e) => tracing::warn!("bus accept error: {e}"),
                }
            }
        });
        Ok(srv)
    }

    /// Publish an event to every connected client.
    pub fn publish(&self, ev: &Event) -> Result<()> {
        let bytes = encode(ev)?;
        // Ignore lagging/absent subscribers.
        let _ = self.tx.send(bytes);
        Ok(())
    }
}

async fn handle_conn(
    stream: tokio::net::TcpStream,
    tx: broadcast::Sender<Vec<u8>>,
) -> Result<()> {
    let ws = tokio_tungstenite::accept_async(stream).await?;
    let (mut sink, mut stream) = ws.split();
    let mut rx = tx.subscribe();

    // Single select loop: a lagged or closed broadcast must TERMINATE the
    // connection so the client sees a real disconnect and reconnects for
    // a fresh stream. The previous split-writer design let a `Lagged`
    // error silently kill only the write half — the socket stayed open
    // and the client's `recv()` blocked forever (observed: dashboard
    // index frozen ~10h while the journal kept appending).
    loop {
        tokio::select! {
            msg = stream.next() => {
                match msg {
                    Some(Ok(Message::Binary(b))) => {
                        let _ = tx.send(b.to_vec());
                    }
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                    Some(_) => {}
                }
            }
            bytes = rx.recv() => {
                match bytes {
                    Ok(b) => {
                        if sink.send(Message::binary(b)).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(lagged = n,
                            "bus subscriber lagged — closing connection for resync");
                        let _ = sink.send(Message::Close(None)).await;
                        break;
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
    Ok(())
}

/// Subscriber/publisher handle. `incoming` yields decoded events; `send`
/// publishes to the server (which fans out to all other clients).
pub struct BusClient {
    pub incoming: mpsc::Receiver<Event>,
    out: mpsc::Sender<Vec<u8>>,
}

impl BusClient {
    /// Connect once; callers wanting resilience wrap this in a reconnect loop.
    pub async fn connect(addr: &str) -> Result<Self> {
        let url = format!("ws://{addr}");
        let (ws, _) = tokio_tungstenite::connect_async(&url).await?;
        let (mut sink, mut stream) = ws.split();

        let (in_tx, in_rx) = mpsc::channel::<Event>(CAP);
        let (out_tx, mut out_rx) = mpsc::channel::<Vec<u8>>(CAP);

        tokio::spawn(async move {
            while let Some(bytes) = out_rx.recv().await {
                if sink.send(Message::binary(bytes)).await.is_err() {
                    break;
                }
            }
        });

        tokio::spawn(async move {
            while let Some(msg) = stream.next().await {
                match msg {
                    Ok(Message::Binary(b)) => {
                        if let Ok(ev) = decode(&b[..]) {
                            if in_tx.send(ev).await.is_err() {
                                break;
                            }
                        }
                    }
                    Ok(Message::Close(_)) | Err(_) => break,
                    _ => {}
                }
            }
        });

        Ok(BusClient { incoming: in_rx, out: out_tx })
    }

    /// Connect with reconnect-with-backoff until success.
    pub async fn connect_retry(addr: &str) -> Self {
        let mut backoff = std::time::Duration::from_millis(250);
        loop {
            match Self::connect(addr).await {
                Ok(c) => return c,
                Err(e) => {
                    tracing::warn!("bus connect {addr} failed: {e}; retrying in {backoff:?}");
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(std::time::Duration::from_secs(5));
                }
            }
        }
    }

    pub fn send(&self, ev: &Event) -> Result<()> {
        let bytes = encode(ev)?;
        let _ = self.out.try_send(bytes);
        Ok(())
    }

    /// Split into independent halves so separate tasks can read/write.
    pub fn split(self) -> (mpsc::Receiver<Event>, BusSender) {
        (self.incoming, BusSender { out: self.out })
    }
}

#[derive(Clone)]
pub struct BusSender {
    out: mpsc::Sender<Vec<u8>>,
}

impl BusSender {
    pub fn send(&self, ev: &Event) -> Result<()> {
        let bytes = encode(ev)?;
        let _ = self.out.try_send(bytes);
        Ok(())
    }
}
