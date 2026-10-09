//! allbright-dashboard — Commander-facing operational surface (:3001).
//!
//! Serves the six-page SPA (Profit → Strategy → Deployment →
//! Infrastructure → Reports → Wallet, AGENTS §38), a REST API over the
//! dashboard's own bounded event index, and the live telemetry WS.
//! Observability layer only — it is never the execution authority.

mod api;
mod state;

use allbright_core::{
    config::Config,
    events::{Event, ResearchStats, TelemetryFrame},
};
use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    http::header,
    response::{Html, IntoResponse},
    routing::{get, post},
    Router,
};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;
use tracing::info;

use api::Shared;
use state::EventIndex;

#[derive(Default, Clone, serde::Serialize)]
struct View {
    solver: TelemetryFrame,
    executor: TelemetryFrame,
    stats: Option<ResearchStats>,
    last_opportunity: Option<serde_json::Value>,
    last_exec: Option<serde_json::Value>,
    /// Recent classified failures (component:class:origin) — newest first.
    failures: std::collections::VecDeque<serde_json::Value>,
    /// IOO: latest independently-observed spread + universe stats.
    last_observed: Option<serde_json::Value>,
    universe: Option<serde_json::Value>,
    /// Latest challenger verdict.
    last_challenge: Option<serde_json::Value>,
}

const INDEX_HTML: &str = include_str!("../static/index.html");
const LOGO_SVG: &str = include_str!("../static/logo.svg");

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
    info!(config = ?cfg.redacted_summary(), "allbright-dashboard starting");

    let (tx, _) = broadcast::channel::<View>(256);
    let view = Arc::new(tokio::sync::RwLock::new(View::default()));

    // Persistent event index — shared with the API layer. Fed ONLY from
    // the ingest bus: ingest fans in every sibling service and republishes
    // journaled events, so indexing here captures solver/executor/
    // observatory/challenger/sentinel/scout exactly once (no double count).
    let app: Shared = Arc::new(api::AppState {
        idx: Mutex::new(EventIndex::new()),
        cfg: cfg.clone(),
        started_ms: allbright_core::events::now_ms(),
    });

    // Fan-in: solver funnel + opportunities.
    spawn_feed(
        cfg.ports.solver,
        |ev, v: &mut View| match ev {
            Event::Telemetry(f) => v.solver = f.clone(),
            Event::Opportunity(o) => {
                v.last_opportunity = serde_json::to_value(&o).ok();
            }
            _ => {}
        },
        view.clone(),
        tx.clone(),
    );

    // Fan-in: executor results + funnel.
    spawn_feed(
        cfg.ports.executor,
        |ev, v: &mut View| match ev {
            Event::Telemetry(f) => v.executor = f.clone(),
            Event::ExecResult(r) => {
                v.last_exec = serde_json::to_value(&r).ok();
            }
            Event::Failure(f) => {
                v.failures
                    .push_front(serde_json::to_value(&f).unwrap_or_default());
                v.failures.truncate(50);
            }
            _ => {}
        },
        view.clone(),
        tx.clone(),
    );

    // Fan-in: research stats.
    spawn_feed(
        cfg.ports.research,
        |ev, v: &mut View| {
            if let Event::Stats(s) = ev {
                v.stats = Some(s.clone());
            }
        },
        view.clone(),
        tx.clone(),
    );

    // Fan-in: ingest bus — the durable event index feed (all journaled
    // governance events republished here) + failure tail for the view.
    {
        let idx = app.clone();
        spawn_feed(
            cfg.ports.ingest,
            move |ev, v: &mut View| {
                idx.idx.lock().unwrap().push(&ev);
                if let Event::Failure(f) = ev {
                    v.failures
                        .push_front(serde_json::to_value(&f).unwrap_or_default());
                    v.failures.truncate(50);
                }
            },
            view.clone(),
            tx.clone(),
        );
    }

    // Fan-in: observatory — independent opportunity universe.
    spawn_feed(
        cfg.ports.observatory,
        |ev, v: &mut View| match ev {
            Event::Observed(o) => v.last_observed = serde_json::to_value(&o).ok(),
            Event::Universe(u) => v.universe = serde_json::to_value(&u).ok(),
            _ => {}
        },
        view.clone(),
        tx.clone(),
    );

    // Fan-in: challenger — counterfactual verdicts.
    spawn_feed(
        cfg.ports.challenger,
        |ev, v: &mut View| {
            if let Event::Challenge(c) = ev {
                v.last_challenge = serde_json::to_value(&c).ok();
            }
        },
        view.clone(),
        tx.clone(),
    );

    let app_router = Router::new()
        .route("/", get(|| async { Html(INDEX_HTML) }))
        .route(
            "/logo.svg",
            get(|| async { ([(header::CONTENT_TYPE, "image/svg+xml")], LOGO_SVG).into_response() }),
        )
        .route("/api/meta", get(api::meta))
        .route("/api/pnl", get(api::pnl))
        .route("/api/strategies", get(api::strategies))
        .route("/api/infrastructure", get(api::infrastructure))
        .route("/api/diagnoses", get(api::diagnoses))
        .route("/api/healing", get(api::healing))
        .route("/api/competitive", get(api::competitive))
        .route("/api/reports", get(api::reports))
        .route("/api/stream", get(api::stream))
        .route("/api/wallet", get(api::wallet))
        .route("/api/status", get(api::status))
        .route("/api/killswitch", post(api::killswitch))
        .route("/api/deployments", get(api_deployments))
        .route(
            "/ws",
            get({
                let tx = tx.clone();
                let view = view.clone();
                move |ws: WebSocketUpgrade| {
                    let tx = tx.clone();
                    let view = view.clone();
                    async move { ws.on_upgrade(move |s| handle_ws(s, tx, view)) }
                }
            }),
        )
        .with_state(app);

    let addr: SocketAddr = ([127, 0, 0, 1], cfg.ports.dashboard).into();
    info!(%addr, "dashboard listening (telemetry only — off hot path)");
    axum::serve(tokio::net::TcpListener::bind(addr).await?, app_router).await?;
    Ok(())
}

/// Deployment register (§38.7): honest current-build entry. Fields that
/// cannot be evidenced locally are `null` → rendered NOT VERIFIED.
async fn api_deployments(
    axum::extract::State(st): axum::extract::State<Shared>,
) -> axum::Json<serde_json::Value> {
    // Git commit fingerprint — best-effort, from .git directly.
    let commit = std::fs::read_to_string(".git/HEAD").ok().and_then(|h| {
        let r = h.trim().strip_prefix("ref: ")?;
        std::fs::read_to_string(format!(".git/{r}"))
            .ok()
            .map(|c| c.trim().chars().take(12).collect::<String>())
    });
    let halted = std::path::Path::new("HALT.flag").exists();
    axum::Json(serde_json::json!({
        "deployments": [{
            "deployment_id": "current-worktree",
            "version": env!("CARGO_PKG_VERSION"),
            "git_commit": commit,
            "environment": "local-pm2",
            "mode": if halted { "SAFE-HALT" } else { "development" },
            "chain_scope": allbright_core::events::Chain::ALL.len(),
            "readiness_score": null,
            "critical_gate": "NO-GO (execution unproven, key rotation pending)",
            "commander_decision": null,
            "verification": "partial — see readiness gates",
            "pipeline": [
                "pre-flight","config-validation","wiring-validation",
                "rpc-visibility","simulation","risk-gates",
                "readiness-certification","commander-decision",
                "deployment","post-deploy-verification","monitoring"
            ],
            "note": "formal deployment register not yet populated — \
                     entries below are NOT VERIFIED until pipeline runs emit them",
        }],
        "data_since_ms": st.idx.lock().unwrap().index_since_ms,
    }))
}

fn spawn_feed<F>(
    port: u16,
    apply: F,
    view: Arc<tokio::sync::RwLock<View>>,
    tx: broadcast::Sender<View>,
) where
    F: Fn(&Event, &mut View) + Send + Sync + 'static,
{
    tokio::spawn(async move {
        let addr = format!("127.0.0.1:{port}");
        loop {
            let mut cli = allbright_core::bus::BusClient::connect_retry(&addr).await;
            while let Some(ev) = cli.incoming.recv().await {
                {
                    let mut v = view.write().await;
                    apply(&ev, &mut v);
                }
                let snapshot = view.read().await.clone();
                let _ = tx.send(snapshot);
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    });
}

async fn handle_ws(
    mut socket: WebSocket,
    tx: broadcast::Sender<View>,
    view: Arc<tokio::sync::RwLock<View>>,
) {
    // Send current snapshot, then broadcast updates.
    {
        let v = view.read().await;
        if let Ok(s) = serde_json::to_string(&*v) {
            let _ = socket.send(Message::Text(s.into())).await;
        }
    }
    let mut rx = tx.subscribe();
    loop {
        tokio::select! {
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
            upd = rx.recv() => {
                match upd {
                    Ok(v) => {
                        if let Ok(s) = serde_json::to_string(&v) {
                            if socket.send(Message::Text(s.into())).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(_) => {}
                }
            }
        }
    }
}
