use reqwest::Client;
use serde_json::{json, Value};
use std::time::Duration;
use tracing::warn;

/// Pimlico ERC-4337 bundler + paymaster JSON-RPC client.
///
/// Endpoint: `https://api.pimlico.io/v2/{chain_id}/rpc?apikey=...` — the v2 API accepts a
/// numeric chain id for every supported chain, so no per-chain slug table is needed.
/// The API key is only ever read server-side; the browser talks to Pimlico through the
/// `POST /api/pimlico/rpc/{chain_id}` proxy in `api.rs`.
///
/// UserOperation payloads use the flat/unpacked field layout
/// (`verificationGasLimit`, `callGasLimit`, `maxFeePerGas`, `maxPriorityFeePerGas`,
/// `factory`/`factoryData`, `paymaster`/`paymasterData`). This was verified against the
/// live Pimlico API: the packed v0.7 layout (`accountGasLimits`, `gasFees`) and the v0.6
/// layout (`initCode`, `paymasterAndData`) are both rejected with -32601 validation errors
/// for `eth_sendUserOperation`, `eth_estimateUserOperationGas` and `pm_sponsorUserOperation`.
pub struct PimlicoClient {
    client: Client,
    api_key: Option<String>,
}

impl PimlicoClient {
    /// EntryPoint v0.6 (still served by Pimlico, legacy accounts).
    pub const ENTRY_POINT_V06: &'static str = "0x5FF137D4b0FDCD49DcA30c7CF57E578a026d2789";
    /// EntryPoint v0.7 — the version used for SimpleAccount UserOperations (default in
    /// permissionless.js and supported by Pimlico's bundler + paymaster).
    pub const ENTRY_POINT_V07: &'static str = "0x0000000071727De22E5E9d8BAf0edAc6f37da032";
    /// EntryPoint v0.8 (PackedUserOperation / EIP-7702).
    pub const ENTRY_POINT_V08: &'static str = "0x4337084d9e255ff0702461cf8895ce9e3b5ff108";

    pub fn new(api_key: Option<String>) -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(20))
                .user_agent("ZeroCapArb/1.0")
                .build()
                .expect("Failed to create HTTP client"),
            api_key: api_key.filter(|k| !k.trim().is_empty()),
        }
    }

    /// True when `PIMLICO_API_KEY` was provided and non-empty.
    pub fn is_configured(&self) -> bool {
        self.api_key.is_some()
    }

    fn endpoint(&self, chain_id: u64) -> Result<String, String> {
        let key = self.api_key.as_deref().ok_or_else(|| {
            "PIMLICO_API_KEY is not configured; set it in .env to enable ERC-4337 bundling"
                .to_string()
        })?;
        Ok(format!(
            "https://api.pimlico.io/v2/{}/rpc?apikey={}",
            chain_id, key
        ))
    }

    /// Raw JSON-RPC call. Returns `result` on success; a Pimlico `error` object is
    /// converted into `Err` carrying its message (bundler/paymaster failures such as
    /// `AA20 account not deployed` surface verbatim).
    pub async fn rpc(&self, chain_id: u64, method: &str, params: Value) -> Result<Value, String> {
        let url = self.endpoint(chain_id)?;
        let body = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });

        let resp = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Pimlico request failed: {}", e))?;

        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| format!("Pimlico response read failed: {}", e))?;

        if !status.is_success() {
            warn!("Pimlico HTTP error {}: {}", status, text);
            return Err(format!("Pimlico HTTP {}: {}", status, text));
        }

        let value: Value = serde_json::from_str(&text)
            .map_err(|e| format!("Pimlico returned a non-JSON response: {}", e))?;

        if let Some(err) = value.get("error").filter(|e| !e.is_null()) {
            let code = err.get("code").and_then(|c| c.as_i64()).unwrap_or(0);
            let message = err
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown bundler error");
            return Err(format!("Pimlico error {}: {}", code, message));
        }

        value
            .get("result")
            .cloned()
            .ok_or_else(|| "Pimlico response missing `result`".to_string())
    }

    // ─── Typed helpers ─────────────────────────────────

    /// `eth_supportedEntryPoints` — entry points served on this chain for this API key.
    pub async fn supported_entry_points(&self, chain_id: u64) -> Result<Vec<String>, String> {
        let result = self
            .rpc(chain_id, "eth_supportedEntryPoints", json!([]))
            .await?;
        Ok(result
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default())
    }

    /// `eth_estimateUserOperationGas` — gas bounds for an (optionally undeployed) account.
    pub async fn estimate_user_operation_gas(
        &self,
        chain_id: u64,
        user_operation: &Value,
        entry_point: &str,
    ) -> Result<Value, String> {
        self.rpc(
            chain_id,
            "eth_estimateUserOperationGas",
            json!([user_operation, entry_point]),
        )
        .await
    }

    /// `pm_sponsorUserOperation` — ask the Pimlico paymaster to sponsor gas.
    pub async fn sponsor_user_operation(
        &self,
        chain_id: u64,
        user_operation: &Value,
        entry_point: &str,
    ) -> Result<Value, String> {
        self.rpc(
            chain_id,
            "pm_sponsorUserOperation",
            json!([user_operation, entry_point, {}]),
        )
        .await
    }

    /// `eth_sendUserOperation` — submit a signed UserOperation to the bundler.
    /// Returns the `userOpHash`.
    pub async fn send_user_operation(
        &self,
        chain_id: u64,
        user_operation: &Value,
        entry_point: &str,
    ) -> Result<String, String> {
        let result = self
            .rpc(
                chain_id,
                "eth_sendUserOperation",
                json!([user_operation, entry_point]),
            )
            .await?;
        result
            .as_str()
            .map(String::from)
            .ok_or_else(|| "eth_sendUserOperation returned no userOpHash".to_string())
    }

    /// `eth_getUserOperationReceipt` — receipt of a bundled UserOperation
    /// (includes the on-chain transaction hash).
    pub async fn user_operation_receipt(
        &self,
        chain_id: u64,
        user_op_hash: &str,
        entry_point: &str,
    ) -> Result<Value, String> {
        self.rpc(
            chain_id,
            "eth_getUserOperationReceipt",
            json!([user_op_hash, entry_point]),
        )
        .await
    }

    /// `pimlico_getUserOperationStatus` — bundler-side status of a submitted UserOperation.
    pub async fn user_operation_status(
        &self,
        chain_id: u64,
        user_op_hash: &str,
    ) -> Result<Value, String> {
        self.rpc(
            chain_id,
            "pimlico_getUserOperationStatus",
            json!([user_op_hash]),
        )
        .await
    }

    /// `pimlico_getUserOperationGasPrice` — slow/standard/fast fee quotes.
    pub async fn gas_price(&self, chain_id: u64) -> Result<Value, String> {
        self.rpc(chain_id, "pimlico_getUserOperationGasPrice", json!([]))
            .await
    }
}
