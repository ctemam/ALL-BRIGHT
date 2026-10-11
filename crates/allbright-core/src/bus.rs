//! Process bus: BSON-serialized `Event`s over localhost WebSocket.
//!
//! `BusServer` — one per producer process; fans every event out to all
//! connected subscribers and re-publishes anything it receives.
//! `BusClient` — connects to a server, yields incoming events, sends
//! outgoing ones. Reconnect-with-backoff lives on the caller side.
//!
//! GAP-29 publisher-side outbox (opt-in, wire-compatible):
//! - `BusServer` retains a bounded ring of recently published frames.
//! - A subscriber that sends the text control frame `ab-replay` right
//!   after connect receives the buffered backlog (seq < subscribe
//!   point) before live frames. Existing clients are byte-identical —
//!   the replay is just normal binary Event frames arriving early.
//! - Overflow evictions bump `dropped`; the 1st and every NOTICE_EVERY-th
//!   eviction emits a journaled `System{action:"event_dropped"}` event.
//! - File-spool alternative (fjall) evaluated and rejected: the BSONL
//!   journal already provides durable replay; an LSM write path would
//!   add fsync latency to the hot publish path for no durability gain
//!   at bus scope.

use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use tokio::sync::{broadcast, mpsc};
use tokio_tungstenite::tungstenite::Message;

use crate::events::{Event, SystemEvent};

const CAP: usize = 4096;

/// Text control frame a subscriber sends to request outbox replay.
/// Old servers ignore unknown text frames — opt-in, feature-detected.
pub const REPLAY_CMD: &str = "ab-replay";

/// Emit a journaled drop notice on the 1st eviction, then every Nth.
const NOTICE_EVERY: u64 = 4096;

pub fn encode(ev: &Event) -> Result<Vec<u8>> {
    Ok(bson::to_vec(ev)?)
}

pub fn decode(bytes: &[u8]) -> Result<Event> {
    Ok(bson::from_slice::<Event>(bytes)?)
}

#[derive(Debug, Clone, Copy)]
pub struct OutboxConfig {
    /// Max retained frames per publisher. Default 8192.
    pub capacity: usize,
}

impl Default for OutboxConfig {
    fn default() -> Self {
        OutboxConfig { capacity: 8192 }
    }
}

/// Bounded ring of recently published frames, each tagged with a
/// monotonic seq so reconnect replay can stop exactly at the point the
/// live stream took over.
struct Outbox {
    frames: VecDeque<(u64, Vec<u8>)>,
    capacity: usize,
    next_seq: u64,
    dropped: u64,
}

impl Outbox {
    fn new(capacity: usize) -> Self {
        Outbox {
            frames: VecDeque::with_capacity(capacity),
            capacity,
            next_seq: 0,
            dropped: 0,
        }
    }

    /// Push a frame; returns true when a drop-notice should be emitted.
    /// The notice frame itself is pushed via `push_silent` so eviction
    /// caused by the notice cannot cascade into another notice.
    fn push(&mut self, bytes: Vec<u8>) -> bool {
        let evicted = self.push_silent(bytes);
        evicted && (self.dropped == 1 || self.dropped % NOTICE_EVERY == 0)
    }

    fn push_silent(&mut self, bytes: Vec<u8>) -> bool {
        let seq = self.next_seq;
        self.next_seq += 1;
        let mut evicted = false;
        while self.frames.len() >= self.capacity {
            self.frames.pop_front();
            self.dropped += 1;
            evicted = true;
        }
        self.frames.push_back((seq, bytes));
        evicted
    }

    /// Frames older than `floor` — the reconnect gap, in order.
    fn replay_upto(&self, floor: u64) -> Vec<Vec<u8>> {
        self.frames
            .iter()
            .filter(|(s, _)| *s < floor)
            .map(|(_, b)| b.clone())
            .collect()
    }
}

/// Broadcast server: every accepted socket gets all events; anything a
/// client sends is fanned out to everyone else too.
pub struct BusServer {
    pub addr: SocketAddr,
    tx: broadcast::Sender<Vec<u8>>,
    outbox: Arc<Mutex<Outbox>>,
}

impl BusServer {
    pub async fn bind(addr: SocketAddr) -> Result<Self> {
        Self::bind_with_outbox(addr, OutboxConfig::default()).await
    }

    pub async fn bind_with_outbox(addr: SocketAddr, cfg: OutboxConfig) -> Result<Self> {
        let listener = TcpListener::bind(addr).await?;
        let local = listener.local_addr().unwrap_or(addr);
        let (tx, _) = broadcast::channel(CAP);
        let outbox = Arc::new(Mutex::new(Outbox::new(cfg.capacity)));
        let srv = BusServer {
            addr: local,
            tx: tx.clone(),
            outbox: outbox.clone(),
        };
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, peer)) => {
                        let tx = tx.clone();
                        let ob = outbox.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_conn(stream, tx, ob).await {
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

    /// Publish an event to every connected client AND the outbox ring.
    /// Ordering invariant: the outbox push and the broadcast send are
    /// atomic w.r.t. a subscriber's `subscribe()`+floor capture (same
    /// mutex), so a frame is either fully pre-subscribe (replayed) or
    /// fully post-subscribe (live) — never both, never neither.
    pub fn publish(&self, ev: &Event) -> Result<()> {
        let bytes = encode(ev)?;
        let mut ob = match self.outbox.lock() {
            Ok(g) => g,
            Err(_) => {
                // Poisoned mutex — still deliver live frames.
                let _ = self.tx.send(bytes);
                return Ok(());
            }
        };
        if ob.push(bytes.clone()) {
            let note = encode(&Event::System(SystemEvent {
                component: "bus".into(),
                action: "event_dropped".into(),
                detail: format!(
                    "outbox evicted; total_dropped={} capacity={}",
                    ob.dropped, ob.capacity
                ),
                ts_ms: crate::events::now_ms(),
            }))?;
            ob.push_silent(note.clone());
            let _ = self.tx.send(note);
        }
        let _ = self.tx.send(bytes);
        Ok(())
    }

    /// Frames evicted by outbox overflow — the journaled drop counter's
    /// live view (mirrors the emitted `event_dropped` System events).
    pub fn outbox_dropped(&self) -> u64 {
        self.outbox.lock().map(|o| o.dropped).unwrap_or(0)
    }

    pub fn outbox_len(&self) -> usize {
        self.outbox.lock().map(|o| o.frames.len()).unwrap_or(0)
    }
}

async fn handle_conn(
    stream: tokio::net::TcpStream,
    tx: broadcast::Sender<Vec<u8>>,
    outbox: Arc<Mutex<Outbox>>,
) -> Result<()> {
    let ws = tokio_tungstenite::accept_async(stream).await?;
    let (mut sink, mut stream) = ws.split();

    // Subscribe + capture the seq floor atomically (see publish()).
    let (mut rx, floor) = {
        let ob = outbox
            .lock()
            .map_err(|_| anyhow::anyhow!("outbox poisoned"))?;
        (tx.subscribe(), ob.next_seq)
    };
    let mut replayed = false;

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
                    // GAP-29: opt-in replay — send the pre-subscribe gap
                    // (seq < floor), then the live stream continues.
                    Some(Ok(Message::Text(t))) if !replayed && &*t == REPLAY_CMD => {
                        replayed = true;
                        let backlog = outbox
                            .lock()
                            .map(|ob| ob.replay_upto(floor))
                            .unwrap_or_default();
                        for b in backlog {
                            if sink.send(Message::binary(b)).await.is_err() {
                                return Ok(());
                            }
                        }
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
        Self::connect_inner(addr, false).await
    }

    /// Connect AND request outbox replay (GAP-29): the server sends its
    /// bounded backlog (frames published before our subscribe point)
    /// before live frames. On a pre-GAP-29 server the control text is
    /// ignored and behavior is identical to `connect`.
    pub async fn connect_replay(addr: &str) -> Result<Self> {
        Self::connect_inner(addr, true).await
    }

    async fn connect_inner(addr: &str, replay: bool) -> Result<Self> {
        let url = format!("ws://{addr}");
        let (ws, _) = tokio_tungstenite::connect_async(&url).await?;
        let (mut sink, mut stream) = ws.split();

        if replay {
            sink.send(Message::Text(REPLAY_CMD.into())).await?;
        }

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

        Ok(BusClient {
            incoming: in_rx,
            out: out_tx,
        })
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::SystemEvent;

    fn sys(detail: &str) -> Event {
        Event::System(SystemEvent {
            component: "test".into(),
            action: "t".into(),
            detail: detail.into(),
            ts_ms: crate::events::now_ms(),
        })
    }

    fn detail_of(ev: &Event) -> String {
        match ev {
            Event::System(s) => s.detail.clone(),
            _ => panic!("expected System"),
        }
    }

    #[test]
    fn outbox_evicts_oldest_and_counts() {
        let mut ob = Outbox::new(3);
        for i in 0..5u8 {
            ob.push(vec![i]);
        }
        assert_eq!(ob.dropped, 2);
        assert_eq!(ob.frames.len(), 3);
        // retained frames are the newest: seq 2,3,4
        let seqs: Vec<u64> = ob.frames.iter().map(|(s, _)| *s).collect();
        assert_eq!(seqs, vec![2, 3, 4]);
        // replay below a mid-point floor yields only the older tail
        assert_eq!(ob.replay_upto(4).len(), 2);
        assert_eq!(ob.replay_upto(0).len(), 0);
    }

    #[tokio::test]
    async fn replay_delivers_pre_subscribe_frames() {
        let srv = BusServer::bind("127.0.0.1:0".parse().unwrap())
            .await
            .unwrap();
        let addr = srv.addr.to_string();
        srv.publish(&sys("gap-1")).unwrap();
        srv.publish(&sys("gap-2")).unwrap();

        // Replay-capable client sees the gap frames first, in order.
        let mut c = BusClient::connect_replay(&addr).await.unwrap();
        let e1 = c.incoming.recv().await.unwrap();
        let e2 = c.incoming.recv().await.unwrap();
        assert_eq!(detail_of(&e1), "gap-1");
        assert_eq!(detail_of(&e2), "gap-2");

        // Then live frames flow normally.
        srv.publish(&sys("live")).unwrap();
        let e3 = c.incoming.recv().await.unwrap();
        assert_eq!(detail_of(&e3), "live");
    }

    #[tokio::test]
    async fn plain_client_gets_no_replay() {
        let srv = BusServer::bind("127.0.0.1:0".parse().unwrap())
            .await
            .unwrap();
        let addr = srv.addr.to_string();
        srv.publish(&sys("old")).unwrap();

        let mut c = BusClient::connect(&addr).await.unwrap();
        srv.publish(&sys("new")).unwrap();
        let e = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            c.incoming.recv(),
        )
        .await
        .unwrap()
        .unwrap();
        // First frame is the live one — no backlog leaked.
        assert_eq!(detail_of(&e), "new");
    }
}
