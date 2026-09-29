//! Server configuration: ports, run mode, deployment region, profit transfer.
//!
//! Port values default to the `simulation`/`production` ranges declared in
//! `ports.config.json` at the repo root (the single source of truth shared with
//! `ecosystem.config.js` and the Dockerfile). `PORT` always wins when set, so
//! PM2 and container orchestration can override without code changes.

use std::net::SocketAddr;

/// Which port range the server runs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    Simulation,
    Production,
}

impl RunMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            RunMode::Simulation => "simulation",
            RunMode::Production => "production",
        }
    }

    /// `ZCA_MODE=production` (or the legacy alias `live`) selects the
    /// production port range; anything else, including unset, is simulation.
    fn from_env() -> Self {
        match std::env::var("ZCA_MODE")
            .unwrap_or_default()
            .trim()
            .to_lowercase()
            .as_str()
        {
            "production" | "live" => RunMode::Production,
            _ => RunMode::Simulation,
        }
    }
}

impl std::fmt::Display for RunMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Profit withdrawal behaviour. `Manual` is the fail-safe default: no funds
/// move until an operator explicitly opts in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfitTransferMode {
    Auto,
    Manual,
}

impl ProfitTransferMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProfitTransferMode::Auto => "AUTO",
            ProfitTransferMode::Manual => "MANUAL",
        }
    }
}

impl std::fmt::Display for ProfitTransferMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub port: u16,
    pub host: String,
    pub mode: RunMode,
    /// Deployment region label (e.g. `frankfurt`). Advisory: it does not move
    /// the process, it is recorded for telemetry and RPC endpoint selection.
    pub region: String,
    pub profit_transfer_mode: ProfitTransferMode,
    /// Server-enforced floor below which a withdrawal is refused, in USD.
    pub profit_transfer_min_usd: f64,
    /// Upper bound on concurrent outbound RPC requests.
    pub rpc_max_concurrency: usize,
}

impl ServerConfig {
    pub fn from_env() -> Self {
        let mode = RunMode::from_env();
        let default_port = match mode {
            RunMode::Simulation => 3001,
            RunMode::Production => 4001,
        };
        let port = std::env::var("PORT")
            .ok()
            .and_then(|p| p.parse::<u16>().ok())
            .unwrap_or(default_port);
        let host = std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string());

        let region = std::env::var("DEPLOYMENT_REGION").unwrap_or_else(|_| "unpinned".to_string());

        let profit_transfer_mode = match std::env::var("PROFIT_TRANSFER_MODE")
            .unwrap_or_default()
            .to_uppercase()
            .as_str()
        {
            "AUTO" => ProfitTransferMode::Auto,
            _ => ProfitTransferMode::Manual,
        };

        let profit_transfer_min_usd = std::env::var("PROFIT_TRANSFER_MIN_USD")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .filter(|v| v.is_finite() && *v > 0.0)
            .unwrap_or(25.0);

        let rpc_max_concurrency = std::env::var("RPC_MAX_CONCURRENCY")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|v| *v > 0)
            .unwrap_or(16);

        Self {
            port,
            host,
            mode,
            region,
            profit_transfer_mode,
            profit_transfer_min_usd,
            rpc_max_concurrency,
        }
    }

    pub fn bind_address(&self) -> SocketAddr {
        // `host` is operator-supplied; fall back to all interfaces rather than
        // panicking on a malformed value.
        SocketAddr::from((
            self.host
                .parse::<std::net::IpAddr>()
                .unwrap_or(std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED)),
            self.port,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bind_address_uses_host_and_port() {
        let cfg = ServerConfig {
            port: 4001,
            host: "127.0.0.1".into(),
            mode: RunMode::Production,
            region: "frankfurt".into(),
            profit_transfer_mode: ProfitTransferMode::Manual,
            profit_transfer_min_usd: 25.0,
            rpc_max_concurrency: 16,
        };
        let addr = cfg.bind_address();
        assert_eq!(addr.port(), 4001);
        assert_eq!(addr.ip().to_string(), "127.0.0.1");
    }

    #[test]
    fn malformed_host_falls_back_to_unspecified() {
        let mut cfg = ServerConfig {
            port: 3001,
            host: "not-an-ip".into(),
            mode: RunMode::Simulation,
            region: "unpinned".into(),
            profit_transfer_mode: ProfitTransferMode::Manual,
            profit_transfer_min_usd: 25.0,
            rpc_max_concurrency: 16,
        };
        cfg.host = "not-an-ip".into();
        assert!(cfg.bind_address().ip().is_unspecified());
    }

    #[test]
    fn mode_strings_are_stable() {
        assert_eq!(RunMode::Simulation.as_str(), "simulation");
        assert_eq!(RunMode::Production.as_str(), "production");
        assert_eq!(ProfitTransferMode::Auto.as_str(), "AUTO");
        assert_eq!(ProfitTransferMode::Manual.as_str(), "MANUAL");
    }
}
