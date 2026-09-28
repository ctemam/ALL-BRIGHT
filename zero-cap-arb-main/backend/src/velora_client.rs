use crate::types::*;
use reqwest::Client;
use serde_json::Value;
use std::time::Duration;
use tracing::{info, warn};

/// Velora Market API (formerly ParaSwap V5)
/// Documentation: https://developers.velora.xyz
///
/// Endpoints used:
///   GET  /swap       — unified price + tx calldata (recommended)
///   GET  /prices     — price route only
///   POST /transactions — build tx calldata from price route
///   GET  /tokens     — list supported tokens
///
/// Velora Delta API (intent-based):
///   POST /delta/order — submit a delta order
pub struct VeloraClient {
    client: Client,
    base_url: &'static str,
    api_key: Option<String>,
    partner: &'static str,
}

const VELORA_API: &str = "https://api.paraswap.io";
const PARTNER: &str = "zerocaparb";

impl VeloraClient {
    pub fn new(api_key: Option<String>) -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(10))
                .user_agent("ZeroCapArb/1.0")
                .build()
                .expect("Failed to create HTTP client"),
            base_url: VELORA_API,
            api_key,
            partner: PARTNER,
        }
    }

    // ──────────────────────────────────────────────
    //  Unified /swap — price + calldata in one call
    // ──────────────────────────────────────────────

    /// Get price + transaction calldata in a single GET request.
    /// This replaces the old two-step (prices → transactions) flow.
    pub async fn get_swap(
        &self,
        chain_id: u64,
        src_token: &str,
        dest_token: &str,
        src_decimals: u8,
        dest_decimals: u8,
        amount: &str,
        side: &str,
        user_address: Option<&str>,
        slippage: Option<u32>,
    ) -> Result<VeloraSwapResponse, Box<dyn std::error::Error + Send + Sync>> {
        let mut url = format!(
            "{}/swap?srcToken={}&destToken={}&srcDecimals={}&destDecimals={}&amount={}&side={}&network={}&partner={}&version=6.2",
            self.base_url, src_token, dest_token, src_decimals, dest_decimals, amount, side, chain_id, self.partner,
        );

        if let Some(ua) = user_address {
            url.push_str(&format!("&userAddress={}", ua));
        }
        if let Some(slip) = slippage {
            url.push_str(&format!("&slippage={}", slip));
        }
        if let Some(ref key) = self.api_key {
            url.push_str(&format!("&apiKey={}", key));
        }

        let resp = self.client.get(&url).header("Accept", "application/json").send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            warn!("Velora /swap error {}: {}", status, body);
            return Err(format!("Velora /swap error: {} - {}", status, body).into());
        }

        let raw: Value = resp.json().await?;
        VeloraSwapResponse::from_value(raw)
    }

    // ──────────────────────────────────────────────
    //  /prices — price route only (legacy)
    // ──────────────────────────────────────────────

    pub async fn get_price(
        &self,
        chain_id: u64,
        src_token: &str,
        dest_token: &str,
        src_decimals: u8,
        dest_decimals: u8,
        amount: &str,
        side: &str,
    ) -> Result<VeloraPriceResponse, Box<dyn std::error::Error + Send + Sync>> {
        let mut url = format!(
            "{}/prices?srcToken={}&destToken={}&srcDecimals={}&destDecimals={}&amount={}&side={}&network={}&partner={}",
            self.base_url, src_token, dest_token, src_decimals, dest_decimals, amount, side, chain_id, self.partner,
        );

        if let Some(ref key) = self.api_key {
            url.push_str(&format!("&apiKey={}", key));
        }

        let resp = self.client.get(&url).header("Accept", "application/json").send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Velora /prices error: {} - {}", status, body).into());
        }

        let raw: Value = resp.json().await?;
        VeloraPriceResponse::from_value(raw)
    }

    // ──────────────────────────────────────────────
    //  /transactions — build tx calldata (legacy)
    // ──────────────────────────────────────────────

    pub async fn build_transaction(
        &self,
        chain_id: u64,
        src_token: &str,
        dest_token: &str,
        src_decimals: u8,
        dest_decimals: u8,
        src_amount: &str,
        dest_amount: &str,
        slippage: f64,
        user_address: &str,
        receiver: Option<&str>,
        price_route: &Value,
    ) -> Result<VeloraTxResponse, Box<dyn std::error::Error + Send + Sync>> {
        let url = format!("{}/transactions/{}", self.base_url, chain_id);

        let mut body = serde_json::json!({
            "srcToken": src_token,
            "destToken": dest_token,
            "srcDecimals": src_decimals,
            "destDecimals": dest_decimals,
            "srcAmount": src_amount,
            "destAmount": dest_amount,
            "slippage": slippage,
            "priceRoute": price_route,
            "userAddress": user_address,
            "receiver": receiver.unwrap_or(user_address),
            "partner": self.partner,
        });

        if let Some(ref key) = self.api_key {
            body["apiKey"] = serde_json::json!(key);
        }

        info!("Building Velora tx on chain {}", chain_id);
        let resp = self.client.post(&url).json(&body).header("Accept", "application/json").send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            return Err(format!("Velora /transactions error: {} - {}", status, body_text).into());
        }

        let raw: Value = resp.json().await?;
        VeloraTxResponse::from_value(raw)
    }

    // ──────────────────────────────────────────────
    //  /tokens — list supported tokens
    // ──────────────────────────────────────────────

    pub async fn get_supported_tokens(
        &self,
        chain_id: u64,
    ) -> Result<Vec<TokenInfo>, Box<dyn std::error::Error + Send + Sync>> {
        let url = format!("{}/tokens/{}", self.base_url, chain_id);
        let resp = self.client.get(&url).header("Accept", "application/json").send().await?;
        let raw: Value = resp.json().await?;

        let mut tokens = Vec::new();
        if let Some(arr) = raw.as_array() {
            for t in arr {
                tokens.push(TokenInfo {
                    address: t["address"].as_str().unwrap_or("").to_string(),
                    symbol: t["symbol"].as_str().unwrap_or("").to_string(),
                    decimals: t["decimals"].as_u64().unwrap_or(18) as u8,
                });
            }
        }
        Ok(tokens)
    }

    // ──────────────────────────────────────────────
    //  Velora Delta API — intent-based orders
    // ──────────────────────────────────────────────

    /// Submit a Delta order (intent-based, gas-less execution)
    /// Multiple agents compete to fill the order at the best price.
    pub async fn submit_delta_order(
        &self,
        chain_id: u64,
        src_token: &str,
        dest_token: &str,
        amount: &str,
        user_address: &str,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let url = format!("{}/delta/order", self.base_url);

        let body = serde_json::json!({
            "network": chain_id,
            "srcToken": src_token,
            "destToken": dest_token,
            "amount": amount,
            "userAddress": user_address,
            "partner": self.partner,
        });

        let resp = self.client.post(&url).json(&body).send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            return Err(format!("Velora Delta error: {} - {}", status, body_text).into());
        }

        Ok(resp.json().await?)
    }
}

// ─── Response Types ──────────────────────────────────

#[derive(Debug, Clone)]
pub struct VeloraSwapResponse {
    pub price_route: VeloraPriceRoute,
    pub tx_params: VeloraTxParams,
}

impl VeloraSwapResponse {
    pub fn from_value(raw: Value) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let pr = VeloraPriceRoute::from_value(raw["priceRoute"].clone())?;
        let tx = VeloraTxParams::from_value(raw["txParams"].clone())?;
        Ok(Self { price_route: pr, tx_params: tx })
    }
}

#[derive(Debug, Clone)]
pub struct VeloraPriceResponse {
    pub price_route: VeloraPriceRoute,
}

impl VeloraPriceResponse {
    pub fn from_value(raw: Value) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Ok(Self { price_route: VeloraPriceRoute::from_value(raw["priceRoute"].clone())? })
    }
}

#[derive(Debug, Clone)]
pub struct VeloraPriceRoute {
    pub block_number: u64,
    pub network: u64,
    pub src_token: String,
    pub src_decimals: u8,
    pub src_amount: String,
    pub dest_token: String,
    pub dest_decimals: u8,
    pub dest_amount: String,
    pub best_route: Vec<VeloraRouteSegment>,
    pub gas_cost_usd: String,
    pub gas_cost: String,
    pub side: String,
    pub version: String,
    pub contract_address: String,
    pub token_transfer_proxy: String,
    pub contract_method: String,
    pub src_usd: String,
    pub dest_usd: String,
}

impl VeloraPriceRoute {
    pub fn from_value(v: Value) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let mut segments = Vec::new();
        if let Some(routes) = v["bestRoute"].as_array() {
            for r in routes {
                let mut swaps = Vec::new();
                if let Some(swaps_arr) = r["swaps"].as_array() {
                    for s in swaps_arr {
                        swaps.push(VeloraSwapDetail {
                            src_token: s["srcToken"].as_str().unwrap_or("").to_string(),
                            dest_token: s["destToken"].as_str().unwrap_or("").to_string(),
                            exchange: s["swapExchanges"][0]["exchange"].as_str().unwrap_or("Unknown").to_string(),
                            src_amount: s["swapExchanges"][0]["srcAmount"].as_str().unwrap_or("0").to_string(),
                            dest_amount: s["swapExchanges"][0]["destAmount"].as_str().unwrap_or("0").to_string(),
                            percent: s["swapExchanges"][0]["percent"].as_str().and_then(|p| p.parse().ok()).unwrap_or(0.0),
                        });
                    }
                }
                segments.push(VeloraRouteSegment {
                    percent: r["percent"].as_str().and_then(|p| p.parse().ok()).unwrap_or(0.0),
                    swaps,
                });
            }
        }

        Ok(Self {
            block_number: v["blockNumber"].as_u64().unwrap_or(0),
            network: v["network"].as_u64().unwrap_or(0),
            src_token: v["srcToken"].as_str().unwrap_or("").to_string(),
            src_decimals: v["srcDecimals"].as_u64().unwrap_or(18) as u8,
            src_amount: v["srcAmount"].as_str().unwrap_or("0").to_string(),
            dest_token: v["destToken"].as_str().unwrap_or("").to_string(),
            dest_decimals: v["destDecimals"].as_u64().unwrap_or(18) as u8,
            dest_amount: v["destAmount"].as_str().unwrap_or("0").to_string(),
            best_route: segments,
            gas_cost_usd: v["gasCostUSD"].as_str().unwrap_or("0").to_string(),
            gas_cost: v["gasCost"].as_str().unwrap_or("0").to_string(),
            side: v["side"].as_str().unwrap_or("SELL").to_string(),
            version: v["version"].as_str().unwrap_or("5").to_string(),
            contract_address: v["contractAddress"].as_str().unwrap_or("").to_string(),
            token_transfer_proxy: v["tokenTransferProxy"].as_str().unwrap_or("").to_string(),
            contract_method: v["contractMethod"].as_str().unwrap_or("").to_string(),
            src_usd: v["srcUSD"].as_str().unwrap_or("0").to_string(),
            dest_usd: v["destUSD"].as_str().unwrap_or("0").to_string(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct VeloraRouteSegment {
    pub percent: f64,
    pub swaps: Vec<VeloraSwapDetail>,
}

#[derive(Debug, Clone)]
pub struct VeloraSwapDetail {
    pub src_token: String,
    pub dest_token: String,
    pub exchange: String,
    pub src_amount: String,
    pub dest_amount: String,
    pub percent: f64,
}

#[derive(Debug, Clone)]
pub struct VeloraTxParams {
    pub from: String,
    pub to: String,
    pub value: String,
    pub data: String,
    pub gas_price: String,
    pub chain_id: u64,
}

impl VeloraTxParams {
    pub fn from_value(v: Value) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Ok(Self {
            from: v["from"].as_str().unwrap_or("").to_string(),
            to: v["to"].as_str().unwrap_or("").to_string(),
            value: v["value"].as_str().unwrap_or("0").to_string(),
            data: v["data"].as_str().unwrap_or("0x").to_string(),
            gas_price: v["gasPrice"].as_str().unwrap_or("0").to_string(),
            chain_id: v["chainId"].as_u64().unwrap_or(0),
        })
    }
}

#[derive(Debug, Clone)]
pub struct VeloraTxResponse {
    pub from: String,
    pub to: String,
    pub value: String,
    pub data: String,
    pub gas_price: String,
    pub gas: String,
    pub chain_id: u64,
}

impl VeloraTxResponse {
    pub fn from_value(v: Value) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Ok(Self {
            from: v["from"].as_str().unwrap_or("").to_string(),
            to: v["to"].as_str().unwrap_or("").to_string(),
            value: v["value"].as_str().unwrap_or("0").to_string(),
            data: v["data"].as_str().unwrap_or("0x").to_string(),
            gas_price: v["gasPrice"].as_str().unwrap_or("0").to_string(),
            gas: v["gas"].as_str().unwrap_or("0").to_string(),
            chain_id: v["chainId"].as_u64().unwrap_or(0),
        })
    }
}

#[derive(Debug, Clone)]
pub struct TokenInfo {
    pub address: String,
    pub symbol: String,
    pub decimals: u8,
}
