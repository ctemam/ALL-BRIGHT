//! Per-chain EVM collector → `PoolUpdate`.
//!
//! All pool reads for a chain are batched into ONE Multicall3
//! `aggregate3` request per cycle — this is what lets a free public RPC
//! sustain a 5 s scan loop across 10 chains / 50+ pools.
//!
//! First fetch also verifies `token0()`/`token1()` against the configured
//! descriptor; mismatches drop the pool with a warning (addresses are
//! config, not trust).

use alloy::eips::BlockId;
use alloy::providers::{Provider, ProviderBuilder};
use alloy::sol_types::SolCall;
use std::collections::{HashMap, HashSet};
use std::time::Duration;
use tokio::sync::mpsc;

use allbright_core::{
    chain::{self, PoolDescriptor},
    config::Config,
    events::{now_ms, Chain, DexKind, Event, HealAction, PoolState, Token},
    lineage::{Component, FailureClass, FailureEvent},
    multicall::{multicall3_addr, IMulticall3, IPairCalls, IV3PoolCalls},
};

alloy::sol! {
    /// Minimal ERC20 surface for P06 discovery verification.
    interface IERC20Min {
        function decimals() external view returns (uint8);
    }
}

/// Emit a classified failure onto the bus (journaled by ingest main).
async fn fail(tx: &mpsc::Sender<Event>, class: FailureClass, stage: &str, chain: Chain, detail: String) {
    let _ = tx
        .send(Event::Failure(FailureEvent::new(Component::EvmStream, class, stage, detail).on_chain(chain)))
        .await;
}

pub async fn run(cfg: Config, tx: mpsc::Sender<Event>) {
    let pools = chain::load_pools();
    let by_chain = chain::pools_by_chain(&pools);
    let interval = Duration::from_secs(cfg.scan_interval_secs.max(1));

    let mut handles = Vec::new();
    for (chain_id, rpcs) in cfg.rpc_pools.iter() {
        if !cfg.active_chains.contains(chain_id) {
            tracing::info!(chain = %chain_id.name(),
                "chain outside ACTIVE_CHAINS — collector not started");
            continue;
        }
        let Some(chain_pools) = by_chain.get(chain_id) else {
            tracing::info!(chain = %chain_id.name(), "no pools configured — chain idle");
            continue;
        };
        if rpcs.len() < 2 {
            tracing::warn!(chain = %chain_id.name(),
                "single-homed RPC — no failover capacity (AGENTS §32/§35)");
        }
        // Registry augmentation: env primaries + OVS-ordered QUALIFIED
        // backups (§36/§37). Refreshed again on failover wrap-around.
        let env_pool = rpcs.clone();
        let rpcs = augmented_endpoints(*chain_id, rpcs);
        let pools = chain_pools.clone();
        let tx = tx.clone();
        let chain = *chain_id;
        // P06: factory-event watcher — new pools enter the OBSERVATION
        // tier (DiscoveredPool events), never the live set directly.
        let factories: HashSet<String> = pools
            .iter()
            .filter_map(|p| p.factory.clone())
            .collect();
        let known: HashSet<String> =
            pools.iter().map(|p| p.address.to_lowercase()).collect();
        let rpcs2 = rpcs.clone();
        let tx2 = tx.clone();
        let wss = cfg.wss_urls.get(chain_id).cloned();
        let wss2 = wss.clone();
        // RPC-capacity scaling (AGENTS §37): shard the pool set across
        // parallel workers, each starting on a different endpoint of the
        // augmented list. GAP-17: a supervisor owns the shard set —
        // endpoint promotions/demotions in the registry rescale workers
        // at runtime instead of pinning the boot-time count forever.
        // Cross-shard coherence cell (arb.coherence fix): one shared head
        // per chain. Every shard merges the head it observes and pins its
        // aggregate3 to the max seen — all shards read the same block
        // instead of whatever block each endpoint happened to be on.
        let head = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let env_pool_s = env_pool.clone();
        let pools_s = pools.clone();
        let rpcs_s = rpcs.clone();
        let tx_s = tx.clone();
        let wss_s = wss.clone();
        let head_s = head.clone();
        handles.push(tokio::spawn(async move {
            shard_supervisor(chain, env_pool_s, rpcs_s, pools_s, interval, tx_s, wss_s, head_s).await;
        }));
        tracing::info!(chain = %chain.name(), endpoints = rpcs.len(),
            pools = pools.len(), "collector shard supervisor spawned");
        handles.push(tokio::spawn(async move {
            discovery_loop(chain, rpcs2, factories, known, tx2, wss2).await;
        }));
        // GAP-3: event-driven freshness — Swap/Sync/Mint/Burn logs on
        // watched pools trigger a targeted refresh, so a trade updates
        // our state within a block instead of waiting for the poll
        // cycle. L2-feasible equivalent of mempool awareness (no public
        // mempool exists on Arb/Base).
        let rpcs3 = rpcs.clone();
        let pools3 = chain_pools.clone();
        let tx3 = tx.clone();
        let wss3 = cfg.wss_urls.get(chain_id).cloned();
        handles.push(tokio::spawn(async move {
            swap_watch_loop(chain, rpcs3, pools3, tx3, wss3, head).await;
        }));
        // U1: mempool watcher — pending txs touching watched pools emit
        // PendingSwap (pre-confirmation trigger). Chains without a public
        // mempool fail-closed inside the loop.
        let rpcs4 = rpcs.clone();
        let pools4 = chain_pools.clone();
        let tx4 = tx.clone();
        let wss4 = cfg.wss_urls.get(chain_id).cloned();
        handles.push(tokio::spawn(async move {
            pending_watch_loop(chain, rpcs4, pools4, tx4, wss4).await;
        }));
    }

    if handles.is_empty() {
        tracing::warn!("no chain collectors — no RPC/pool overlap in config");
    }
    for h in handles {
        let _ = h.await;
    }
}

/// Classify an RPC error into the blindness taxonomy (AGENTS §32.5) —
/// never a generic failure.
fn classify_rpc_err(msg: &str) -> FailureClass {
    let m = msg.to_lowercase();
    if m.contains("429") || m.contains("rate limit") || m.contains("too many") {
        FailureClass::RpcRateLimit
    } else if m.contains("timed out") || m.contains("timeout") {
        FailureClass::RpcTimeout
    } else if m.contains("method not found") || m.contains("400") || m.contains("405") {
        FailureClass::RpcMethodUnavailable
    } else {
        FailureClass::RpcProviderFailure
    }
}

/// Host part of a URL for logs — endpoints may later carry keys; never
/// log the full URL.
fn host_of(url: &str) -> &str {
    url.split("://").nth(1).unwrap_or(url).split('/').next().unwrap_or(url)
}

/// Production endpoint shortlist per chain (AGENTS §36.8): env entries
/// stay first (operator-curated primaries), then registry-QUALIFIED
/// endpoints ordered by OVS. Bounded — the failover substrate is the
/// shortlist, not the whole 200+ universe.
const MAX_CHAIN_ENDPOINTS: usize = 32;

fn augmented_endpoints(chain: Chain, env_pool: &[String]) -> Vec<String> {
    let mut out: Vec<String> = env_pool.to_vec();
    let path = std::env::var("RPC_REGISTRY_PATH")
        .unwrap_or_else(|_| allbright_core::rpc_registry::REGISTRY_FILE.to_string());
    let reg = allbright_core::rpc_registry::RpcRegistry::load(std::path::Path::new(&path));
    for u in reg.qualified_urls(chain) {
        if out.len() >= MAX_CHAIN_ENDPOINTS {
            break;
        }
        if !out.iter().any(|e| e == &u) {
            out.push(u);
        }
    }
    // Fallback tier (§37.1): when Qualified is exhausted — or absent —
    // admit V4-verified Degraded endpoints (slow but economically
    // sighted). A chain with zero Qualified AND zero env URLs otherwise
    // goes fully dark while the registry holds usable endpoints.
    if out.len() < MAX_CHAIN_ENDPOINTS {
        for u in reg.fallback_urls(chain) {
            if out.len() >= MAX_CHAIN_ENDPOINTS {
                break;
            }
            if !out.iter().any(|e| e == &u) {
                out.push(u);
            }
        }
    }
    out
}

/// GAP-17 — shard supervisor: recomputes the desired shard count from
/// a FRESH registry read every 120s (the scout promotes/demotes
/// endpoints while collectors run). Rescales require two consecutive
/// matching desired counts (hysteresis — no thrash on a single noisy
/// sweep). A rescale respawns the shard set under the new modulo
/// partition and is journaled as a `Heal` action — never silent.
/// Dead workers are detected and respawned on the same tick.
async fn shard_supervisor(
    chain: Chain,
    env_pool: Vec<String>,
    rpcs: Vec<String>,
    pools: Vec<PoolDescriptor>,
    interval: Duration,
    tx: mpsc::Sender<Event>,
    wss: Option<String>,
    head: std::sync::Arc<std::sync::atomic::AtomicU64>,
) {
    const RESCALE_TICK: Duration = Duration::from_secs(120);
    const MAX_SHARDS: usize = 3;
    let mut rpcs = rpcs;
    let mut workers: Vec<tokio::task::JoinHandle<()>> = Vec::new();
    let mut current = 0usize;
    let mut pending: Option<usize> = None;
    let mut streak = 0u32;

    loop {
        let fresh = augmented_endpoints(chain, &env_pool);
        if fresh != rpcs {
            rpcs = fresh;
        }
        // Desired parallelism: ~1 worker per 4 endpoints, bounded by
        // MAX_SHARDS and by the pool count (empty shards are useless).
        let desired = (rpcs.len() / 4)
            .clamp(1, MAX_SHARDS)
            .min(pools.len().max(1));

        if desired == current {
            pending = None;
            streak = 0;
        } else {
            if pending == Some(desired) {
                streak += 1;
            } else {
                pending = Some(desired);
                streak = 1;
            }
            // Cold start scales immediately; rescales need 2 ticks.
            if workers.is_empty() || streak >= 2 {
                for w in &workers {
                    w.abort();
                }
                workers.clear();
                for shard in 0..desired {
                    let shard_pools: Vec<PoolDescriptor> = pools
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| i % desired == shard)
                        .map(|(_, p)| p.clone())
                        .collect();
                    if shard_pools.is_empty() {
                        continue;
                    }
                    // Rotate each shard's endpoint view so workers don't
                    // pile onto endpoint 0 simultaneously.
                    let mut shard_rpcs = rpcs.clone();
                    let n_rpc = shard_rpcs.len().max(1);
                    shard_rpcs.rotate_left(shard % n_rpc);
                    let env_s = env_pool.clone();
                    let tx_s = tx.clone();
                    let wss_s = wss.clone();
                    let head_w = head.clone();
                    workers.push(tokio::spawn(async move {
                        chain_loop(chain, env_s, shard_rpcs, shard_pools, interval, tx_s, wss_s, head_w)
                            .await;
                    }));
                }
                if current != 0 && !workers.is_empty() {
                    let _ = tx
                        .send(Event::Heal(HealAction {
                            target: "allbright-ingest".into(),
                            action: "shard_rescale".into(),
                            reason: format!(
                                "{}: shards {}→{} after registry reread",
                                chain.name(),
                                current,
                                workers.len()
                            ),
                            success: true,
                            restarts_in_window: 0,
                            ts_ms: now_ms(),
                        }))
                        .await;
                }
                tracing::info!(chain = %chain.name(), shards = workers.len(),
                    endpoints = rpcs.len(), "shard set (re)scaled");
                current = workers.len();
                pending = None;
                streak = 0;
            }
        }

        // Dead-worker detection: a finished chain_loop is reaped here;
        // if every shard died, current drops to 0 and the next tick
        // cold-starts the set again.
        workers.retain(|w| !w.is_finished());
        if workers.is_empty() {
            current = 0;
        }
        tokio::time::sleep(RESCALE_TICK).await;
    }
}

async fn chain_loop(
    chain: Chain,
    env_pool: Vec<String>,
    mut rpcs: Vec<String>,
    pools: Vec<PoolDescriptor>,
    interval: Duration,
    tx: mpsc::Sender<Event>,
    wss: Option<String>,
    head: std::sync::Arc<std::sync::atomic::AtomicU64>,
) {
    let mut ep = 0usize;
    let mut ep_fails = 0u32;
    let mut provider =
        ProviderBuilder::new().connect_http(rpcs[ep].parse().expect("rpc url must parse"));
    // Pools that failed verification are quarantined, not retried forever.
    let mut dead: HashSet<String> = HashSet::new();
    let mut verified: HashSet<String> = HashSet::new();
    // Material-change dedup: pool → last emitted state fingerprint.
    let mut last_seen: HashMap<String, [u64; 4]> = HashMap::new();

    // Prefer block-driven refresh: subscribe to newHeads over WSS (the
    // configured <CHAIN>_WSS_URL, else derive from the HTTP endpoint —
    // not every HTTP RPC offers a websocket transport). Falls back to
    // interval polling if the subscription can't be established or drops.
    let wss_url = wss.unwrap_or_else(|| rpcs[0].replacen("https://", "wss://", 1));
    let mut heads: Option<alloy::pubsub::SubscriptionStream<alloy::rpc::types::Header>> = None;
    // The provider owns the pubsub service — dropping it kills the
    // subscription. Must outlive `heads`.
    let mut _ws_guard = None;
    // If no head arrives within this window, refresh anyway (stall guard).
    let stall = (interval * 4).max(Duration::from_secs(30));

    use futures_util::StreamExt;
    // WSS failure backoff — after 3 consecutive failures the endpoint is
    // treated as WS-incapable for a cooling window (AGENTS §32.7); the
    // polling path keeps running either way.
    let mut wss_fail = 0u32;
    let mut wss_retry_at = std::time::Instant::now();
    loop {
        // (Re)subscribe to newHeads whenever the stream isn't live — retries
        // at scan-interval cadence when the WSS endpoint is flaky.
        if heads.is_none() && std::time::Instant::now() >= wss_retry_at {
            match ProviderBuilder::new()
                .connect_ws(alloy::providers::WsConnect::new(wss_url.as_str()))
                .await
            {
                Ok(ws) => match ws.subscribe_blocks().await {
                    Ok(sub) => {
                        tracing::info!(chain = %chain.name(), "newHeads subscription live — block-driven refresh");
                        heads = Some(sub.into_stream());
                        _ws_guard = Some(ws); // keep the pubsub service alive
                        wss_fail = 0;
                    }
                    Err(e) => {
                        tracing::warn!(chain = %chain.name(), "block subscribe failed ({e}); interval polling");
                        fail(&tx, FailureClass::StreamDrop, "evm.newHeads.subscribe", chain, e.to_string()).await;
                        wss_fail += 1;
                    }
                },
                Err(e) => {
                    let msg = e.to_string();
                    let class = if msg.contains("400") || msg.contains("405") {
                        FailureClass::RpcMethodUnavailable
                    } else if msg.contains("429") {
                        FailureClass::RpcRateLimit
                    } else {
                        FailureClass::RpcProviderFailure
                    };
                    if wss_fail < 3 {
                        tracing::warn!(chain = %chain.name(), "wss connect failed ({msg}); interval polling");
                    }
                    fail(&tx, class, "evm.newHeads.connect", chain, msg).await;
                    wss_fail += 1;
                }
            }
            if wss_fail >= 3 {
                // Endpoint is WS-incapable/degraded — retry in 5 min, not
                // every scan tick.
                wss_retry_at = std::time::Instant::now() + Duration::from_secs(300);
                wss_fail = 0;
            }
        }

        let mut do_refresh = true;
        if let Some(s) = heads.as_mut() {
            match tokio::time::timeout(stall, s.next()).await {
                Ok(Some(_h)) => {} // new block → refresh
                Ok(None) => {
                    tracing::warn!(chain = %chain.name(), "newHeads stream ended; interval polling");
                    fail(&tx, FailureClass::StreamDrop, "evm.newHeads.ended", chain, "stream closed by endpoint".into()).await;
                    heads = None;
                    _ws_guard = None;
                    do_refresh = false;
                }
                Err(_) => {} // stalled → refresh anyway
            }
        } else {
            tokio::time::sleep(interval).await;
        }
        if !do_refresh {
            continue;
        }
        let pin = match provider.get_block_number().await {
            Ok(own) => {
                let prev = head.fetch_max(own, std::sync::atomic::Ordering::Relaxed);
                prev.max(own)
            }
            Err(_) => head.load(std::sync::atomic::Ordering::Relaxed),
        };
        match refresh(&provider, &pools, &mut dead, &mut verified, &mut last_seen, chain, &tx, pin).await {
            Ok(()) => ep_fails = 0,
            Err(msg) => {
                // Silent-blindness fix: a failed state pull is a classified
                // visibility failure, not a quiet debug line.
                if ep_fails == 0 {
                    fail(&tx, classify_rpc_err(&msg), "evm.multicall", chain, msg).await;
                }
                ep_fails += 1;
                // Bounded self-heal (H2): rotate to the next configured
                // endpoint after consecutive failures.
                if ep_fails >= 2 && rpcs.len() > 1 {
                    let old = host_of(&rpcs[ep]).to_string();
                    ep = (ep + 1) % rpcs.len();
                    // Wrapped through every endpoint — re-pull the
                    // registry's qualified set (scout may have added
                    // fresh endpoints or quarantined bad ones).
                    if ep == 0 {
                        rpcs = augmented_endpoints(chain, &env_pool);
                    }
                    provider = ProviderBuilder::new()
                        .connect_http(rpcs[ep].parse().expect("rpc url must parse"));
                    tracing::warn!(chain = %chain.name(), "rpc failover {old} → {}", host_of(&rpcs[ep]));
                    fail(&tx, FailureClass::RpcProviderFailure, "evm.failover", chain,
                        format!("rotated endpoint {old} → {}", host_of(&rpcs[ep]))).await;
                    ep_fails = 0;
                }
            }
        }
    }
}

/// Factory deployment event signatures the P06 watcher subscribes to —
/// all computed, never memorized. Order: V2 pair, UniV3-style pool,
/// Solidly stable pool, tickSpacing-indexed CL pool (Slipstream/Aero).
fn factory_sigs() -> [alloy::primitives::B256; 4] {
    use alloy::primitives::keccak256;
    [
        keccak256("PairCreated(address,address,address,uint256)"),
        keccak256("PoolCreated(address,address,uint24,int24,address)"),
        keccak256("PoolCreated(address,address,bool,address)"),
        keccak256("PoolCreated(address,address,int24,address)"),
    ]
}

/// Decode a factory `*Created` log into (token0, token1, pool, kind).
/// Returns None for unknown signatures or malformed payloads — the log
/// is ignored, never trusted.
fn decode_pool_creation(
    topics: &[alloy::primitives::B256],
    data: &[u8],
) -> Option<(alloy::primitives::Address, alloy::primitives::Address, alloy::primitives::Address, DexKind)> {
    use alloy::primitives::Address;
    let [pair_created, pool_created_v3, pool_created_solidly, pool_created_cl] =
        factory_sigs();
    let topic_addr = |i: usize| -> Option<Address> {
        topics.get(i).map(|t| Address::from_slice(&t[12..]))
    };
    let data_addr = |i: usize| -> Option<Address> {
        let w = data.get(i * 32..i * 32 + 32)?;
        Some(Address::from_slice(&w[12..]))
    };
    let t0 = *topics.first()?;
    if t0 == pair_created {
        // PairCreated(token0, token1, pool, uint256)
        Some((topic_addr(1)?, topic_addr(2)?, data_addr(0)?, DexKind::V2))
    } else if t0 == pool_created_v3 {
        // PoolCreated(token0, token1, fee, tickSpacing, pool)
        Some((topic_addr(1)?, topic_addr(2)?, data_addr(1)?, DexKind::V3))
    } else if t0 == pool_created_solidly {
        // PoolCreated(token0, token1, stable, pool[, uint256])
        Some((topic_addr(1)?, topic_addr(2)?, data_addr(0)?, DexKind::Stable))
    } else if t0 == pool_created_cl {
        // PoolCreated(token0, token1, tickSpacing, pool)
        Some((topic_addr(1)?, topic_addr(2)?, data_addr(0)?, DexKind::V3))
    } else {
        None
    }
}

/// P06 new-pool monitor: subscribe to the factory deployment events of
/// every factory proven by the verified pool registry (factory addresses
/// come from pools.json provenance — never guessed).
///
/// Discovered pools are emitted as `Event::DiscoveredPool` after a
/// minimal on-chain verification (token0/token1 match + decimals). They
/// are observation-tier only — promotion to the execution universe goes
/// through the risk lifecycle, not this stream.
async fn discovery_loop(
    chain: Chain,
    rpcs: Vec<String>,
    factories: HashSet<String>,
    known: HashSet<String>,
    tx: mpsc::Sender<Event>,
    wss: Option<String>,
) {
    use alloy::primitives::{Address, B256};
    use alloy::rpc::types::Filter;
    use futures_util::StreamExt;

    if factories.is_empty() {
        return;
    }
    let factory_addrs: Vec<Address> = factories
        .iter()
        .filter_map(|f| f.parse().ok())
        .collect();
    if factory_addrs.is_empty() {
        return;
    }
    let mut ep = 0usize;
    let mut http =
        ProviderBuilder::new().connect_http(rpcs[ep].parse().expect("rpc url"));
    let wss_cands = wss_candidates(wss, &rpcs);
    if wss_cands.is_empty() {
        fail(&tx, FailureClass::RpcMethodUnavailable, "p06.factory.no_wss", chain,
            "no WS-capable endpoint configured or derivable — discovery off".into())
            .await;
        return;
    }
    let mut wss_idx = 0usize;

    let sigs = factory_sigs().to_vec();
    // Exponential backoff on WSS failure — a chain without WS capability
    // shouldn't burn failure events forever (RPC_BLINDNESS is one fact,
    // not a spam stream).
    let mut backoff = Duration::from_secs(10);

    // GAP-12 — token-risk gate (GoPlus): on-chain token0/token1 match is
    // necessary but not sufficient; a honeypot/taxed/mintable token is
    // never promoted to the verified tier. Verdicts are cached (6h
    // assessed / 60s unverified) so discovery doesn't hammer the API.
    let risk = allbright_core::token_risk::TokenRiskClient::new();

    let mut seen = known;
    loop {
        let ws = match ProviderBuilder::new()
            .connect_ws(alloy::providers::WsConnect::new(wss_cands[wss_idx].as_str()))
            .await
        {
            Ok(w) => w,
            Err(e) => {
                // AGENTS §32.5 — classify the blindness, don't emit a
                // generic drop: 4xx on WS upgrade = method unavailable on
                // this endpoint, 429 = rate limit, else provider failure.
                let msg = e.to_string();
                let class = if msg.contains("400") || msg.contains("405") || msg.contains("unsupported") {
                    FailureClass::RpcMethodUnavailable
                } else if msg.contains("429") {
                    FailureClass::RpcRateLimit
                } else {
                    FailureClass::RpcProviderFailure
                };
                fail(&tx, class, "p06.factory.connect", chain, msg).await;
                wss_idx = (wss_idx + 1) % wss_cands.len();
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(300));
                continue;
            }
        };
        let filter = Filter::new()
            .address(factory_addrs.clone())
            .event_signature(sigs.clone());
        let mut stream = match ws.subscribe_logs(&filter).await {
            Ok(s) => s.into_stream(),
            Err(e) => {
                fail(&tx, FailureClass::StreamDrop, "p06.factory.subscribe", chain, e.to_string()).await;
                // Rotate the verify endpoint too — a dead RPC shouldn't
                // stall discovery verification.
                if rpcs.len() > 1 {
                    ep = (ep + 1) % rpcs.len();
                    http = ProviderBuilder::new()
                        .connect_http(rpcs[ep].parse().expect("rpc url"));
                }
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(300));
                continue;
            }
        };
        backoff = Duration::from_secs(10);
        tracing::info!(chain = %chain.name(), factories = factory_addrs.len(),
            "p06 factory watch live");

        while let Some(log) = stream.next().await {
            let topics: &[B256] = log.topics();
            let Some((token0, token1, pool, dex)) =
                decode_pool_creation(topics, &log.data().data)
            else {
                continue;
            };
            let key = format!("{pool:?}").to_lowercase();
            if seen.contains(&key) { continue; }
            seen.insert(key);

            // Verify on-chain: pool's token0/token1 must match the event,
            // and both tokens must resolve decimals(). GAP-12 then gates
            // promotion on the GoPlus risk verdict — fail-closed:
            // Flagged AND Unverified both keep verified:false.
            let factory = log.address();
            let verified = verify_discovered(&http, pool, token0, token1).await
                && risk_gate(&risk, chain, token0, token1, &tx).await;
            let _ = tx.send(Event::DiscoveredPool(
                allbright_core::events::DiscoveredPool {
                    chain,
                    factory: format!("{factory:?}"),
                    pool: format!("{pool:?}"),
                    token0: format!("{token0:?}"),
                    token1: format!("{token1:?}"),
                    dex,
                    verified,
                    block: log.block_number.unwrap_or(0),
                    ts_ms: now_ms(),
                },
            )).await;
        }
        fail(&tx, FailureClass::StreamDrop, "p06.factory.ended", chain,
            "factory log stream closed".into()).await;
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

/// Swap/state-change event signatures the GAP-3 watcher subscribes to.
/// V2 `Sync` covers swap+mint+burn on constant-product pools; V3
/// `Swap`/`Mint`/`Burn` cover price moves and liquidity changes on CL;
/// Solidly `Sync(uint256,uint256)` covers the Stable lane (Velodrome/
/// Ramses-family — their Swap matches the V2 signature, Sync differs).
fn swap_sigs() -> [alloy::primitives::B256; 6] {
    use alloy::primitives::keccak256;
    [
        keccak256("Sync(uint112,uint112)"),
        keccak256("Sync(uint256,uint256)"),
        keccak256("Swap(address,uint256,uint256,uint256,uint256,address)"),
        keccak256("Swap(address,address,int256,int256,uint160,uint128,int24)"),
        keccak256("Mint(address,address,int24,int24,uint128,uint256,uint256)"),
        keccak256("Burn(address,address,int24,int24,uint256,uint256)"),
    ]
}

/// WSS candidate ladder (GAP-3 / GAP-34): registry-qualified WSS
/// endpoints come first (eth_subscribe grant confirmed by scout),
/// then the configured `<CHAIN>_WSS_URL`, then blind https→wss
/// derivation from the HTTP RPC pool as a last-resort fallback.
///
/// Preference order:
///   1. Registry `wss_qualified_urls` (OVS-ordered, probe-verified)
///   2. Operator-configured `<CHAIN>_WSS_URL`
///   3. Blind scheme substitution (kept for first-boot / no-registry)
///
/// Once the scout sweep populates WSS-qualified records (GAP-34 fix),
/// ingest uses them without any config change. The blind-derivation
/// tail ensures the function always returns *something* on first boot
/// before the first scout sweep completes.
fn wss_candidates(wss: Option<String>, rpcs: &[String]) -> Vec<String> {
    use allbright_core::rpc_registry::RpcRegistry;
    // Determine chain from the first RPC URL by loading the registry
    // and matching; simpler: just read the registry file and let each
    // caller supply the chain directly — but the signature is shared
    // with two call sites. We read the file and extract ALL qualified
    // WSS URLs; callers already filter by chain via the rpc pool, and
    // the registry entries carry chain metadata so we can intersect.
    // Simplest approach: load registry, return wss_qualified_urls
    // for every chain the given rpcs appear in, then deduplicate.
    let path = std::env::var("RPC_REGISTRY_PATH")
        .unwrap_or_else(|_| allbright_core::rpc_registry::REGISTRY_FILE.to_string());
    let reg = RpcRegistry::load(std::path::Path::new(&path));

    let mut out: Vec<String> = Vec::new();

    // Step 1 — registry-qualified WSS (probe-verified eth_subscribe grant).
    // Build a set from the HTTP RPC pool to identify the chain without
    // changing the function signature: registry records whose HTTP URL
    // is in `rpcs` share the same chain as those WSS records.
    let rpcs_set: std::collections::HashSet<&String> = rpcs.iter().collect();
    let matching_chains: std::collections::HashSet<allbright_core::events::Chain> = reg
        .endpoints
        .iter()
        .filter(|e| e.transport != "wss" && e.transport != "ws" && rpcs_set.contains(&e.url))
        .map(|e| e.chain)
        .collect();
    for chain in &matching_chains {
        for u in reg.wss_qualified_urls(*chain) {
            if !out.contains(&u) {
                out.push(u);
            }
        }
    }

    // Step 2 — operator-configured explicit WSS URL.
    if let Some(w) = wss {
        if !w.is_empty() && !out.contains(&w) {
            out.push(w);
        }
    }

    // Step 3 — blind scheme substitution fallback (first-boot / no registry).
    for r in rpcs {
        for derived in [
            r.replacen("https://", "wss://", 1),
            r.replacen("http://", "ws://", 1),
        ] {
            if (derived.starts_with("wss://") || derived.starts_with("ws://"))
                && !out.contains(&derived)
            {
                out.push(derived);
            }
        }
    }
    out
}

/// GAP-3 (AGENTS Appendix B): event-driven pool freshness. Subscribes to
/// Swap/Sync/Mint/Burn logs emitted by the watched pool set; every hit
/// refreshes just that pool via a small Multicall — the solver sees the
/// post-swap state within a block instead of at the next poll tick.
/// Without WSS capability the watcher exits (polling still covers).
async fn swap_watch_loop(
    chain: Chain,
    rpcs: Vec<String>,
    pools: Vec<PoolDescriptor>,
    tx: mpsc::Sender<Event>,
    wss: Option<String>,
    head: std::sync::Arc<std::sync::atomic::AtomicU64>,
) {
    use alloy::primitives::Address;
    use alloy::rpc::types::Filter;
    use futures_util::StreamExt;
    use std::collections::HashMap;

    let by_addr: HashMap<String, PoolDescriptor> = pools
        .iter()
        .map(|p| (p.address.to_lowercase(), p.clone()))
        .collect();
    let addrs: Vec<Address> = pools
        .iter()
        .filter_map(|p| p.address.parse().ok())
        .collect();
    if addrs.is_empty() {
        return;
    }
    let mut ep = 0usize;
    let mut http =
        ProviderBuilder::new().connect_http(rpcs[ep].parse().expect("rpc url"));
    // WSS ladder — configured endpoint first, WS-derivable RPC pool
    // entries after; rotate on failure rather than retrying a
    // WS-incapable endpoint forever.
    let wss_cands = wss_candidates(wss, &rpcs);
    if wss_cands.is_empty() {
        fail(&tx, FailureClass::RpcMethodUnavailable, "gap3.swap.no_wss", chain,
            "no WS-capable endpoint configured or derivable — polling only".into())
            .await;
        return;
    }
    let mut wss_idx = 0usize;
    let sigs = swap_sigs().to_vec();
    let mut backoff = Duration::from_secs(10);
    // Per-watcher verify cache — reuses refresh()'s verification path
    // without sharing state with the poll workers.
    let mut dead: HashSet<String> = HashSet::new();
    let mut verified: HashSet<String> = HashSet::new();
    let mut last_seen: HashMap<String, [u64; 4]> = HashMap::new();

    loop {
        let ws = match ProviderBuilder::new()
            .connect_ws(alloy::providers::WsConnect::new(wss_cands[wss_idx].as_str()))
            .await
        {
            Ok(w) => w,
            Err(e) => {
                let msg = e.to_string();
                let class = if msg.contains("400") || msg.contains("405") || msg.contains("unsupported") {
                    FailureClass::RpcMethodUnavailable
                } else if msg.contains("429") {
                    FailureClass::RpcRateLimit
                } else {
                    FailureClass::RpcProviderFailure
                };
                fail(&tx, class, "gap3.swap.connect", chain, msg).await;
                wss_idx = (wss_idx + 1) % wss_cands.len();
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(300));
                continue;
            }
        };
        // Provider filter limits cap ~hundreds of addresses per
        // subscription — chunk the watched set at 200 and merge the
        // streams over the same WS connection (GAP-3).
        const MAX_FILTER_ADDRS: usize = 200;
        let mut streams: Vec<std::pin::Pin<Box<dyn futures_util::Stream<
            Item = alloy::rpc::types::Log> + Send>>> = Vec::new();
        let mut sub_ok = true;
        for chunk in addrs.chunks(MAX_FILTER_ADDRS) {
            let filter = Filter::new()
                .address(chunk.to_vec())
                .event_signature(sigs.clone());
            match ws.subscribe_logs(&filter).await {
                Ok(s) => streams.push(Box::pin(s.into_stream())),
                Err(e) => {
                    fail(&tx, FailureClass::StreamDrop, "gap3.swap.subscribe", chain,
                        format!("chunk {}: {e}", streams.len())).await;
                    sub_ok = false;
                    break;
                }
            }
        }
        if !sub_ok {
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(Duration::from_secs(300));
            continue;
        }
        let mut stream = futures_util::stream::select_all(streams);
        backoff = Duration::from_secs(10);
        tracing::info!(chain = %chain.name(), pools = addrs.len(),
            subs = (addrs.len() + MAX_FILTER_ADDRS - 1) / MAX_FILTER_ADDRS,
            "swap-event watch live — event-driven freshness");

        while let Some(log) = stream.next().await {
            let key = format!("{:?}", log.address()).to_lowercase();
            let Some(p) = by_addr.get(&key) else { continue };
            // Targeted refresh of just this pool — bounded by event
            // rate; a hot pool self-throttles via block cadence.
            let slice = std::slice::from_ref(p);
            // Pin to the log's own block — post-swap state exactly; fall
            // back to the shared head when the log carries no block.
            let pin = log
                .block_number
                .unwrap_or_else(|| head.load(std::sync::atomic::Ordering::Relaxed));
            if let Err(msg) =
                refresh(&http, slice, &mut dead, &mut verified, &mut last_seen, chain, &tx, pin).await
            {
                if rpcs.len() > 1 {
                    ep = (ep + 1) % rpcs.len();
                    http = ProviderBuilder::new()
                        .connect_http(rpcs[ep].parse().expect("rpc url"));
                }
                fail(&tx, classify_rpc_err(&msg), "gap3.swap.refresh", chain, msg).await;
            }
        }
        fail(&tx, FailureClass::StreamDrop, "gap3.swap.ended", chain,
            "swap log stream closed".into()).await;
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

/// U1 — pre-confirmation trigger (mev-templates pipeline pattern):
/// subscribe `newPendingTransactions` (full bodies, geth ≥1.11) and emit
/// `PendingSwap` whenever a pending tx touches a watched pool — matched
/// on `tx.to` OR on any watched address embedded in the calldata's
/// 32-byte words (router/aggregator paths carry pool addresses).
///
/// Honest coverage: private/sequenced mempools (Arbitrum, Base, OP L2s)
/// expose nothing — the subscribe fails and the loop exits after
/// classifying each WSS candidate once. Polling + GAP-3 log watches
/// still cover those chains.
async fn pending_watch_loop(
    chain: Chain,
    rpcs: Vec<String>,
    pools: Vec<PoolDescriptor>,
    tx: mpsc::Sender<Event>,
    wss: Option<String>,
) {
    use alloy::consensus::Transaction as _;
    use alloy::primitives::Address;
    use futures_util::StreamExt;

    // Watched set: lowercase 0x-prefixed addresses.
    let watched: HashSet<String> = pools
        .iter()
        .map(|p| p.address.to_lowercase())
        .collect();
    if watched.is_empty() {
        return;
    }
    let wss_cands = wss_candidates(wss, &rpcs);
    if wss_cands.is_empty() {
        fail(&tx, FailureClass::RpcMethodUnavailable, "u1.pending.no_wss", chain,
            "no WS-capable endpoint — pending watch off".into())
            .await;
        return;
    }
    let mut wss_idx = 0usize;
    let mut backoff = Duration::from_secs(10);
    let mut tried = 0usize;

    loop {
        let ws = match ProviderBuilder::new()
            .connect_ws(alloy::providers::WsConnect::new(wss_cands[wss_idx].as_str()))
            .await
        {
            Ok(w) => w,
            Err(e) => {
                let msg = e.to_string();
                let class = if msg.contains("400") || msg.contains("405") || msg.contains("unsupported") {
                    FailureClass::RpcMethodUnavailable
                } else if msg.contains("429") {
                    FailureClass::RpcRateLimit
                } else {
                    FailureClass::RpcProviderFailure
                };
                fail(&tx, class, "u1.pending.connect", chain, msg).await;
                tried += 1;
                if tried >= wss_cands.len() {
                    tracing::info!(chain = %chain.name(),
                        "pending watch: no WSS endpoint reachable — off for this chain");
                    return;
                }
                wss_idx = (wss_idx + 1) % wss_cands.len();
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(300));
                continue;
            }
        };
        let mut stream = match ws.subscribe_full_pending_transactions().await {
            Ok(s) => s.into_stream(),
            Err(e) => {
                // Most non-geth endpoints reject full-body pending subs —
                // classify once per endpoint, rotate, then exit cleanly.
                let msg = e.to_string();
                let class = if msg.contains("not supported") || msg.contains("method")
                    || msg.contains("invalid") || msg.contains("-32601")
                {
                    FailureClass::RpcMethodUnavailable
                } else if msg.contains("429") {
                    FailureClass::RpcRateLimit
                } else {
                    FailureClass::RpcProviderFailure
                };
                fail(&tx, class, "u1.pending.subscribe", chain, msg).await;
                tried += 1;
                if tried >= wss_cands.len() {
                    tracing::info!(chain = %chain.name(),
                        "pending watch: no endpoint supports full pending txs — off for this chain");
                    return;
                }
                wss_idx = (wss_idx + 1) % wss_cands.len();
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(300));
                continue;
            }
        };
        tried = 0;
        backoff = Duration::from_secs(10);
        tracing::info!(chain = %chain.name(), pools = watched.len(),
            "pending-tx watch live — pre-confirmation trigger");

        while let Some(tx_env) = stream.next().await {
            // Match 1 — direct call: tx.to is a watched pool.
            let mut hit: Option<String> = tx_env
                .inner
                .to()
                .map(|a| format!("{a:?}").to_lowercase())
                .filter(|k| watched.contains(k));
            // Match 2 — embedded pool address in calldata words
            // (router/aggregator paths carry pool addresses as
            // zero-padded 32-byte words). Scan word-aligned chunks only.
            if hit.is_none() {
                let input = tx_env.inner.input();
                // ABI bodies: 4-byte selector, then 32-byte words — so
                // embedded addresses sit at offset 4 + 32k, not 32k.
                let body = if input.len() > 4 { &input[4..] } else { &input[..] };
                for w in body.chunks_exact(32) {
                    if w[..12].iter().all(|b| *b == 0) {
                        let addr = Address::from_slice(&w[12..]);
                        let k = format!("{addr:?}").to_lowercase();
                        if watched.contains(&k) {
                            hit = Some(k);
                            break;
                        }
                    }
                }
            }
            if let Some(pool) = hit {
                let _ = tx
                    .send(Event::PendingSwap(
                        allbright_core::events::PendingSwap {
                            chain,
                            pool_address: pool,
                            tx_hash: format!("{:?}", tx_env.inner.tx_hash()),
                            ts_ms: now_ms(),
                        },
                    ))
                    .await;
            }
        }
        fail(&tx, FailureClass::StreamDrop, "u1.pending.ended", chain,
            "pending tx stream closed".into())
            .await;
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

/// GAP-12 — assess both pool legs against the token-risk index.
/// `true` only when BOTH tokens come back `Clear`; any `Flagged` or
/// `Unverified` verdict emits a classified `TokenRisk` failure and
/// blocks promotion (fail-closed — unassessable is never safe).
async fn risk_gate(
    risk: &allbright_core::token_risk::TokenRiskClient,
    chain: Chain,
    t0: alloy::primitives::Address,
    t1: alloy::primitives::Address,
    tx: &mpsc::Sender<Event>,
) -> bool {
    use allbright_core::token_risk::RiskGrade;
    let a0 = format!("{t0:?}");
    let a1 = format!("{t1:?}");
    let verdicts = risk
        .check_many(chain.chain_id(), &[a0.clone(), a1.clone()])
        .await;
    let mut clean = true;
    for (addr, v) in verdicts {
        let grade = match v.grade {
            RiskGrade::Clear => continue,
            RiskGrade::Flagged => "flagged",
            RiskGrade::Unverified => "unverified",
        };
        clean = false;
        tracing::warn!(chain = %chain.name(), token = %addr, grade,
            reasons = %v.reason_codes(), "token risk gate blocked promotion");
        fail(
            tx,
            FailureClass::TokenRisk,
            "p06.token_risk",
            chain,
            format!("token {addr} {grade}: {}", v.reason_codes()),
        )
        .await;
    }
    clean
}

/// Verify a discovered pool: token0/token1 must equal the event's
/// tokens and both tokens must answer `decimals()`. One multicall.
async fn verify_discovered<P: Provider>(
    provider: &P,
    pool: alloy::primitives::Address,
    t0: alloy::primitives::Address,
    t1: alloy::primitives::Address,
) -> bool {
    let mc = IMulticall3::new(multicall3_addr(), provider);
    let calls = vec![
        IMulticall3::Call3 { target: pool, allowFailure: true,
            callData: IPairCalls::token0Call {}.abi_encode().into() },
        IMulticall3::Call3 { target: pool, allowFailure: true,
            callData: IPairCalls::token1Call {}.abi_encode().into() },
        IMulticall3::Call3 { target: t0, allowFailure: true,
            callData: IERC20Min::decimalsCall {}.abi_encode().into() },
        IMulticall3::Call3 { target: t1, allowFailure: true,
            callData: IERC20Min::decimalsCall {}.abi_encode().into() },
    ];
    let Ok(res) = mc.aggregate3(calls).call().await else { return false };
    if res.len() != 4 || !res.iter().all(|r| r.success) { return false; }
    let on0 = IPairCalls::token0Call::abi_decode_returns(&res[0].returnData);
    let on1 = IPairCalls::token1Call::abi_decode_returns(&res[1].returnData);
    match (on0, on1) {
        (Ok(a), Ok(b)) => a == t0 && b == t1,
        _ => false,
    }
}

/// One scan cycle: token-verification calls for unverified pools, then a
/// single Multicall3 `aggregate3` for reserves/√P+liquidity of every live pool.
/// `Err(msg)` when the endpoint itself failed — the caller classifies,
/// journals and may fail over; pool-level failures inside a successful
/// batch are handled per-pool.
async fn refresh<P: Provider>(
    provider: &P,
    pools: &[PoolDescriptor],
    dead: &mut HashSet<String>,
    verified: &mut HashSet<String>,
    last_seen: &mut HashMap<String, [u64; 4]>,
    chain: Chain,
    tx: &mpsc::Sender<Event>,
    pin_block: u64,
) -> Result<(), String> {
    {
        let mc = IMulticall3::new(multicall3_addr(), provider);
        // pin_block is the caller-chosen coherence anchor (shared head
        // for polls, the log's own block for swap refreshes). 0 → unpin.
        let block = pin_block;
        let mut unchanged: Vec<String> = Vec::new();
        let live: Vec<&PoolDescriptor> =
            pools.iter().filter(|p| !dead.contains(&p.address)).collect();

        // Build the aggregate3 batch.
        let mut calls: Vec<IMulticall3::Call3> = Vec::with_capacity(live.len() * 4);
        for p in &live {
            let Ok(target) = p.address.parse::<alloy::primitives::Address>() else {
                dead.insert(p.address.clone());
                tracing::warn!(pool = %p.address, "invalid pool address — dropped");
                continue;
            };
            if !verified.contains(&p.address) {
                calls.push(IMulticall3::Call3 {
                    target,
                    allowFailure: true,
                    callData: IPairCalls::token0Call {}.abi_encode().into(),
                });
                calls.push(IMulticall3::Call3 {
                    target,
                    allowFailure: true,
                    callData: IPairCalls::token1Call {}.abi_encode().into(),
                });
            }
            match p.dex {
                DexKind::V3 => {
                    calls.push(IMulticall3::Call3 {
                        target,
                        allowFailure: true,
                        callData: IV3PoolCalls::slot0Call {}.abi_encode().into(),
                    });
                    calls.push(IMulticall3::Call3 {
                        target,
                        allowFailure: true,
                        callData: IV3PoolCalls::liquidityCall {}.abi_encode().into(),
                    });
                }
                _ => {
                    calls.push(IMulticall3::Call3 {
                        target,
                        allowFailure: true,
                        callData: IPairCalls::getReservesCall {}.abi_encode().into(),
                    });
                }
            }
        }

        // Coherence pin (arb.coherence fix): previously the label block
        // was fetched but aggregate3 executed at whatever block the node
        // was on when the call landed — every shard read a different
        // block, all stamped `block`, feeding the solver legs that never
        // coexisted (574k+ routes dropped). Pin the call to the fetched
        // block so all pool states in this scan are one snapshot.
        let call = mc.aggregate3(calls);
        let call = if block > 0 { call.block(BlockId::from(block)) } else { call };
        match call.call().await {
            Ok(results) => {
                let mut idx = 0usize;
                for p in &live {
                    if dead.contains(&p.address) {
                        continue;
                    }
                    // verification pair consumed only while unverified
                    if !verified.contains(&p.address) {
                        let (r0, r1) = (&results[idx], &results[idx + 1]);
                        idx += 2;
                        match verify_tokens(p, r0, r1) {
                            Ok(true) => {
                                verified.insert(p.address.clone());
                                tracing::info!(
                                    chain = %chain.name(), venue = %p.venue,
                                    pool = %p.address, "pool verified"
                                );
                            }
                            Ok(false) | Err(_) => {
                                dead.insert(p.address.clone());
                                tracing::warn!(
                                    chain = %chain.name(), pool = %p.address,
                                    "token0/token1 mismatch — pool dropped"
                                );
                                // Metadata defect: config said one pair,
                                // chain says another — Allbright origin.
                                fail(tx, FailureClass::MetadataMismatch, "pool.verify", chain,
                                     format!("{} tokens don't match descriptor", p.address)).await;
                                continue;
                            }
                        }
                    }
                    let state = match p.dex {
                        DexKind::V3 => {
                            let (s, l) = (&results[idx], &results[idx + 1]);
                            idx += 2;
                            decode_v3(chain, p, s, l, block)
                        }
                        _ => {
                            let r = &results[idx];
                            idx += 1;
                            decode_v2(chain, p, r, block)
                        }
                    };
                    if let Some(state) = state {
                        // Event-flood fix (§31 event_flood diagnosis):
                        // identical-state republishes starved low-rate
                        // subscribers — emit only on a material change.
                        // Unchanged pools join one PoolHeartbeat so
                        // consumers keep honest "verified-at" stamps —
                        // silent suppress would fake staleness and the
                        // solver's coherence filter would kill routes
                        // touching any quiet pool.
                        let fp = [
                            state.reserve0.to_bits(),
                            state.reserve1.to_bits(),
                            state.sqrt_price_x96.unwrap_or(0.0).to_bits(),
                            state.liquidity.unwrap_or(0.0).to_bits(),
                        ];
                        if last_seen.get(&state.address) != Some(&fp) {
                            last_seen.insert(state.address.clone(), fp);
                            let _ = tx.send(Event::PoolUpdate(state)).await;
                        } else {
                            unchanged.push(state.address.clone());
                        }
                    }
                }
            }
            Err(e) => {
                tracing::debug!(chain = %chain.name(), "multicall failed: {e}");
                return Err(e.to_string());
            }
        }
        // One heartbeat per scan carries the "verified unchanged" set at
        // this pin block — consumers refresh staleness stamps without a
        // per-pool re-emit (the flood this replaced).
        if !unchanged.is_empty() {
            let _ = tx
                .send(Event::PoolHeartbeat(allbright_core::events::PoolHeartbeat {
                    chain,
                    block,
                    ts_ms: allbright_core::events::now_ms(),
                    pools: unchanged,
                }))
                .await;
        }
        Ok(())
    }
}

fn verify_tokens(
    p: &PoolDescriptor,
    r0: &IMulticall3::Result,
    r1: &IMulticall3::Result,
) -> anyhow::Result<bool> {
    if !r0.success || !r1.success {
        return Ok(false);
    }
    let t0: alloy::primitives::Address =
        IPairCalls::token0Call::abi_decode_returns(&r0.returnData)?;
    let t1: alloy::primitives::Address =
        IPairCalls::token1Call::abi_decode_returns(&r1.returnData)?;
    let cfg0: alloy::primitives::Address = p.token0.address.parse()?;
    let cfg1: alloy::primitives::Address = p.token1.address.parse()?;
    // Pool may store the pair in either order — accept both.
    Ok((t0 == cfg0 && t1 == cfg1) || (t0 == cfg1 && t1 == cfg0))
}

fn decode_v2(
    chain: Chain,
    p: &PoolDescriptor,
    r: &IMulticall3::Result,
    block: u64,
) -> Option<PoolState> {
    if !r.success {
        return None;
    }
    let res = IPairCalls::getReservesCall::abi_decode_returns(&r.returnData).ok()?;
    let dec0 = 10f64.powi(p.token0.decimals as i32);
    let dec1 = 10f64.powi(p.token1.decimals as i32);
    Some(PoolState {
        chain,
        dex: p.dex,
        venue: p.venue.clone(),
        address: p.address.clone(),
        token0: Token {
            symbol: p.token0.symbol.clone(),
            address: p.token0.address.clone(),
            decimals: p.token0.decimals,
        },
        token1: Token {
            symbol: p.token1.symbol.clone(),
            address: p.token1.address.clone(),
            decimals: p.token1.decimals,
        },
        reserve0: res.reserve0.to::<u128>() as f64 / dec0,
        reserve1: res.reserve1.to::<u128>() as f64 / dec1,
        sqrt_price_x96: None,
        liquidity: None,
        fee_bps: p.fee_bps,
        block,
        ts_ms: now_ms(),
    })
}

fn decode_v3(
    chain: Chain,
    p: &PoolDescriptor,
    s: &IMulticall3::Result,
    l: &IMulticall3::Result,
    block: u64,
) -> Option<PoolState> {
    if !s.success || !l.success {
        return None;
    }
    let slot0 = IV3PoolCalls::slot0Call::abi_decode_returns(&s.returnData).ok()?;
    let liq = IV3PoolCalls::liquidityCall::abi_decode_returns(&l.returnData).ok()?;
    Some(PoolState {
        chain,
        dex: p.dex,
        venue: p.venue.clone(),
        address: p.address.clone(),
        token0: Token {
            symbol: p.token0.symbol.clone(),
            address: p.token0.address.clone(),
            decimals: p.token0.decimals,
        },
        token1: Token {
            symbol: p.token1.symbol.clone(),
            address: p.token1.address.clone(),
            decimals: p.token1.decimals,
        },
        reserve0: 0.0,
        reserve1: 0.0,
        sqrt_price_x96: Some(slot0.sqrtPriceX96.to::<u128>() as f64),
        liquidity: Some(liq as f64),
        fee_bps: p.fee_bps,
        block,
        ts_ms: now_ms(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::primitives::{Address, B256};

    fn taddr(a: Address) -> B256 {
        let mut b = [0u8; 32];
        b[12..].copy_from_slice(a.as_slice());
        B256::from(b)
    }
    fn dword(a: Address) -> Vec<u8> {
        let mut b = vec![0u8; 32];
        b[12..].copy_from_slice(a.as_slice());
        b
    }

    const T0: &str = "0x00000000000000000000000000000000000000a1";
    const T1: &str = "0x00000000000000000000000000000000000000b2";
    const PL: &str = "0x00000000000000000000000000000000000000c3";

    #[test]
    fn decodes_v2_pair_created() {
        let (t0, t1, pl): (Address, Address, Address) =
            (T0.parse().unwrap(), T1.parse().unwrap(), PL.parse().unwrap());
        let mut data = dword(pl);
        data.extend_from_slice(&[0u8; 32]); // pair index
        let topics = [factory_sigs()[0], taddr(t0), taddr(t1)];
        let (a, b, p, k) = decode_pool_creation(&topics, &data).unwrap();
        assert_eq!((a, b, p), (t0, t1, pl));
        assert_eq!(k, DexKind::V2);
    }

    #[test]
    fn decodes_univ3_pool_created() {
        let (t0, t1, pl): (Address, Address, Address) =
            (T0.parse().unwrap(), T1.parse().unwrap(), PL.parse().unwrap());
        let mut data = vec![0u8; 32]; // tickSpacing
        data.extend_from_slice(&dword(pl));
        let topics = [
            factory_sigs()[1],
            taddr(t0),
            taddr(t1),
            taddr(Address::ZERO), // fee (indexed uint24)
        ];
        let (a, b, p, k) = decode_pool_creation(&topics, &data).unwrap();
        assert_eq!((a, b, p), (t0, t1, pl));
        assert_eq!(k, DexKind::V3);
    }

    #[test]
    fn decodes_solidly_pool_created() {
        let (t0, t1, pl): (Address, Address, Address) =
            (T0.parse().unwrap(), T1.parse().unwrap(), PL.parse().unwrap());
        let topics = [
            factory_sigs()[2],
            taddr(t0),
            taddr(t1),
            B256::from([0u8; 32]), // stable=false
        ];
        let (_, _, p, k) = decode_pool_creation(&topics, &dword(pl)).unwrap();
        assert_eq!(p, pl);
        assert_eq!(k, DexKind::Stable);
    }

    #[test]
    fn rejects_unknown_sig_and_short_data() {
        let (t0, t1, pl): (Address, Address, Address) =
            (T0.parse().unwrap(), T1.parse().unwrap(), PL.parse().unwrap());
        assert!(decode_pool_creation(&[B256::ZERO], &dword(pl)).is_none());
        // right sig, truncated data — no pool address, must not panic
        let topics = [factory_sigs()[0], taddr(t0), taddr(t1)];
        assert!(decode_pool_creation(&topics, &[0u8; 10]).is_none());
        assert!(decode_pool_creation(&[], &[]).is_none());
    }
}
