//! Allbright Sentinel — self-diagnosis & bounded remediation (AGENTS §34).
//!
//! Observe → Diagnose → Remediate → Verify → Escalate.
//!
//! Bounded scope by design: the sentinel may restart a dead service via
//! PM2 and kill an orphan process squatting on a managed service's port.
//! It must never lower thresholds, relax gates, mutate config, admit
//! quarantined pools, or retry forever — a service that exceeds the
//! restart budget escalates as a classified failure instead of churning.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use allbright_core::{
    bus::BusServer,
    config::Config,
    events::{now_ms, Chain, Event, HealAction},
    lineage::{Component, FailureClass, FailureEvent},
    rpc_registry::{RpcRegistry, REGISTRY_FILE},
};
use futures_util::stream::{self, StreamExt};
use tracing::info;

/// Dead-state confirmations required before remediation.
const CONFIRM_TICKS: u32 = 2;
/// Max remediation actions per service per 10-minute window (then escalate).
const RESTART_BUDGET: u32 = 3;
const BUDGET_WINDOW: Duration = Duration::from_secs(600);
const TICK: Duration = Duration::from_secs(10);
/// Time allowed for a restarted service to bind its port.
const VERIFY_DELAY: Duration = Duration::from_secs(12);
/// Head not advancing for this long = stale-state blindness.
const RPC_STALE_AFTER: Duration = Duration::from_secs(120);
/// Heartbeat re-emit interval while an RPC stays degraded (≈1 min).
const RPC_DEGRADED_HEARTBEAT: u32 = 6;
/// Healthy endpoints disagreeing by more than this many blocks are
/// divergent — state not trustworthy (§37.8 quorum).
const DIVERGENCE_BLOCKS: u64 = 10;

/// Per-endpoint RPC visibility state.
#[derive(Default)]
struct RpcState {
    last_block: Option<u64>,
    last_advance: Option<Instant>,
    degraded: bool,
    ticks_degraded: u32,
}

/// `eth_blockNumber` probe — cheapest liveness+freshness check.
/// Returns (block, rtt); `Err(class)` carries the blindness class.
async fn rpc_block_number(
    http: &reqwest::Client,
    url: &str,
) -> Result<(u64, Duration), FailureClass> {
    let body = serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "eth_blockNumber", "params": []
    });
    let t0 = Instant::now();
    let resp = http
        .post(url)
        .json(&body)
        .timeout(Duration::from_secs(6))
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                FailureClass::RpcTimeout
            } else {
                FailureClass::RpcProviderFailure
            }
        })?;
    let rtt = t0.elapsed();
    if resp.status().as_u16() == 429 {
        return Err(FailureClass::RpcRateLimit);
    }
    if !resp.status().is_success() {
        return Err(FailureClass::RpcProviderFailure);
    }
    let v: serde_json::Value = resp
        .json()
        .await
        .map_err(|_| FailureClass::RpcProviderFailure)?;
    let block = v["result"]
        .as_str()
        .and_then(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).ok())
        .ok_or(FailureClass::RpcMethodUnavailable)?;
    Ok((block, rtt))
}

/// Watch list: env primaries ∪ registry-QUALIFIED backups ordered by
/// OVS, bounded per chain — the sentinel watches the production
/// shortlist, not the whole candidate universe (§36/§37).
fn watch_endpoints(cfg: &Config, reg: &RpcRegistry) -> Vec<(Chain, Vec<String>)> {
    Chain::ALL
        .iter()
        .filter_map(|c| {
            let mut v: Vec<String> = cfg.rpc_pools.get(c).cloned().unwrap_or_default();
            for u in reg.qualified_urls(*c) {
                if v.len() >= 10 {
                    break;
                }
                if !v.contains(&u) {
                    v.push(u);
                }
            }
            (!v.is_empty()).then_some((*c, v))
        })
        .collect()
}

/// Host part of a URL — endpoints may carry keys; never log them.
fn host_of(url: &str) -> &str {
    url.split("://")
        .nth(1)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or(url)
}

/// Spawn a subprocess with no console window (Windows): service probes
/// run every tick — without CREATE_NO_WINDOW each spawn flashes a
/// console on the operator's desktop.
fn quiet_cmd(prog: &str) -> tokio::process::Command {
    let mut c = tokio::process::Command::new(prog);
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    c
}

/// PID currently holding `port` in LISTENING state, if any.
async fn port_holder_pid(port: u16) -> Option<u32> {
    let out = quiet_cmd("netstat")
        .args(["-ano", "-p", "tcp"])
        .output()
        .await
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let suffix = format!(":{port}");
    for line in text.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        // TCP  127.0.0.1:3103  0.0.0.0:0  LISTENING  24844
        if f.len() >= 5 && f[0] == "TCP" && f[1].ends_with(&suffix) && f[3] == "LISTENING" {
            if let Ok(pid) = f[4].parse() {
                return Some(pid);
            }
        }
    }
    None
}

#[derive(Debug, Default)]
struct ManagedProc {
    pid: u32,
    online: bool,
}

/// Run a pm2 subcommand. On Windows `pm2` resolves to a `.cmd` shim that
/// requires `cmd.exe`; the extensionless bash shim appears first in PATH
/// and is not exec'able by CreateProcess. Bounded by a timeout with one
/// retry — the node CLI can be slow under host CPU pressure, and a
/// transient failure must not be read as daemon blindness.
async fn pm2(args: &[&str]) -> Option<std::process::Output> {
    for attempt in 0..2 {
        let fut = if cfg!(windows) {
            let mut a = vec!["/c", "pm2"];
            a.extend_from_slice(args);
            quiet_cmd("cmd").args(&a).output()
        } else {
            quiet_cmd("pm2").args(args).output()
        };
        match tokio::time::timeout(Duration::from_secs(15), fut).await {
            Ok(Ok(o)) if o.status.success() => return Some(o),
            _ if attempt == 0 => tokio::time::sleep(Duration::from_millis(500)).await,
            _ => {}
        }
    }
    None
}

/// PM2-managed process states, keyed by service name.
/// `None` = observation source failed — callers must NOT remediate on
/// missing data (fail-closed diagnosis, AGENTS §34).
async fn pm2_list() -> Option<HashMap<String, ManagedProc>> {
    let out = pm2(&["jlist"]).await?;
    // PM2 may prefix warnings; strip to the JSON array.
    let text = String::from_utf8_lossy(&out.stdout);
    let start = text.find('[')?;
    let end = text.rfind(']')? + 1;
    let procs: serde_json::Value =
        serde_json::from_str(text.get(start..end)?).ok()?;
    let mut map = HashMap::new();
    for p in procs.as_array().into_iter().flatten() {
        let name = p["name"].as_str().unwrap_or_default().to_string();
        map.insert(
            name,
            ManagedProc {
                pid: p["pid"].as_u64().unwrap_or(0) as u32,
                online: p["pm2_env"]["status"].as_str() == Some("online"),
            },
        );
    }
    Some(map)
}

/// Image name of a process (e.g. "allbright-solver.exe"), if alive.
async fn process_image(pid: u32) -> Option<String> {
    let out = quiet_cmd("tasklist")
        .args([
            "/FI",
            &format!("PID eq {pid}"),
            "/NH",
            "/FO",
            "CSV",
        ])
        .output()
        .await
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().next()?;
    let name = line.trim_matches('"').split('"').next()?;
    (!name.is_empty() && !name.starts_with("INFO")).then(|| name.to_string())
}

async fn kill_pid(pid: u32) -> bool {
    let args = ["/F", "/PID", &pid.to_string()];
    let out = if cfg!(windows) {
        let mut a = vec!["/c", "taskkill"];
        a.extend(args.iter());
        quiet_cmd("cmd").args(&a).output().await
    } else {
        quiet_cmd("kill")
            .args(["-9", &pid.to_string()])
            .output()
            .await
    };
    out.map(|o| o.status.success()).unwrap_or(false)
}

async fn pm2_restart(name: &str) -> bool {
    pm2(&["restart", name]).await.is_some()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("allbright=info".parse().unwrap()),
        )
        .init();

    let cfg = Config::load()?;
    info!(config = ?cfg.redacted_summary(), "allbright-sentinel starting");

    let addr: SocketAddr = ([127, 0, 0, 1], cfg.ports.sentinel).into();
    let bus = BusServer::bind(addr).await?;
    info!(%addr, "sentinel bus listening");

    // Sentinel does not watch itself — a dead sentinel is caught by PM2's
    // own autorestart; a wedged one would starve its own loop anyway.
    let watched: Vec<(String, u16)> = vec![
        ("allbright-ingest".into(), cfg.ports.ingest),
        ("allbright-simulation".into(), cfg.ports.simulation),
        ("allbright-solver".into(), cfg.ports.solver),
        ("allbright-executor".into(), cfg.ports.executor),
        ("allbright-research".into(), cfg.ports.research),
        ("allbright-observatory".into(), cfg.ports.observatory),
        ("allbright-challenger".into(), cfg.ports.challenger),
        ("allbright-dashboard".into(), cfg.ports.dashboard),
    ];

    let mut bad_ticks: HashMap<String, u32> = HashMap::new();
    let mut actions: HashMap<String, Vec<Instant>> = HashMap::new();
    // Post-remediation verification: service → deadline to re-check.
    let mut pending_verify: HashMap<String, Instant> = HashMap::new();
    // Post-escalation cooldown: service → don't re-escalate before this.
    let mut escalated_until: HashMap<String, Instant> = HashMap::new();
    let mut pm2_blind_logged = false;

    // ---- RPC visibility watch (AGENTS §32/§34): the sentinel diagnoses
    // data-layer blindness so it is never silently read as "no market
    // opportunity" (§34.12).
    let http = reqwest::Client::new();
    let mut rpc_state: HashMap<String, RpcState> = HashMap::new();

    // RPC registry (scout-owned, §36): reloaded when the file changes so
    // the watch list tracks the evolving qualified pool.
    let reg_path =
        std::env::var("RPC_REGISTRY_PATH").unwrap_or_else(|_| REGISTRY_FILE.to_string());
    let mut reg_mtime: Option<std::time::SystemTime> = None;
    let mut registry = RpcRegistry::default();
    let mut rpc_chains: Vec<(Chain, Vec<String>)> = watch_endpoints(&cfg, &registry);

    // Wiring-gap audit (§35): a chain with a single endpoint is one
    // failure away from invisibility. Report it as a classified
    // finding, once, instead of pretending coverage exists.
    for (chain, pool) in &rpc_chains {
        if pool.len() < 2 {
            let _ = bus.publish(&Event::Failure(
                FailureEvent::new(
                    Component::EvmStream,
                    FailureClass::RpcBlindness,
                    "wiring.audit",
                    format!(
                        "{} single-homed: 1 RPC endpoint configured, no backup — \
                         visibility not resilient (design target: multi-RPC per chain)",
                        chain.name()
                    ),
                )
                .on_chain(*chain),
            ));
        }
    }
    info!(
        endpoints = rpc_chains.iter().map(|(_, p)| p.len()).sum::<usize>(),
        chains = rpc_chains.len(),
        "RPC visibility watch armed"
    );

    macro_rules! heal {
        ($bus:expr, $name:expr, $action:expr, $reason:expr, $ok:expr, $actions:expr) => {{
            let n = $actions.get($name).map(|v| v.len() as u32).unwrap_or(0);
            let _ = $bus.publish(&Event::Heal(HealAction {
                target: $name.clone(),
                action: $action.into(),
                reason: $reason,
                success: $ok,
                restarts_in_window: n,
                ts_ms: now_ms(),
            }));
        }};
    }
    macro_rules! escalate {
        ($bus:expr, $ctx:expr, $detail:expr) => {{
            let _ = $bus.publish(&Event::Failure(FailureEvent::new(
                Component::Sentinel,
                FailureClass::OrphanedProcess,
                $ctx,
                $detail,
            )));
        }};
    }

    loop {
        // Fail-closed: if the PM2 observation source is unavailable the
        // sentinel cannot distinguish healthy from orphaned processes —
        // escalate the degraded diagnosis instead of remediating blind.
        let Some(managed) = pm2_list().await else {
            if !pm2_blind_logged {
                tracing::error!("pm2 jlist unavailable — diagnosis degraded, remediation suspended");
                escalate!(
                    bus, "sentinel.observe",
                    "pm2 jlist unavailable — remediation suspended (blind healer)".to_string()
                );
                pm2_blind_logged = true;
            }
            tokio::time::sleep(TICK).await;
            continue;
        };
        pm2_blind_logged = false;

        for (name, port) in &watched {
            // 1. Verify a pending remediation first.
            if let Some(deadline) = pending_verify.get(name).copied() {
                if Instant::now() < deadline {
                    continue; // remediation in flight — don't stack another
                }
                pending_verify.remove(name);
                let healthy = match (managed.get(name), port_holder_pid(*port).await) {
                    (Some(m), Some(holder)) => {
                        m.online
                            && (holder == m.pid
                                || process_image(holder).await
                                    .map(|img| img == format!("{name}.exe"))
                                    .unwrap_or(false))
                    }
                    _ => false,
                };
                heal!(
                    bus, name, "pm2.restart".to_string(),
                    format!("post-heal verify on :{port}"), healthy, actions
                );
                if healthy {
                    info!(service = %name, "heal verified — service restored");
                    bad_ticks.remove(name);
                } else {
                    tracing::error!(service = %name, "heal FAILED — service still unhealthy");
                    escalate!(
                        bus, "sentinel.heal.verify",
                        format!("{name} still unhealthy after remediation")
                    );
                }
                continue;
            }

            // 2. Diagnose: PM2 state vs actual port holder.
            let (m_pid, m_online) = managed
                .get(name)
                .map(|p| (p.pid, p.online))
                .unwrap_or((0, false));
            let holder = port_holder_pid(*port).await;
            let expected_exe = format!("{name}.exe");

            let diagnosis: Option<String> = match (m_online, holder) {
                (true, Some(h)) if h == m_pid => {
                    bad_ticks.remove(name);
                    continue; // healthy
                }
                (true, Some(h)) => {
                    // Holder ≠ managed pid could be a wrapper child —
                    // only call it an orphan if it isn't our binary.
                    match process_image(h).await {
                        Some(img) if img == expected_exe => {
                            Some(format!("pid_drift:holder_pid{h}_managed_pid{m_pid}"))
                        }
                        Some(img) => Some(format!(
                            "orphan_squatter:pid{h}({img})_managed_pid{m_pid}"
                        )),
                        None => Some("port_dead:stale_listener".into()),
                    }
                }
                (true, None) => Some("port_dead:process_online_but_not_listening".into()),
                (false, Some(h)) => match process_image(h).await {
                    // Managed dead + port held by our binary = classic orphan.
                    Some(img) if img == expected_exe => {
                        Some(format!("orphan_squatter:pid{h}_managed_dead"))
                    }
                    Some(img) => Some(format!("foreign_squatter:pid{h}({img})")),
                    None => Some("process_down:port_free".into()),
                },
                (false, None) => Some("process_down:port_free".into()),
            };
            let Some(reason) = diagnosis else { continue };

            // Escalation cooldown — don't re-escalate every tick.
            if escalated_until
                .get(name)
                .map(|t| Instant::now() < *t)
                .unwrap_or(false)
            {
                continue;
            }

            let ticks = bad_ticks.entry(name.clone()).or_default();
            *ticks += 1;
            if *ticks < CONFIRM_TICKS {
                continue;
            }

            // 3. Budget check — escalate instead of churning, then cool down.
            let hist = actions.entry(name.clone()).or_default();
            hist.retain(|t| t.elapsed() < BUDGET_WINDOW);
            if hist.len() as u32 >= RESTART_BUDGET {
                escalate!(
                    bus, "sentinel.budget_exhausted",
                    format!("{name} remediation budget exhausted ({reason}) — escalating")
                );
                tracing::error!(service = %name, %reason, "budget exhausted — escalating");
                bad_ticks.remove(name);
                escalated_until.insert(name.clone(), Instant::now() + BUDGET_WINDOW);
                continue;
            }
            hist.push(Instant::now());

            // 4. Remediate: kill OUR orphan binary if it holds the port,
            // never a foreign process — foreign squatters escalate.
            tracing::warn!(service = %name, port, %reason, "unhealthy — remediating");
            let mut action = String::from("pm2.restart");
            if let Some(h) = holder {
                if h != m_pid {
                    match process_image(h).await {
                        Some(img) if img == expected_exe => {
                            action = format!("taskkill.orphan(pid{h})+pm2.restart");
                            if !kill_pid(h).await {
                                escalate!(
                                    bus, "sentinel.taskkill",
                                    format!("failed to kill orphan pid {h} holding :{port} for {name}")
                                );
                            }
                        }
                        Some(img) => {
                            // Foreign process on our port — not ours to kill.
                            escalate!(
                                bus, "sentinel.foreign_squatter",
                                format!("{img} pid {h} holds :{port} for {name} — escalating")
                            );
                            escalated_until
                                .insert(name.clone(), Instant::now() + BUDGET_WINDOW);
                            continue;
                        }
                        None => {} // holder exited — port likely free now
                    }
                }
            }
            if pm2_restart(name).await {
                pending_verify.insert(name.clone(), Instant::now() + VERIFY_DELAY);
                heal!(bus, name, action, reason.clone(), true, actions);
            } else {
                heal!(bus, name, action, reason.clone(), false, actions);
                escalate!(
                    bus, "sentinel.pm2",
                    format!("pm2 restart {name} failed ({reason})")
                );
            }
        }

        // ---- RPC registry hot-reload (scout rewrites it each sweep) ----
        if let Ok(meta) = std::fs::metadata(&reg_path) {
            let mt = meta.modified().ok();
            if mt.is_some() && mt != reg_mtime {
                registry = RpcRegistry::load(std::path::Path::new(&reg_path));
                reg_mtime = mt;
                let s = registry.summary();
                info!(qualified = s.qualified, total = s.total,
                      "RPC registry reloaded");
            }
            rpc_chains = watch_endpoints(&cfg, &registry);
        }

        // ---- RPC visibility probes (§32/§34/§37). Probed CONCURRENTLY —
        // a 100-endpoint watch list can't be serialized inside a 10s
        // tick. A chain is blind only when EVERY watched endpoint is
        // unhealthy; ingest failover is the remediation, the sentinel
        // diagnoses, classifies and verifies recovery.
        let mut probe_futs = Vec::new();
        for (chain, pool) in &rpc_chains {
            for url in pool {
                let http = http.clone(); // reqwest Client is Arc — cheap clone
                let url = url.clone();
                probe_futs.push(async move {
                    (*chain, url.clone(), rpc_block_number(&http, &url).await)
                });
            }
        }
        let probe_results: Vec<(Chain, String, Result<(u64, Duration), FailureClass>)> =
            stream::iter(probe_futs).buffer_unordered(24).collect().await;
        let mut by_chain: HashMap<Chain, Vec<(String, Result<(u64, Duration), FailureClass>)>> =
            HashMap::new();
        for (c, u, r) in probe_results {
            by_chain.entry(c).or_default().push((u, r));
        }

        for (chain, pool) in &rpc_chains {
            let mut healthy_eps = 0usize;
            let mut heads: Vec<u64> = Vec::new();
            for (url, res) in by_chain.remove(chain).unwrap_or_default() {
                let key = format!("{}:{}", chain.name(), host_of(&url));
                let st = rpc_state.entry(key.clone()).or_default();
                match res {
                    Ok((block, rtt)) => {
                        if st.degraded {
                            st.degraded = false;
                            st.ticks_degraded = 0;
                            let _ = bus.publish(&Event::Heal(HealAction {
                                target: format!("rpc:{key}"),
                                action: "rpc.recovered".into(),
                                reason: format!("endpoint responsive again (rtt {rtt:?})"),
                                success: true,
                                restarts_in_window: 0,
                                ts_ms: now_ms(),
                            }));
                            info!(endpoint = %key, "RPC endpoint restored");
                        }
                        let advanced =
                            st.last_block.map(|b| block > b).unwrap_or(true);
                        if advanced {
                            st.last_block = Some(block);
                            st.last_advance = Some(Instant::now());
                        } else {
                            let stale_for = st
                                .last_advance
                                .map(|t| t.elapsed())
                                .unwrap_or_default();
                            if stale_for > RPC_STALE_AFTER {
                                st.degraded = true;
                                let _ = bus.publish(&Event::Failure(
                                    FailureEvent::new(
                                        Component::EvmStream,
                                        FailureClass::RpcStaleMarketState,
                                        "rpc.probe",
                                        format!("{key} head #{block} unchanged {stale_for:?}"),
                                    )
                                    .on_chain(*chain),
                                ));
                                continue;
                            }
                        }
                        if rtt > Duration::from_secs(3) && !st.degraded {
                            st.degraded = true;
                            let _ = bus.publish(&Event::Failure(
                                FailureEvent::new(
                                    Component::EvmStream,
                                    FailureClass::RpcLatencyExpired,
                                    "rpc.probe",
                                    format!("{key} rtt {rtt:?} exceeds 3s budget"),
                                )
                                .on_chain(*chain),
                            ));
                            continue;
                        }
                        healthy_eps += 1;
                        heads.push(block);
                    }
                    Err(class) => {
                        if !st.degraded {
                            st.degraded = true;
                            st.ticks_degraded = 0;
                            let _ = bus.publish(&Event::Failure(
                                FailureEvent::new(
                                    Component::EvmStream,
                                    class,
                                    "rpc.probe",
                                    format!("{key} unhealthy ({class:?})"),
                                )
                                .on_chain(*chain),
                            ));
                            tracing::warn!(endpoint = %key, ?class, "RPC endpoint degraded");
                        }
                        st.ticks_degraded += 1;
                    }
                }
            }

            // Provider-divergence check (§37.8): healthy endpoints that
            // disagree materially on the head are not interchangeable —
            // one of them is seeing a different chain state.
            if heads.len() >= 2 {
                let spread = heads.iter().max().unwrap() - heads.iter().min().unwrap();
                let dk = format!("{}:__divergence__", chain.name());
                let dst = rpc_state.entry(dk).or_default();
                if spread > DIVERGENCE_BLOCKS && !dst.degraded {
                    dst.degraded = true;
                    let _ = bus.publish(&Event::Failure(
                        FailureEvent::new(
                            Component::EvmStream,
                            FailureClass::RpcProviderDivergence,
                            "rpc.probe",
                            format!(
                                "{} head spread {spread} blocks across {} healthy endpoints — \
                                 providers disagree, state not trustworthy",
                                chain.name(),
                                heads.len()
                            ),
                        )
                        .on_chain(*chain),
                    ));
                } else if spread <= DIVERGENCE_BLOCKS && dst.degraded {
                    dst.degraded = false;
                    let _ = bus.publish(&Event::Heal(HealAction {
                        target: format!("rpc:{}:divergence", chain.name()),
                        action: "rpc.divergence_resolved".into(),
                        reason: format!("head spread {spread} blocks within tolerance"),
                        success: true,
                        restarts_in_window: 0,
                        ts_ms: now_ms(),
                    }));
                }
            }

            // Chain-level verdict: all endpoints unhealthy = chain blind.
            let ck = format!("{}:__chain__", chain.name());
            let cst = rpc_state.entry(ck.clone()).or_default();
            if healthy_eps == 0 && !pool.is_empty() {
                let newly_blind = !cst.degraded;
                cst.degraded = true;
                cst.ticks_degraded += 1;
                if newly_blind || cst.ticks_degraded % RPC_DEGRADED_HEARTBEAT == 0 {
                    let _ = bus.publish(&Event::Failure(
                        FailureEvent::new(
                            Component::EvmStream,
                            FailureClass::RpcChainVisibilityFailure,
                            "rpc.probe",
                            format!(
                                "{} all {} endpoints unhealthy — chain visibility DEGRADED \
                                 (not market absence); ingest failover{}",
                                chain.name(),
                                pool.len(),
                                if pool.len() > 1 { " active" } else { " impossible — single-homed" }
                            ),
                        )
                        .on_chain(*chain),
                    ));
                }
            } else if healthy_eps > 0 && cst.degraded {
                cst.degraded = false;
                cst.ticks_degraded = 0;
                let _ = bus.publish(&Event::Heal(HealAction {
                    target: format!("rpc:{}", chain.name()),
                    action: "rpc.chain_recovered".into(),
                    reason: format!("{healthy_eps}/{} endpoints healthy", pool.len()),
                    success: true,
                    restarts_in_window: 0,
                    ts_ms: now_ms(),
                }));
                info!(chain = chain.name(), healthy = healthy_eps, "chain visibility restored");
            }
        }

        tokio::time::sleep(TICK).await;
    }
}
