//! Simulation gate (GAP-1 / GAP-6) — `eth_simulateV1`-first EVM dry-run.
//!
//! Role in the pipeline: pre-submission EVM gate for EVERY execution
//! path. `relay::submit_via_aa` currently gates the UserOp payload with a
//! spoofed-`from` `eth_call` (`eth_call_from`, relay.rs:187-209). This
//! module is a strict superset: where the endpoint supports
//! `eth_simulateV1` (Base public endpoint `https://mainnet.base.org` via
//! Flashblocks; Nodit endpoints on arb/bsc/base/op/poly per the AGENTS
//! GAP-1 note) the exact `execute()` calldata runs inside a single
//! `blockStateCall` with `validation:false` — flash loan, callbacks,
//! swaps and the on-chain `minProfit` assert all execute off-chain
//! against live state. Where the method is absent the gate permanently
//! falls back to the same `eth_call` semantics relay uses today.
//!
//! Failure taxonomy mirrors `relay.rs` `DryRunErr` exactly so this drops
//! in as a one-for-one replacement:
//!   * `EvmOk`      ↔ `Ok(bytes)`            — call executed; return data
//!   * `EvmRevert`  ↔ `Err(DryRunErr::Evm)`  — node ran it and it failed;
//!                                           the real tx reverts
//!                                           identically → always gates
//!   * `Transport`  ↔ `Err(DryRunErr::Transport)` — HTTP/JSON failure;
//!                                           inconclusive, never gates
//!
//! Intended call site: the `eth_call_from` loop at relay.rs:441-457
//! inside `submit_via_aa`:
//!
//! ```ignore
//! match simgate::simulate_route(&rpc, kernel, executor, Bytes::from(encoded.clone())).await? {
//!     SimOutcome::EvmOk { .. } => { gated = true; break; }
//!     SimOutcome::EvmRevert { reason, .. } => anyhow::bail!("evm dry-run gate: {reason}"),
//!     SimOutcome::Transport(e) => transport_err = Some(e.to_string()),
//! }
//! ```
//!
//! Capability detection is per-endpoint (URL), cached for process life:
//! the first call on an endpoint tries `eth_simulateV1`; a definitive
//! answer caches `SimulateV1`, a method-not-found answer caches
//! `EthCallOnly` forever, and a transport failure caches nothing so the
//! next call re-probes. Nothing is assumed from the URL — detection is
//! the only source of truth (AGENTS.md §5: never assume Ethereum infra
//! is portable).
//!
//! `validation:false` skips sender nonce/balance/signature checks — the
//! Kernel smart account holds no native gas, so the simulated call must
//! not be rejected for prefund.

use alloy::primitives::{hex, Address, Bytes};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

/// What the endpoint proved about this call.
#[derive(Debug)]
pub enum SimOutcome {
    /// The node executed the call to completion. `gas_used` is the
    /// EVM-reported gas for the call (0 when the transport didn't report).
    EvmOk { return_data: Bytes, gas_used: u64 },
    /// The node executed the call and it REVERTED — the real transaction
    /// would fail identically on-chain. `reason` carries the RPC error
    /// message; `data` carries raw revert data (Error(string) payload,
    /// custom error, or empty). This outcome ALWAYS gates.
    EvmRevert { reason: String, data: Bytes },
    /// HTTP/JSON/transport failure — the node never gave a verdict.
    /// Inconclusive: callers may retry; per relay.rs semantics a
    /// transport-inconclusive gate does not block submission (the
    /// on-chain `minProfit` guard still bounds the risk).
    Transport(anyhow::Error),
}

impl SimOutcome {
    /// True iff the call executed to completion.
    pub fn is_ok(&self) -> bool {
        matches!(self, SimOutcome::EvmOk { .. })
    }

    /// Raw revert data when the outcome is an EVM revert.
    pub fn revert_data(&self) -> Option<&Bytes> {
        match self {
            SimOutcome::EvmRevert { data, .. } => Some(data),
            _ => None,
        }
    }
}

/// Drop-in error type mirroring `relay.rs::DryRunErr` — kept public so
/// callers outside relay can map outcomes without depending on relay's
/// private enum.
#[derive(Debug)]
pub enum GateErr {
    Evm(String),
    Transport(anyhow::Error),
}

impl std::fmt::Display for GateErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GateErr::Evm(s) => write!(f, "{s}"),
            GateErr::Transport(e) => write!(f, "{e}"),
        }
    }
}
impl std::error::Error for GateErr {}

impl SimOutcome {
    /// Convert to the `eth_call_from` shape: `Ok(return_data)` /
    /// `Err(GateErr::Evm)` / `Err(GateErr::Transport)`.
    pub fn into_gate_result(self) -> std::result::Result<Bytes, GateErr> {
        match self {
            SimOutcome::EvmOk { return_data, .. } => Ok(return_data),
            SimOutcome::EvmRevert { reason, data } => Err(GateErr::Evm(format!(
                "{reason} 0x{}",
                hex::encode(&data)
            ))),
            SimOutcome::Transport(e) => Err(GateErr::Transport(e)),
        }
    }
}

/// Per-endpoint method capability. Learned, never assumed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimCapability {
    /// `eth_simulateV1` answered a real request with a parseable result.
    SimulateV1,
    /// `eth_simulateV1` returned a method-not-found-class error — the
    /// fallback `eth_call` (with overridden `from`) is used permanently.
    EthCallOnly,
}

impl SimCapability {
    pub fn name(self) -> &'static str {
        match self {
            SimCapability::SimulateV1 => "eth_simulateV1",
            SimCapability::EthCallOnly => "eth_call",
        }
    }
}

/// Internal tri-state for one `eth_simulateV1` attempt.
enum V1Verdict {
    /// Definitive response (executed, reverted, or call-level error the
    /// method still answered) → endpoint supports the method.
    Answered(SimOutcome),
    /// Method-not-found class error → permanently downgrade.
    Unsupported,
    /// Transport-level failure → nothing learned; do not cache.
    Unknown(anyhow::Error),
}

/// Stateful gate: owns the HTTP client and the per-endpoint capability
/// cache. One `SimGate` per process (see [`global`]); the free function
/// [`simulate_route`] is the ergonomic entry point.
pub struct SimGate {
    client: reqwest::Client,
    /// rpc url → capability. Only definitive answers are stored.
    caps: Mutex<HashMap<String, SimCapability>>,
}

impl Default for SimGate {
    fn default() -> Self {
        Self::new()
    }
}

impl SimGate {
    pub fn new() -> Self {
        SimGate {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            caps: Mutex::new(HashMap::new()),
        }
    }

    fn caps_lock(&self) -> MutexGuard<'_, HashMap<String, SimCapability>> {
        self.caps.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn cached(&self, rpc: &str) -> Option<SimCapability> {
        self.caps_lock().get(rpc).copied()
    }

    fn set_cap(&self, rpc: &str, cap: SimCapability) {
        self.caps_lock().insert(rpc.to_string(), cap);
    }

    /// Probe (or read the cache for) an endpoint's simulation
    /// capability. A transport-inconclusive probe reports `EthCallOnly`
    /// for this call but is NOT cached — the next call re-probes.
    pub async fn capability(&self, rpc: &str) -> SimCapability {
        if let Some(c) = self.cached(rpc) {
            return c;
        }
        match self
            .try_v1(rpc, Address::ZERO, Address::ZERO, &Bytes::new())
            .await
        {
            V1Verdict::Answered(_) => {
                self.set_cap(rpc, SimCapability::SimulateV1);
                tracing::info!(%rpc, "eth_simulateV1 supported");
                SimCapability::SimulateV1
            }
            V1Verdict::Unsupported => {
                self.set_cap(rpc, SimCapability::EthCallOnly);
                tracing::info!(%rpc, "eth_simulateV1 unsupported — permanent eth_call fallback");
                SimCapability::EthCallOnly
            }
            V1Verdict::Unknown(e) => {
                tracing::debug!(%rpc, err = %e, "capability probe inconclusive");
                SimCapability::EthCallOnly
            }
        }
    }

    /// Simulate `from → to(data)` through the best method the endpoint
    /// supports. Always resolves to a `SimOutcome`; the outer `Result`
    /// exists for signature stability with callers that want `?`.
    pub async fn simulate(
        &self,
        rpc: &str,
        from: Address,
        to: Address,
        data: Bytes,
    ) -> anyhow::Result<SimOutcome> {
        if self.cached(rpc) == Some(SimCapability::EthCallOnly) {
            return Ok(self.eth_call_outcome(rpc, from, to, &data).await);
        }
        match self.try_v1(rpc, from, to, &data).await {
            V1Verdict::Answered(o) => {
                self.set_cap(rpc, SimCapability::SimulateV1);
                Ok(o)
            }
            V1Verdict::Unsupported => {
                // Permanent fallback: first definitive proof the method
                // is absent. Never assume — detect (AGENTS.md §5).
                self.set_cap(rpc, SimCapability::EthCallOnly);
                tracing::info!(%rpc, "eth_simulateV1 unsupported — permanent eth_call fallback");
                Ok(self.eth_call_outcome(rpc, from, to, &data).await)
            }
            V1Verdict::Unknown(e) => Ok(SimOutcome::Transport(e)),
        }
    }

    /// One `eth_simulateV1` attempt. `blockStateCalls` carries exactly
    /// one call — the executor `execute()` calldata — with
    /// `validation:false` so prefund/nonce checks are skipped.
    async fn try_v1(&self, rpc: &str, from: Address, to: Address, data: &Bytes) -> V1Verdict {
        let body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "eth_simulateV1",
            "params": [
                {
                    "blockStateCalls": [
                        {
                            "calls": [
                                {
                                    "from": format!("{from:?}"),
                                    "to": format!("{to:?}"),
                                    "data": format!("0x{}", hex::encode(data)),
                                }
                            ]
                        }
                    ],
                    "validation": false,
                    "traceTransfers": false,
                },
                "latest"
            ]
        });
        let v: Value = match self.post(rpc, &body).await {
            Ok(v) => v,
            Err(e) => return V1Verdict::Unknown(e),
        };
        if let Some(err) = v.get("error") {
            if is_unsupported_error(err) {
                return V1Verdict::Unsupported;
            }
            // A top-level error the method still answered: execution-
            // level failures surface here on some implementations.
            return V1Verdict::Answered(classify_rpc_error(err));
        }
        match parse_simulate_result(&v) {
            Ok(o) => V1Verdict::Answered(o),
            Err(msg) => V1Verdict::Answered(SimOutcome::Transport(anyhow::anyhow!(
                "eth_simulateV1 unrecognized result: {msg}"
            ))),
        }
    }

    /// `eth_call` with an overridden `from` — identical semantics to
    /// `relay.rs::eth_call_from`: any JSON-RPC `error` field is an
    /// EVM-level failure (gates); transport failure is inconclusive.
    async fn eth_call_outcome(
        &self,
        rpc: &str,
        from: Address,
        to: Address,
        data: &Bytes,
    ) -> SimOutcome {
        let body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "eth_call",
            "params": [
                {
                    "from": format!("{from:?}"),
                    "to": format!("{to:?}"),
                    "data": format!("0x{}", hex::encode(data)),
                },
                "latest"
            ]
        });
        let v: Value = match self.post(rpc, &body).await {
            Ok(v) => v,
            Err(e) => return SimOutcome::Transport(e),
        };
        if let Some(err) = v.get("error") {
            return classify_rpc_error(err);
        }
        let s = v["result"].as_str().unwrap_or("0x");
        SimOutcome::EvmOk {
            return_data: Bytes::from(
                hex::decode(s.trim_start_matches("0x")).unwrap_or_default(),
            ),
            gas_used: 0,
        }
    }

    async fn post(&self, rpc: &str, body: &Value) -> anyhow::Result<Value> {
        let resp = self.client.post(rpc).json(body).send().await?;
        let status = resp.status();
        let v: Value = resp
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("rpc http {status}: {e}"))?;
        Ok(v)
    }
}

/// Process-global gate used by [`simulate_route`]. Per-endpoint
/// capability is learned once and shared by every caller.
static GLOBAL_GATE: std::sync::OnceLock<SimGate> = std::sync::OnceLock::new();

pub fn global() -> &'static SimGate {
    GLOBAL_GATE.get_or_init(SimGate::new)
}

/// Gate one route's `execute()` calldata before submission.
///
/// `from` must be the caller the real transaction will use — for the AA
/// path that is the Kernel smart-account address (the contract owner);
/// `eth_simulateV1`/`eth_call` both honor the override. `to` is the
/// deployed `AllbrightExecutor`; `data` is the encoded `execute()` call.
pub async fn simulate_route(
    rpc: &str,
    from: Address,
    to: Address,
    data: Bytes,
) -> anyhow::Result<SimOutcome> {
    global().simulate(rpc, from, to, data).await
}

/// Does this JSON-RPC error object mean "method not supported"?
/// Conservative: a message that smells like a revert NEVER counts as
/// unsupported (a revert reason like "token not found" must not
/// downgrade the endpoint).
fn is_unsupported_error(err: &Value) -> bool {
    let code = err["code"].as_i64().unwrap_or(0);
    let msg = err["message"].as_str().unwrap_or("").to_lowercase();
    if msg.contains("revert") {
        return false;
    }
    // -32601 method not found, -32604 method not supported.
    // -32602 invalid params on a spec-shaped request means a divergent
    // implementation — permanent eth_call fallback keeps the gate live.
    if matches!(code, -32601 | -32604 | -32602) {
        return true;
    }
    msg.contains("does not exist")
        || msg.contains("unknown method")
        || msg.contains("not supported")
        || msg.contains("unsupported method")
        || (msg.contains("method") && msg.contains("not found"))
        || (msg.contains("method") && msg.contains("not available"))
}

/// Map a JSON-RPC error object to a `SimOutcome` (eth_call path and
/// non-method-not-found simulateV1 errors share this): a revert-class
/// message or error data payload → `EvmRevert`; anything else is a
/// node-level failure → `Transport` (inconclusive).
fn classify_rpc_error(err: &Value) -> SimOutcome {
    let msg = err["message"].as_str().unwrap_or("evm error");
    let data_hex = err["data"].as_str().unwrap_or_default();
    let data = Bytes::from(
        hex::decode(data_hex.trim_start_matches("0x")).unwrap_or_default(),
    );
    let code = err["code"].as_i64().unwrap_or(0);
    // JSON-RPC: -32000-class server errors carry execution failures;
    // "execution reverted"/"revert" messages and non-empty revert data
    // are definitive EVM failures. Everything else (rate limits, parse
    // errors, upstream issues) is inconclusive transport.
    let revertish = msg.to_lowercase().contains("revert") || !data.is_empty();
    if revertish || code == 3 {
        SimOutcome::EvmRevert {
            reason: msg.to_string(),
            data,
        }
    } else {
        SimOutcome::Transport(anyhow::anyhow!("rpc error {code}: {msg}"))
    }
}

/// Parse `result[0].calls[0]`-shaped output: `result` is an array of
/// block results; each carries `calls[]` with `status` ("0x1" ok /
/// "0x0" revert), `returnData`, `gasUsed`, and on failure an `error`
/// object.
fn parse_simulate_result(v: &Value) -> std::result::Result<SimOutcome, String> {
    let block = v["result"]
        .as_array()
        .and_then(|a| a.first())
        .ok_or_else(|| "result.blocks missing".to_string())?;
    let call = block["calls"]
        .as_array()
        .and_then(|a| a.first())
        .ok_or_else(|| "result.blocks[0].calls missing".to_string())?;
    let gas_used = call["gasUsed"]
        .as_str()
        .and_then(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).ok())
        .unwrap_or(0);
    let return_data = Bytes::from(
        hex::decode(
            call["returnData"]
                .as_str()
                .unwrap_or("0x")
                .trim_start_matches("0x"),
        )
        .unwrap_or_default(),
    );
    if let Some(e) = call.get("error") {
        let data = Bytes::from(
            hex::decode(
                e["data"].as_str().unwrap_or("0x").trim_start_matches("0x"),
            )
            .unwrap_or_default(),
        );
        return Ok(SimOutcome::EvmRevert {
            reason: e["message"]
                .as_str()
                .unwrap_or("execution reverted")
                .to_string(),
            data: if data.is_empty() { return_data } else { data },
        });
    }
    match call["status"].as_str() {
        Some("0x1") | Some("0x01") => Ok(SimOutcome::EvmOk { return_data, gas_used }),
        Some("0x0") | Some("0x00") => Ok(SimOutcome::EvmRevert {
            reason: "execution reverted (status 0x0)".to_string(),
            data: return_data,
        }),
        other => Err(format!("unrecognized call status {other:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_method_not_found() {
        let e = json!({"code": -32601, "message": "the method eth_simulateV1 does not exist/is not available"});
        assert!(is_unsupported_error(&e));
    }

    #[test]
    fn revert_never_marks_unsupported() {
        // A revert REASON containing "not found" must not downgrade.
        let e = json!({"code": -32000, "message": "execution reverted: token not found"});
        assert!(!is_unsupported_error(&e));
    }

    #[test]
    fn parse_ok_call() {
        let v = json!({"result": [{"calls": [{"status": "0x1", "gasUsed": "0x5208",
            "returnData": "0xabcd", "logs": []}]}]});
        match parse_simulate_result(&v).unwrap() {
            SimOutcome::EvmOk { return_data, gas_used } => {
                assert_eq!(gas_used, 0x5208);
                assert_eq!(return_data.as_ref(), &[0xab, 0xcd]);
            }
            _ => panic!("expected EvmOk"),
        }
    }

    #[test]
    fn parse_reverted_call_with_error() {
        let v = json!({"result": [{"calls": [{"status": "0x0", "gasUsed": "0x1234",
            "returnData": "0x",
            "error": {"code": 3, "message": "execution reverted: CamelotPair: K",
                      "data": "0x08c379a0"}}]}]});
        match parse_simulate_result(&v).unwrap() {
            SimOutcome::EvmRevert { reason, data } => {
                assert!(reason.contains("CamelotPair: K"));
                assert_eq!(data.as_ref(), &[0x08, 0xc3, 0x79, 0xa0]);
            }
            _ => panic!("expected EvmRevert"),
        }
    }

    #[test]
    fn parse_status_zero_revert() {
        let v = json!({"result": [{"calls": [{"status": "0x0", "gasUsed": "0x1",
            "returnData": "0xdead", "logs": []}]}]});
        assert!(matches!(
            parse_simulate_result(&v).unwrap(),
            SimOutcome::EvmRevert { .. }
        ));
    }

    #[test]
    fn malformed_result_is_err() {
        let v = json!({"result": []});
        assert!(parse_simulate_result(&v).is_err());
        let v = json!({"result": [{"calls": []}]});
        assert!(parse_simulate_result(&v).is_err());
    }

    #[test]
    fn error_field_classifies() {
        let rev = json!({"code": -32000, "message": "execution reverted: UniswapV2: K",
            "data": "0x08c379a0"});
        assert!(matches!(classify_rpc_error(&rev), SimOutcome::EvmRevert { .. }));
        let rate = json!({"code": -32005, "message": "limit exceeded"});
        assert!(matches!(classify_rpc_error(&rate), SimOutcome::Transport(_)));
    }
}
