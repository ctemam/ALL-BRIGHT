//! Allbright Research Layer
//!
//! Architectural lineage:
//! - Allbright original: historical competitor/tip analysis and
//!   deterministic replay over recorded journals.
//!
//! Reads append-only BSONL journals written by the ingest recorder and
//! serves aggregate stats on the research bus (:3105). Scanning is
//! incremental — per-file byte offsets are tracked and only appended
//! bytes are decoded, so memory stays bounded as journals grow.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::RwLock;

use tracing::info;

use allbright_core::{
    bus::BusServer,
    config::Config,
    events::{now_ms, Event, ResearchStats},
};

/// Rolling aggregation over incrementally-scanned journal bytes.
#[derive(Default)]
struct JournalStats {
    stats: ResearchStats,
    profit_sum: f64,
    per_chain: HashMap<String, u64>,
    /// `component:class:origin` → count (source-origin diagnostics).
    failure_counts: HashMap<String, u64>,
    /// RejectReason string → count.
    rejection_counts: HashMap<String, u64>,
    /// path → byte offset already consumed (points at next record).
    offsets: HashMap<PathBuf, u64>,
}

impl JournalStats {
    /// Decode records appended since the last pass. Files that shrank
    /// (rotation/truncation) are rescanned from zero.
    fn scan_new(&mut self, dir: &str) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for ent in entries.flatten() {
            let path = ent.path();
            if path.extension().and_then(|s| s.to_str()) != Some("bsonl") {
                continue;
            }
            let meta_len = ent.metadata().map(|m| m.len()).unwrap_or(0);
            let mut off = *self.offsets.get(&path).unwrap_or(&0);
            if meta_len < off {
                off = 0; // rotated/truncated — rescan
            }
            let Ok(mut f) = std::fs::File::open(&path) else { continue };
            if f.seek(SeekFrom::Start(off)).is_err() {
                continue;
            }
            let mut buf = Vec::with_capacity(1 << 20);
            let mut chunk = [0u8; 1 << 20];
            loop {
                let n = match f.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                buf.extend_from_slice(&chunk[..n]);
                // Consume complete length-prefixed records.
                let mut used = 0usize;
                while used + 4 <= buf.len() {
                    let len =
                        u32::from_le_bytes(buf[used..used + 4].try_into().unwrap()) as usize;
                    if used + 4 + len > buf.len() {
                        break; // partial record — wait for more bytes
                    }
                    if let Ok(ev) =
                        bson::from_slice::<Event>(&buf[used + 4..used + 4 + len])
                    {
                        self.observe(&ev);
                    }
                    used += 4 + len;
                }
                off += used as u64;
                buf.drain(..used);
                if n < chunk.len() {
                    break;
                }
            }
            self.offsets.insert(path, off);
        }
        self.stats.per_chain_opps = self.per_chain.clone().into_iter().collect();
        self.stats.failures = self.failure_counts.clone().into_iter().collect();
        self.stats.rejections = self.rejection_counts.clone().into_iter().collect();
        self.stats.avg_expected_profit_usd = if self.stats.opportunities > 0 {
            self.profit_sum / self.stats.opportunities as f64
        } else {
            0.0
        };
        self.stats.ts_ms = now_ms();
    }

    fn observe(&mut self, ev: &Event) {
        self.stats.journal_events += 1;
        match ev {
            Event::Opportunity(o) => {
                self.stats.opportunities += 1;
                self.profit_sum += o.expected_profit_usd;
                *self.per_chain.entry(o.chain.name().to_string()).or_default() += 1;
            }
            Event::ExecResult(_) => self.stats.executions += 1,
            Event::Failure(f) => {
                *self
                    .failure_counts
                    .entry(format!("{:?}:{:?}:{:?}", f.component, f.class, f.origin))
                    .or_default() += 1;
            }
            Event::Observed(_) => self.stats.observed_universe += 1,
            Event::Rejected(r) => {
                *self
                    .rejection_counts
                    .entry(format!("{:?}", r.reason))
                    .or_default() += 1;
            }
            Event::Challenge(c) => {
                if c.challenged {
                    self.stats.challenged += 1;
                    self.stats.rescued_usd += c.counterfactual_net_usd;
                }
            }
            Event::DiscoveredPool(d) => {
                self.stats.pools_discovered += 1;
                if d.verified {
                    self.stats.pools_discovered_verified += 1;
                }
            }
            _ => {}
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("allbright=info".parse().unwrap()),
        )
        .init();
    allbright_core::init_crypto_provider();

    let cfg = Config::load()?;
    info!(config = ?cfg.redacted_summary(), "allbright-research starting");

    // Port can briefly be held by a just-stopped sibling — retry the bind
    // rather than dying into a PM2 restart storm.
    let addr: SocketAddr = ([127, 0, 0, 1], cfg.ports.research).into();
    let bus = loop {
        match BusServer::bind(addr).await {
            Ok(b) => break b,
            Err(e) => {
                tracing::warn!(%addr, "research bind failed: {e}; retrying in 2s");
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        }
    };
    info!(%addr, "research bus listening");

    let journal_dir =
        std::env::var("ALLBRIGHT_JOURNAL_DIR").unwrap_or_else(|_| "journal".into());
    let acc: Arc<RwLock<JournalStats>> = Arc::new(RwLock::new(JournalStats::default()));

    // Periodic incremental rescan + publish.
    tokio::spawn({
        let acc = acc.clone();
        let dir = journal_dir.clone();
        async move {
            loop {
                let dir2 = dir.clone();
                let acc2 = acc.clone();
                tokio::task::spawn_blocking(move || {
                    acc2.write().unwrap().scan_new(&dir2);
                })
                .await
                .ok();
                let (events, opps) = {
                    let a = acc.read().unwrap();
                    (a.stats.journal_events, a.stats.opportunities)
                };
                info!(events, opps, "journal scan");
                let stats = acc.read().unwrap().stats.clone();
                let _ = bus.publish(&Event::Stats(stats));
                tokio::time::sleep(std::time::Duration::from_secs(15)).await;
            }
        }
    });

    tokio::signal::ctrl_c().await.ok();
    Ok(())
}
