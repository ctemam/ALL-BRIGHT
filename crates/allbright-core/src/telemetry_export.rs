//! OpenTelemetry export (GAP-23) — OTLP metrics + tracing bridge.
//!
//! Contract (measure-first; zero hot-path cost by default):
//! - OTLP export requires BOTH the `telemetry` cargo feature (off by
//!   default — keeps core dependency-light) AND the env var
//!   `OTEL_EXPORTER_OTLP_ENDPOINT`. Either absent → disabled; every
//!   instrument is a no-op handle on the global noop meter.
//! - Uses `opentelemetry` + `opentelemetry-otlp` 0.31.x + the
//!   `tracing-opentelemetry` 0.32 bridge. (NOT the deprecated
//!   `opentelemetry-prometheus` stack.)
//! - Instruments: counter `events_total{component,class}`, histogram
//!   `event_latency_ms`, gauge `rpc_qualified{chain}`.
//!   OTLP resource `service.name = "allbright-<service>"`.
//!
//! Intended init call sites (NOT wired — each binary's main(), after
//! `init_crypto_provider()` + tracing_subscriber init):
//!   allbright-{ingest,simulation,solver,executor,research,dashboard,
//!   observatory,challenger,sentinel,rpc-scout}/src/main.rs
//!   `let _t = allbright_core::telemetry_export::Telemetry::init("solver");`

use opentelemetry::global;
use opentelemetry::metrics::{Counter, Gauge, Histogram, Meter};
use opentelemetry::KeyValue;

#[cfg(feature = "telemetry")]
use opentelemetry_sdk::metrics::SdkMeterProvider;

pub const ENDPOINT_ENV: &str = "OTEL_EXPORTER_OTLP_ENDPOINT";

/// Process telemetry handle. Cheap to hold; dropping it shuts the
/// provider down when OTLP was enabled.
pub struct Telemetry {
    enabled: bool,
    events_total: Counter<u64>,
    event_latency_ms: Histogram<f64>,
    rpc_qualified: Gauge<u64>,
    #[cfg(feature = "telemetry")]
    provider: Option<SdkMeterProvider>,
}

impl Telemetry {
    /// `service` is the crate short-name ("solver", "ingest", …).
    pub fn init(service: &str) -> Self {
        // `global::meter` requires a 'static name — leak the small
        // per-process meter name once (one init per process lifetime).
        let meter_name: &'static str =
            Box::leak(format!("allbright-{service}").into_boxed_str());
        let endpoint =
            std::env::var(ENDPOINT_ENV).ok().filter(|s| !s.is_empty());

        #[cfg(feature = "telemetry")]
        let provider: Option<SdkMeterProvider> = match endpoint {
            Some(ep) => match build_provider(&ep, service) {
                Ok(p) => {
                    global::set_meter_provider(p.clone());
                    tracing::info!(endpoint = %ep, service = %meter_name,
                        "OTLP metrics export enabled");
                    Some(p)
                }
                Err(e) => {
                    tracing::warn!(err = %e,
                        "OTLP exporter init failed — telemetry disabled");
                    None
                }
            },
            None => {
                tracing::debug!(
                    "{ENDPOINT_ENV} unset — telemetry disabled (zero overhead)"
                );
                None
            }
        };

        #[cfg(not(feature = "telemetry"))]
        {
            if endpoint.is_some() {
                tracing::warn!("{ENDPOINT_ENV} set but `telemetry` feature \
                    not compiled — disabled");
            }
        }

        let meter: Meter = global::meter(meter_name);
        Telemetry {
            enabled: {
                #[cfg(feature = "telemetry")]
                {
                    provider.is_some()
                }
                #[cfg(not(feature = "telemetry"))]
                {
                    false
                }
            },
            events_total: meter
                .u64_counter("events_total")
                .with_description("Allbright funnel/lifecycle events")
                .build(),
            event_latency_ms: meter
                .f64_histogram("event_latency_ms")
                .with_unit("ms")
                .with_description("Event handling latency")
                .build(),
            rpc_qualified: meter
                .u64_gauge("rpc_qualified")
                .with_description("Currently qualified RPC endpoints")
                .build(),
            #[cfg(feature = "telemetry")]
            provider,
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// events_total{component,class} += 1
    pub fn record_event(&self, component: &str, class: &str) {
        self.events_total.add(
            1,
            &[
                KeyValue::new("component", component.to_string()),
                KeyValue::new("class", class.to_string()),
            ],
        );
    }

    /// event_latency_ms observe
    pub fn record_latency(&self, component: &str, class: &str, ms: f64) {
        self.event_latency_ms.record(
            ms,
            &[
                KeyValue::new("component", component.to_string()),
                KeyValue::new("class", class.to_string()),
            ],
        );
    }

    /// rpc_qualified{chain} = n
    pub fn set_rpc_qualified(&self, chain: &str, n: u64) {
        self.rpc_qualified
            .record(n, &[KeyValue::new("chain", chain.to_string())]);
    }

    /// tracing → OTel metrics bridge (feature-gated). Attach via
    /// `tracing_subscriber::registry().with(layer)`; `tracing` events
    /// carrying `counter.*` / `histogram.*` fields flow into the meter.
    /// `MetricsLayer::new` takes the `MeterProvider` in
    /// tracing-opentelemetry 0.32 — pass the provider clone, not a Meter.
    #[cfg(feature = "telemetry")]
    pub fn metrics_layer<S>(
        &self,
    ) -> Option<tracing_opentelemetry::MetricsLayer<S, SdkMeterProvider>>
    where
        S: tracing::Subscriber
            + for<'span> tracing_subscriber::registry::LookupSpan<'span>,
    {
        self.provider
            .as_ref()
            .map(|p| tracing_opentelemetry::MetricsLayer::new(p.clone()))
    }
}

#[cfg(feature = "telemetry")]
fn build_provider(endpoint: &str, service: &str) -> anyhow::Result<SdkMeterProvider> {
    use opentelemetry_otlp::WithExportConfig;
    let exporter = opentelemetry_otlp::MetricExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint)
        .build()?;
    let reader = opentelemetry_sdk::metrics::PeriodicReader::builder(exporter)
        .with_interval(std::time::Duration::from_secs(30))
        .build();
    let resource = opentelemetry_sdk::Resource::builder()
        .with_service_name(format!("allbright-{service}"))
        .build();
    Ok(SdkMeterProvider::builder()
        .with_reader(reader)
        .with_resource(resource)
        .build())
}

impl Drop for Telemetry {
    fn drop(&mut self) {
        #[cfg(feature = "telemetry")]
        if let Some(p) = self.provider.take() {
            if let Err(e) = p.shutdown() {
                tracing::warn!(err = ?e, "meter provider shutdown");
            }
        }
    }
}
