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
    events::{now_ms, Chain, Event, HealAction, SystemEvent},
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
/// Funnel-starvation watch (§34.3): how often the sentinel audits the
/// opportunity funnel for silence while armed.
const FUNNEL_AUDIT_EVERY: Duration = Duration::from_secs(300);
/// No profit-bearing activity for this long triggers diagnosis.
const PROFIT_STALL_WINDOW_MS: u64 = 30 * 60 * 1000;
/// Heartbeat re-emit interval while an RPC stays degraded (≈1 min).
const RPC_DEGRADED_HEARTBEAT: u32 = 6;
/// Healthy endpoints disagreeing by more than this many blocks are
/// divergent — state not trustworthy (§37.8 quorum).
const DIVERGENCE_BLOCKS: u64 = 10;
// ---- Resource & data-plane telemetry (AGENTS §34.0/§34.2): the gaps
// that made this session's faults invisible — sustained CPU/RAM
// saturation, PM2 restart loops, silent-dead feeds, event floods,
// PM2 daemon loss, host saturation. Measured per audit, emitted as
// classified failures, healed where authorized.
/// Sustained per-service CPU above this = saturation (PM2 % of one core;
/// >100 means more than one core).
const CPU_HOT_PCT: f64 = 150.0;
/// Sustained RSS above this = memory pressure (bytes).
const MEM_HOT_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// restart_time growth = PM2 restarted it unprompted (memory cap,
/// crash loop) — autorestart masks the underlying fault.
/// Consecutive hot audits before a resource finding is emitted.
const HOT_TICKS: u32 = 3;
/// Resource audit cadence (PM2 monit + host CPU).
const RESOURCE_AUDIT_EVERY: Duration = Duration::from_secs(30);
/// Data-plane rate audit cadence.
const FEED_AUDIT_EVERY: Duration = Duration::from_secs(30);
/// Zero CexTick for this long = CEX feed blind (silent-dead socket).
const CEX_BLIND_AFTER: Duration = Duration::from_secs(90);
/// Zero PoolUpdate for this long = ingest blind.
const POOL_BLIND_AFTER: Duration = Duration::from_secs(60);
/// Sustained PoolUpdate rate above this = subscriber-starving flood.
const POOL_FLOOD_PER_SEC: f64 = 150.0;
/// Host CPU above this = machine-level saturation.
const HOST_CPU_HOT: f64 = 90.0;

/// Data-plane feed state — counted from the ingest bus.
#[derive(Default)]
struct FeedState {
    pool_count: u64,
    cex_count: u64,
    last_pool: Option<Instant>,
    last_cex: Option<Instant>,
}

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
    // Watch the production scope only — retired chains' endpoints aren't
    // failures, they're out of mandate (ACTIVE_CHAINS).
    cfg.active_chains
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
    cpu: f64,
    mem_bytes: u64,
    restart_time: u64,
}

/// Funnel liveness state — latest observed timestamp per stage plus a
/// rolling rejection-reason window for diagnosis.
#[derive(Default)]
struct FunnelStats {
    last_row_ms: u64,          // any indexed funnel event
    last_opp_ms: u64,
    last_xreq_ms: u64,
    last_exec_ms: u64,
    last_exec_included_ms: u64,
    last_exec_failed_ms: u64,
    last_fail_ms: u64,
    /// (ts, reason-bucket) rejection ring — pruned to the stall window.
    rejections: std::collections::VecDeque<(u64, &'static str)>,
}

/// Read newly appended index rows since `offset`; updates `st` in place.
/// Bounded: only bytes appended since last audit are decoded.
fn funnel_scan(path: &str, offset: &mut u64, st: &mut FunnelStats) {
    let Ok(mut f) = std::fs::File::open(path) else { return };
    use std::io::{Read, Seek, SeekFrom};
    if f.seek(SeekFrom::Start(*offset)).is_err() {
        return;
    }
    let mut buf = String::new();
    if f.read_to_string(&mut buf).is_err() {
        return;
    }
    *offset += buf.len() as u64;
    for line in buf.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let ts = v["ts"].as_u64().unwrap_or(0);
        if ts == 0 {
            continue;
        }
        st.last_row_ms = st.last_row_ms.max(ts);
        match v["t"].as_str().unwrap_or("") {
            "opp" => st.last_opp_ms = st.last_opp_ms.max(ts),
            "xreq" => st.last_xreq_ms = st.last_xreq_ms.max(ts),
            "exec" => {
                st.last_exec_ms = st.last_exec_ms.max(ts);
                match v["status"].as_str().unwrap_or("") {
                    "Included" | "Submitted" => {
                        st.last_exec_included_ms = st.last_exec_included_ms.max(ts)
                    }
                    "Failed" => st.last_exec_failed_ms = st.last_exec_failed_ms.max(ts),
                    _ => {}
                }
            }
            "rej" => {
                let reason = match v["reason"].as_str().unwrap_or("?") {
                    "NoUsdPricing" => "NoUsdPricing",
                    "BelowStrategyThreshold" => "BelowStrategyThreshold",
                    "NegativeEdge" => "NegativeEdge",
                    _ => "other",
                };
                st.rejections.push_back((ts, reason));
            }
            "fail" => st.last_fail_ms = st.last_fail_ms.max(ts),
            _ => {}
        }
    }
}

/// Evidence-pipeline check (§34.3 root-cause discipline): if the index
/// file itself has gone stale while the raw BSONL journal keeps
/// appending, the dashboard's index feed is dead — funnel silence is an
/// observability artifact, not a market signal. Returns true when the
/// index is provably stalled behind a live journal.
fn index_stalled(index_path: &str, journal_dir: &str, now: u64) -> bool {
    let idx_mtime = std::fs::metadata(index_path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    if idx_mtime == 0 || now.saturating_sub(idx_mtime) <= PROFIT_STALL_WINDOW_MS {
        return false; // index fresh — funnel reads are trustworthy
    }
    // Is the raw journal still appending? (proof the funnel is alive but
    // the index feed is not)
    let journal_fresh = std::fs::read_dir(journal_dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| {
                    e.file_name().to_string_lossy().starts_with("journal-")
                        && e.file_name().to_string_lossy().ends_with(".bsonl")
                })
                .filter_map(|e| {
                    e.metadata().and_then(|m| m.modified()).ok()
                })
                .filter_map(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .any(|d| now.saturating_sub(d.as_millis() as u64) <= PROFIT_STALL_WINDOW_MS)
        })
        .unwrap_or(false);
    journal_fresh
}

/// Classify a profit stall into its root funnel stage (§34.3). Returns
/// (class, human detail). `None` = funnel is alive or execution unarmed.
fn diagnose_stall(
    st: &FunnelStats,
    now: u64,
    armed: bool,
    index_stalled: bool,
) -> Option<(FailureClass, String)> {
    if !armed {
        return None; // dormant by policy — not a fault
    }
    let age = |ts: u64| now.saturating_sub(ts);
    let window = PROFIT_STALL_WINDOW_MS;

    // Count rejection reasons inside the window.
    let mut no_pricing = 0u32;
    let mut below_thr = 0u32;
    let mut neg_edge = 0u32;
    let mut other = 0u32;
    for (ts, r) in &st.rejections {
        if now.saturating_sub(*ts) <= window {
            match *r {
                "NoUsdPricing" => no_pricing += 1,
                "BelowStrategyThreshold" => below_thr += 1,
                "NegativeEdge" => neg_edge += 1,
                _ => other += 1,
            }
        }
    }
    let rej_total = no_pricing + below_thr + neg_edge + other;
    let funnel_quiet = age(st.last_row_ms) > window;
    let no_profit = st.last_exec_included_ms == 0 || age(st.last_exec_included_ms) > window;

    if !no_profit {
        return None; // included results inside the window — healthy
    }

    // Stage-by-stage root cause — symptom → component → cause.
    // Rule 0: if the evidence file itself is dead, the stall is in the
    // dashboard index feed, not the funnel — never report this as market
    // absence or pipeline starvation.
    if funnel_quiet && index_stalled {
        return Some((
            FailureClass::StreamDrop,
            "dashboard index stale while raw journal still appends — \
             index feed dead (bus/writer fault), funnel evidence \
             unverifiable — repair observability before diagnosing market"
                .into(),
        ));
    }
    if funnel_quiet && rej_total == 0 {
        return Some((
            FailureClass::FunnelStarvation,
            "entire funnel silent — no opportunities, rejections or execs \
             in window: ingest/solver pipeline not producing (wiring/feed \
             failure, not market absence)".into(),
        ));
    }
    if rej_total > 0 && st.last_opp_ms == 0 && age(st.last_row_ms) <= window {
        // Rejections flowing but nothing qualifies — classify dominant cause.
        if no_pricing * 2 > rej_total {
            return Some((
                FailureClass::PricingBlindness,
                format!("{no_pricing}/{rej_total} rejections = NoUsdPricing — \
                         USD price coverage gap suppressing the universe"),
            ));
        }
        if below_thr + neg_edge == rej_total {
            return Some((
                FailureClass::ProfitBelowThreshold,
                format!("{rej_total} rejections in window, all economic \
                         (below-threshold/negative-edge): market thin under \
                         current liquidity — no market opportunity, not blindness"),
            ));
        }
        return Some((
            FailureClass::FunnelStarvation,
            format!("{rej_total} rejections, 0 opportunities — mixed reasons \
                     (NoUsdPricing={no_pricing} BelowThr={below_thr} NegEdge={neg_edge} other={other})"),
        ));
    }
    if st.last_opp_ms > 0 && st.last_xreq_ms == 0 {
        return Some((
            FailureClass::ConfigurationError,
            format!("opportunities detected (last {}m ago) but ZERO exec \
                     requests — solver→executor decision boundary broken",
                    age(st.last_opp_ms) / 60_000),
        ));
    }
    if st.last_xreq_ms > 0 && st.last_exec_ms == 0 {
        return Some((
            FailureClass::StreamDrop,
            format!("exec requests issued (last {}m ago) but ZERO results — \
                     executor intake path dead", age(st.last_xreq_ms) / 60_000),
        ));
    }
    if st.last_exec_ms > 0 && st.last_exec_included_ms == 0
        && age(st.last_exec_failed_ms) <= window
    {
        return Some((
            FailureClass::BuilderRejected,
            format!("executions reaching the executor but ALL failing \
                     (last failure {}m ago) — relay/provider/gate rejection",
                    age(st.last_exec_failed_ms) / 60_000),
        ));
    }
    Some((
        FailureClass::FunnelStarvation,
        format!("no included execution for {}m — funnel stage undetermined \
                 (last row {}m ago)", window / 60_000, age(st.last_row_ms) / 60_000),
    ))
}

/// Run a pm2 subcommand. On Windows `pm2` resolves to a `.cmd` shim that
/// requires `cmd.exe`; the extensionless bash shim appears first in PATH
/// and is not exec'able by CreateProcess. Bounded retries with growing
/// backoff — under host CPU saturation `pm2` routinely exceeds 15s, and
/// a transient slowness must not read as daemon blindness (the observed
/// "jlist unavailable — remediation suspended" windows were CLI latency,
/// not a dead daemon).
async fn pm2(args: &[&str]) -> Option<std::process::Output> {
    for attempt in 0..4u32 {
        let fut = if cfg!(windows) {
            // Explicit `pm2.cmd` — the extensionless bash shim `pm2` resolves
            // first in PATH and is not exec'able, which silently broke every
            // pm2 call (diagnosis degraded / remediation suspended).
            let mut a = vec!["/c", "pm2.cmd"];
            a.extend_from_slice(args);
            quiet_cmd("cmd").args(&a).output()
        } else {
            quiet_cmd("pm2").args(args).output()
        };
        // Grow the timeout each attempt: 20s → 35s → 50s → 65s. A loaded
        // host still answers; a dead daemon fails fast and we give up.
        let timeout = Duration::from_secs(20 + 15 * attempt as u64);
        match tokio::time::timeout(timeout, fut).await {
            Ok(Ok(o)) if o.status.success() => return Some(o),
            _ if attempt < 3 => {
                tokio::time::sleep(Duration::from_millis(500 * (1 << attempt))).await;
            }
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
                cpu: p["monit"]["cpu"].as_f64().unwrap_or(0.0),
                mem_bytes: p["monit"]["memory"].as_u64().unwrap_or(0),
                restart_time: p["pm2_env"]["restart_time"].as_u64().unwrap_or(0),
            },
        );
    }
    Some(map)
}

/// Host CPU % via typeperf — one sample, ~1s, no admin needed.
async fn host_cpu_pct() -> Option<f64> {
    let out = quiet_cmd("typeperf")
        .args([r"\Processor(_Total)\% Processor Time", "-sc", "1"])
        .output()
        .await
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines()
        .filter(|l| l.contains(','))
        .last()
        .and_then(|l| l.rsplit(',').next()?.trim_matches('"').parse().ok())
}

/// Top N processes by current CPU sample (attribution for a hot host).
/// PowerShell Get-Counter two-sample delta — ~2s, runs only when host
/// is already diagnosed hot.
async fn top_cpu_processes(n: usize) -> String {
    let script = format!(
        "$a=Get-Process;Start-Sleep 1;$b=@{{}};foreach($p in Get-Process){{$b[$p.Id]=$p.TotalProcessorTime.TotalSeconds}};$a|ForEach-Object{{$d=$b[$_.Id]-$_.TotalProcessorTime.TotalSeconds;[pscustomobject]@{{N=$_.Name;C=[int]($d*100)}}}}|Sort-Object C -Descending|Select-Object -First {n}|ForEach-Object{{\"$($_.N):$($_.C)%\"}}"
    );
    let out = quiet_cmd("powershell")
        .args(["-NoProfile", "-Command", &script])
        .output()
        .await
        .ok();
    out.map(|o| String::from_utf8_lossy(&o.stdout).trim().replace('\n', " "))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "attribution unavailable".into())
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
    let _ = bus.publish(&Event::System(SystemEvent {
        component: "sentinel".into(),
        action: "startup".into(),
        detail: "process boot".into(),
        ts_ms: now_ms(),
    }));

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

    // ---- Data-plane rate monitor (§34.0): dead-silent feeds and
    // subscriber-starving floods are both invisible to port probes —
    // this session's two biggest blind spots. The sentinel counts
    // per-variant rates on the ingest bus.
    let feed = std::sync::Arc::new(std::sync::Mutex::new(FeedState::default()));
    {
        let feed = feed.clone();
        let ingest_addr = format!("127.0.0.1:{}", cfg.ports.ingest);
        tokio::spawn(async move {
            loop {
                let mut cli =
                    allbright_core::bus::BusClient::connect_retry(&ingest_addr).await;
                while let Some(ev) = cli.incoming.recv().await {
                    if let Ok(mut f) = feed.lock() {
                        match &ev {
                            Event::PoolUpdate(_) => {
                                f.pool_count += 1;
                                f.last_pool = Some(Instant::now());
                            }
                            Event::CexTick(_) => {
                                f.cex_count += 1;
                                f.last_cex = Some(Instant::now());
                            }
                            _ => {}
                        }
                    }
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        });
    }
    let mut last_resource_audit = Instant::now() - RESOURCE_AUDIT_EVERY;
    let mut last_feed_audit = Instant::now(); // grace period: bus warmup
    let mut hot_ticks: HashMap<String, u32> = HashMap::new();
    let mut hot_emitted: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut prev_restarts: HashMap<String, u64> = HashMap::new();
    let mut loop_warned: HashMap<String, Instant> = HashMap::new();
    let mut feed_prev: (u64, u64) = (0, 0);
    let mut feed_state: Option<&'static str> = None;
    let mut host_hot_ticks = 0u32;
    let mut daemon_fix_at = Instant::now() - BUDGET_WINDOW;

    let mut bad_ticks: HashMap<String, u32> = HashMap::new();
    let mut actions: HashMap<String, Vec<Instant>> = HashMap::new();
    // Post-remediation verification: service → deadline to re-check.
    let mut pending_verify: HashMap<String, Instant> = HashMap::new();
    // Post-escalation cooldown: service → don't re-escalate before this.
    let mut escalated_until: HashMap<String, Instant> = HashMap::new();
    let mut pm2_blind_logged = false;

    // ---- Funnel-starvation watch (§34.3/§34.12): "no profit" is a
    // symptom, not a diagnosis. The sentinel tails the dashboard index
    // (offset-tracked) and, when armed execution yields zero included
    // results for PROFIT_STALL_WINDOW, classifies the root stage:
    //   pipeline silent → FunnelStarvation
    //   rejections dominated by NoUsdPricing → PricingBlindness
    //   requests sent but all failed → BuilderRejected/adapter detail
    //   sub-threshold rejections only → market thin (honest, no heal)
    let index_path =
        std::env::var("DASHBOARD_INDEX").unwrap_or_else(|_| "journal/dashboard-index.jsonl".into());
    let mut index_offset: u64 = std::fs::metadata(&index_path)
        .map(|m| m.len())
        .unwrap_or(0);
    let mut funnel_state: Option<&'static str> = None; // dedupe: emit once per state
    let mut last_funnel_audit = Instant::now() - FUNNEL_AUDIT_EVERY; // audit on first tick
    // Counters over the stall window, refreshed each audit.
    let mut funnel = FunnelStats::default();

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

        // PM2 daemon blind: jlist succeeded but the map is EMPTY — a
        // daemon respawn loses registrations (observed live: daemon died,
        // respawned, whole fleet orphaned then dead). Bounded remediation:
        // resurrect the saved list once per window, escalate on failure.
        if managed.is_empty() && daemon_fix_at.elapsed() >= BUDGET_WINDOW {
            daemon_fix_at = Instant::now();
            tracing::error!("pm2 process list empty — daemon lost registrations; resurrecting");
            let eco = std::env::var("ECOSYSTEM_CONFIG")
                .unwrap_or_else(|_| "ecosystem.config.js".into());
            let ok = pm2(&["resurrect"]).await.is_some()
                || pm2(&["start", &eco]).await.is_some();
            let _ = bus.publish(&Event::Heal(HealAction {
                target: "pm2-daemon".into(),
                action: "pm2.resurrect".into(),
                reason: "pm2 process list empty — daemon registrations lost".into(),
                success: ok,
                restarts_in_window: 0,
                ts_ms: now_ms(),
            }));
            if !ok {
                escalate!(
                    bus, "sentinel.pm2_daemon",
                    "pm2 resurrect/start failed — fleet unmanaged".to_string()
                );
            }
        }

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

        // ---- Funnel-starvation audit (§34.3): "Allbright must not
        // sleep" — when armed execution yields zero included results for
        // the stall window, diagnose the silent stage and classify it.
        // Bounded actions only: the audit diagnoses + escalates; port/RPC
        // remediators above handle what they can fix.
        if last_funnel_audit.elapsed() >= FUNNEL_AUDIT_EVERY {
            last_funnel_audit = Instant::now();
            funnel_scan(&index_path, &mut index_offset, &mut funnel);
            // Prune the rejection ring to the window.
            let now = now_ms();
            while funnel
                .rejections
                .front()
                .map(|(ts, _)| now.saturating_sub(*ts) > PROFIT_STALL_WINDOW_MS * 2)
                .unwrap_or(false)
            {
                funnel.rejections.pop_front();
            }
            let stalled = index_stalled(&index_path, "journal", now);
            match diagnose_stall(&funnel, now, cfg.auto_execute, stalled) {
                Some((class, detail)) => {
                    let key = format!("{class:?}");
                    if funnel_state.as_deref() != Some(key.as_str()) {
                        funnel_state = Some(match class {
                            FailureClass::FunnelStarvation => "FunnelStarvation",
                            FailureClass::PricingBlindness => "PricingBlindness",
                            FailureClass::ProfitBelowThreshold => "ProfitBelowThreshold",
                            FailureClass::ConfigurationError => "ConfigurationError",
                            FailureClass::StreamDrop => "StreamDrop",
                            FailureClass::BuilderRejected => "BuilderRejected",
                            _ => "Other",
                        });
                        tracing::warn!(?class, %detail, "profit stall diagnosed");
                        let _ = bus.publish(&Event::Failure(
                            FailureEvent::new(Component::Sentinel, class, "sentinel.funnel_audit", detail),
                        ));
                    }
                }
                None => {
                    if let Some(prev) = funnel_state.take() {
                        tracing::info!(prev, "funnel recovered — included results resumed");
                        let _ = bus.publish(&Event::Heal(HealAction {
                            target: "funnel".into(),
                            action: "funnel.recovered".into(),
                            reason: "included executions resumed within window".into(),
                            success: true,
                            restarts_in_window: 0,
                            ts_ms: now_ms(),
                        }));
                    }
                }
            }
        }

        // ---- Resource audit (§34.0): PM2 monit telemetry — restart
        // loops, sustained CPU/RAM saturation, host load. Diagnosis and
        // escalation: restarting doesn't fix algorithmic load.
        if last_resource_audit.elapsed() >= RESOURCE_AUDIT_EVERY {
            last_resource_audit = Instant::now();
            for (name, m) in &managed {
                if !m.online {
                    continue;
                }
                // Restart loop: restart_time grows ⇒ PM2 keeps killing
                // and respawning it (memory cap, crash loop) — the
                // autorestart flag masks the underlying fault.
                let prev = prev_restarts
                    .insert(name.clone(), m.restart_time)
                    .unwrap_or(m.restart_time);
                if m.restart_time > prev {
                    let warned = loop_warned
                        .get(name)
                        .map(|t| t.elapsed() < BUDGET_WINDOW)
                        .unwrap_or(false);
                    if !warned {
                        loop_warned.insert(name.clone(), Instant::now());
                        let _ = bus.publish(&Event::Failure(FailureEvent::new(
                            Component::Sentinel,
                            FailureClass::ResourceSaturation,
                            "sentinel.resource",
                            format!(
                                "restart_loop:{name} restarts {prev}→{} — PM2 keep \
                                 killing it (memory cap breach or crash-loop); autorestart \
                                 was masking the fault",
                                m.restart_time
                            ),
                        )));
                    }
                }
                // Sustained CPU/RAM saturation.
                let hot = m.cpu > CPU_HOT_PCT || m.mem_bytes > MEM_HOT_BYTES;
                if hot {
                    let t = hot_ticks.entry(name.clone()).or_default();
                    *t += 1;
                    if *t >= HOT_TICKS && !hot_emitted.contains(name.as_str()) {
                        hot_emitted.insert(name.clone());
                        let _ = bus.publish(&Event::Failure(FailureEvent::new(
                            Component::Sentinel,
                            FailureClass::ResourceSaturation,
                            "sentinel.resource",
                            format!(
                                "resource_saturation:{name} cpu={:.0}% mem={:.0}MB \
                                 sustained {t} audits — needs algorithmic diagnosis, \
                                 not restart",
                                m.cpu,
                                m.mem_bytes as f64 / 1e6
                            ),
                        )));
                    }
                } else {
                    hot_ticks.remove(name);
                    if hot_emitted.remove(name.as_str()) {
                        let _ = bus.publish(&Event::Heal(HealAction {
                            target: name.clone(),
                            action: "resource.recovered".into(),
                            reason: format!(
                                "cpu={:.0}% mem={:.0}MB within budget",
                                m.cpu,
                                m.mem_bytes as f64 / 1e6
                            ),
                            success: true,
                            restarts_in_window: 0,
                            ts_ms: now_ms(),
                        }));
                    }
                }
            }
            // Host-level saturation + process attribution (foreign hogs
            // are escalated, never killed — §34 remediation ordering).
            if let Some(h) = host_cpu_pct().await {
                if h > HOST_CPU_HOT {
                    host_hot_ticks += 1;
                    if host_hot_ticks >= HOT_TICKS && !hot_emitted.contains("host") {
                        hot_emitted.insert("host".into());
                        let top = top_cpu_processes(5).await;
                        let _ = bus.publish(&Event::Failure(FailureEvent::new(
                            Component::Sentinel,
                            FailureClass::ResourceSaturation,
                            "sentinel.resource",
                            format!(
                                "host_saturation:cpu={h:.0}% top=[{top}] — foreign \
                                 processes escalated, never killed"
                            ),
                        )));
                    }
                } else {
                    host_hot_ticks = 0;
                    if hot_emitted.remove("host") {
                        let _ = bus.publish(&Event::Heal(HealAction {
                            target: "host".into(),
                            action: "resource.recovered".into(),
                            reason: format!("host cpu {h:.0}% within budget"),
                            success: true,
                            restarts_in_window: 0,
                            ts_ms: now_ms(),
                        }));
                    }
                }
            }
        }

        // ---- Data-plane feed audit (§34.0): feed silence AND flood —
        // the two faults invisible to every port probe this session.
        if last_feed_audit.elapsed() >= FEED_AUDIT_EVERY {
            last_feed_audit = Instant::now();
            let (pc, cc, lp, lc) = {
                let f = feed.lock().unwrap();
                (f.pool_count, f.cex_count, f.last_pool, f.last_cex)
            };
            let pool_rate =
                (pc - feed_prev.0) as f64 / FEED_AUDIT_EVERY.as_secs_f64();
            feed_prev = (pc, cc);

            let pool_age = lp.map(|t| t.elapsed());
            let cex_age = lc.map(|t| t.elapsed());
            let diagnosed: Option<(&'static str, Component, FailureClass, String)> =
                if pool_age.map(|a| a > POOL_BLIND_AFTER).unwrap_or(false) {
                    Some((
                        "pool_silent",
                        Component::EvmStream,
                        FailureClass::StreamDrop,
                        format!(
                            "data_blindness: zero PoolUpdate for {pool_age:?} — ingest \
                             pool feed silent (collectors dead); market state invisible, \
                             NOT market absence"
                        ),
                    ))
                } else if cex_age.map(|a| a > CEX_BLIND_AFTER).unwrap_or(false) {
                    Some((
                        "cex_silent",
                        Component::CexStream,
                        FailureClass::StreamDrop,
                        format!(
                            "data_blindness: zero CexTick for {cex_age:?} — Binance feed \
                             silent (dead-socket symptom); native pricing on fallback → \
                             gas economics distorted"
                        ),
                    ))
                } else if pool_rate > POOL_FLOOD_PER_SEC {
                    Some((
                        "pool_flood",
                        Component::Ingest,
                        FailureClass::ResourceSaturation,
                        format!(
                            "event_flood: PoolUpdate {pool_rate:.0}/s starving \
                             subscribers — low-rate events (CexTick) queue behind \
                             identical-state republishes"
                        ),
                    ))
                } else {
                    None
                };

            match diagnosed {
                Some((key, comp, class, detail)) if feed_state != Some(key) => {
                    feed_state = Some(key);
                    tracing::warn!(%detail, "data-plane fault diagnosed");
                    let _ = bus.publish(&Event::Failure(FailureEvent::new(
                        comp, class, "sentinel.feed_audit", detail.clone(),
                    )));
                    // Bounded remediation: silence means the collector
                    // socket is wedged — restarting ingest re-establishes
                    // it (H2 authority; budget-shared with the port
                    // remediator). Flood is NOT restartable — it needs
                    // the ingest dedup/algorithmic fix.
                    if key != "pool_flood" {
                        let svc = "allbright-ingest".to_string();
                        let hist = actions.entry(svc.clone()).or_default();
                        hist.retain(|t| t.elapsed() < BUDGET_WINDOW);
                        if (hist.len() as u32) < RESTART_BUDGET {
                            hist.push(Instant::now());
                            let ok = pm2_restart(&svc).await;
                            pending_verify.insert(
                                svc.clone(),
                                Instant::now() + VERIFY_DELAY,
                            );
                            heal!(
                                bus, &svc,
                                "pm2.restart(feed_silent)".to_string(),
                                detail, ok, actions
                            );
                        } else {
                            escalate!(
                                bus, "sentinel.feed_audit",
                                format!("ingest feed-silent remediation budget exhausted ({detail})")
                            );
                        }
                    }
                }
                None => {
                    if feed_state.take().is_some() {
                        let _ = bus.publish(&Event::Heal(HealAction {
                            target: "feed".into(),
                            action: "feed.recovered".into(),
                            reason: format!("pool {pool_rate:.0}/s, cex flowing"),
                            success: true,
                            restarts_in_window: 0,
                            ts_ms: now_ms(),
                        }));
                    }
                }
                _ => {}
            }
        }

        tokio::time::sleep(TICK).await;
    }
}
