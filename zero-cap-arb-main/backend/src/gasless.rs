//! ERC-4337 gasless execution via Pimlico bundler + paymaster.
//!
//! The entire execution pipeline is gasless:
//!   1. Build a UserOperation whose callData invokes ZeroRiskArb.execute()
//!   2. Ask Pimlico's verifying paymaster to sponsor gas (pm_sponsorUserOperation)
//!   3. Sign the UserOp with the EOA owner's ECDSA key
//!   4. Submit via eth_sendUserOperation to Pimlico's bundler
//!   5. Poll eth_getUserOperationReceipt for on-chain confirmation
//!
//! No native balance is required in the wallet — Pimlico pays gas, the flash
//! loan provides capital, and profit is returned atomically.

use crate::pimlico_client::PimlicoClient;
use alloy::primitives::keccak256;
use k256::ecdsa::{signature::hazmat::PrehashSigner, SigningKey};
use serde_json::{json, Value};
use tracing::{info, warn};

// ─── Well-known addresses ─────────────────────────────────────────────

/// EntryPoint v0.7 — deployed on all EVM chains.
pub const ENTRY_POINT_V07: &str = "0x0000000071727De22E5E9d8BAf0edAc6f37da032";

/// SimpleAccountFactory v0.7 — deploys deterministic SimpleAccount instances.
/// Deployed on Ethereum, Arbitrum, Optimism, Polygon, BSC, Avalanche, Base,
/// Celo, Gnosis, Linea and most other EVM chains.
pub const SIMPLE_ACCOUNT_FACTORY: &str = "0x91E60e0613810449d098b0b5Ec8b51A0FE8c8985";

// ─── ABI selectors ────────────────────────────────────────────────────

/// SimpleAccountFactory.createAccount(address owner, uint256 salt) → address
fn encode_create_account(owner: &str, salt: u64) -> String {
    // selector: keccak256("createAccount(address,uint256)")[:4] = 0x5fbfb9cf
    let owner_hex = owner.trim_start_matches("0x").to_lowercase();
    format!(
        "0x5fbfb9cf{:0>64}{:0>64}",
        owner_hex,
        format!("{:x}", salt)
    )
}

/// SimpleAccount.execute(address dest, uint256 value, bytes calldata func)
fn encode_execute(dest: &str, value: u128, func_data: &str) -> String {
    // selector: keccak256("execute(address,uint256,bytes)")[:4] = 0xb61d27f6
    let dest_hex = dest.trim_start_matches("0x").to_lowercase();
    let func_bytes = hex::decode(func_data.trim_start_matches("0x"))
        .unwrap_or_default();
    let func_len = func_bytes.len();

    // ABI encode: dest (32 bytes) + value (32 bytes) + offset (32 bytes) + len (32 bytes) + data (padded)
    let padded_len = ((func_len + 31) / 32) * 32;
    let mut padded = func_bytes.clone();
    padded.resize(padded_len, 0);

    format!(
        "0xb61d27f6{:0>64}{:0>64}{:0>64}{:0>64}{}",
        dest_hex,
        format!("{:x}", value),
        format!("{:x}", 96u64), // offset to bytes data = 3 * 32
        format!("{:x}", func_len),
        hex::encode(&padded)
    )
}

/// ZeroRiskArb.execute(uint8 source, address asset, uint256 amount,
///                     uint256 minProfit, bytes calldata swapData,
///                     bool flashbots, address v3Pool)
pub fn encode_arb_execute(
    source: u8,
    asset: &str,
    amount: &str,       // decimal string in wei
    min_profit: &str,   // decimal string in wei
    swap_data: &str,    // hex-encoded Velora calldata
    flashbots: bool,
    v3_pool: &str,
) -> String {
    // selector: first 4 bytes of keccak256("execute(uint8,address,uint256,uint256,bytes,bool,address)")
    let selector = "0xd73b7e0e"; // precomputed
    let asset_hex = asset.trim_start_matches("0x");
    let v3_hex = v3_pool.trim_start_matches("0x");
    let swap_bytes = hex::decode(swap_data.trim_start_matches("0x"))
        .unwrap_or_default();

    let amount_u256 = amount.parse::<u128>().unwrap_or(0);
    let min_profit_u256 = min_profit.parse::<u128>().unwrap_or(0);

    let swap_len = swap_bytes.len();
    let padded_len = ((swap_len + 31) / 32) * 32;
    let mut padded = swap_bytes;
    padded.resize(padded_len, 0);

    // Fixed params: source(32) + asset(32) + amount(32) + minProfit(32) +
    //               offset_swapData(32) + flashbots(32) + v3Pool(32) = 7 * 32 = 224
    // Then: length(32) + padded_data
    let offset_swap = 7u64 * 32; // byte offset to start of swapData

    format!(
        "{}{:0>64}{:0>64}{:0>64}{:0>64}{:0>64}{:0>64}{:0>64}{:0>64}{}",
        selector,
        format!("{:x}", source),
        asset_hex,
        format!("{:x}", amount_u256),
        format!("{:x}", min_profit_u256),
        format!("{:x}", offset_swap),
        format!("{:x}", if flashbots { 1u8 } else { 0u8 }),
        v3_hex,
        format!("{:x}", swap_len),
        hex::encode(&padded)
    )
}

/// One leg of a direct route for `ZeroRiskArb.executeDirect`, matching the
/// contract's `SwapLeg` struct.
pub struct SwapLegInput {
    /// Router or pool address the leg calls.
    pub target: String,
    /// Token the leg spends.
    pub token_in: String,
    /// true → transfer tokenIn to target, then call `data` (pair.swap);
    /// false → approve tokenIn to target (must be whitelisted), then call.
    pub to_pool: bool,
    /// Raw amount for toPool legs; 0 sends the contract's full balance.
    pub amount_in: String,
    /// Hex-encoded calldata for `target`.
    pub data: String,
}

/// abi.encode(SwapLeg[]) — the `swapData` payload for executeDirect.
/// Layout: 0x20 offset, length, then per-element heads (offsets relative to
/// the head region) followed by each element's (target, tokenIn, toPool,
/// amountIn, bytes-tail).
pub fn encode_swap_legs(legs: &[SwapLegInput]) -> String {
    let n = legs.len();
    // Each element: 5 head words + 1 len word + padded data.
    let elem_size = |l: &SwapLegInput| {
        let d = hex::decode(l.data.trim_start_matches("0x")).unwrap_or_default();
        5 * 32 + 32 + (d.len() + 31) / 32 * 32
    };
    let heads = n * 32;

    let mut out = String::with_capacity(64 + 64 + n * 64);
    out.push_str(&format!("{:0>64}", format!("{:x}", 32u64))); // array offset
    out.push_str(&format!("{:0>64}", format!("{:x}", n)));

    // Element head offsets, relative to the region right after the length.
    let mut off = heads;
    for l in legs {
        out.push_str(&format!("{:0>64}", format!("{:x}", off)));
        off += elem_size(l);
    }
    // Element tails.
    for l in legs {
        let data = hex::decode(l.data.trim_start_matches("0x")).unwrap_or_default();
        let mut padded = data.clone();
        padded.resize((data.len() + 31) / 32 * 32, 0);
        out.push_str(&format!("{:0>64}",
            l.target.trim_start_matches("0x").to_lowercase()));
        out.push_str(&format!("{:0>64}",
            l.token_in.trim_start_matches("0x").to_lowercase()));
        out.push_str(&format!("{:0>64}", if l.to_pool { 1 } else { 0 }));
        out.push_str(&format!("{:0>64}", format!("{:x}",
            l.amount_in.parse::<u128>().unwrap_or(0))));
        out.push_str(&format!("{:0>64}", format!("{:x}", 160u64))); // bytes offset
        out.push_str(&format!("{:0>64}", format!("{:x}", data.len())));
        out.push_str(&hex::encode(&padded));
    }
    out
}

/// ZeroRiskArb.executeDirect(uint8 source, address asset, uint256 amount,
///                           uint256 minProfit, bytes swapData, address v3Pool)
pub fn encode_arb_execute_direct(
    source: u8,
    asset: &str,
    amount: &str,      // decimal string in wei
    min_profit: &str,  // decimal string in wei
    legs_data: &str,   // hex-encoded abi.encode(SwapLeg[])
    v3_pool: &str,
) -> String {
    // keccak256("executeDirect(uint8,address,uint256,uint256,bytes,address)")
    let selector = "0x6c8cac5a";
    let asset_hex = asset.trim_start_matches("0x").to_lowercase();
    let v3_hex = v3_pool.trim_start_matches("0x").to_lowercase();
    let legs = hex::decode(legs_data.trim_start_matches("0x")).unwrap_or_default();
    let padded_len = (legs.len() + 31) / 32 * 32;
    let mut padded = legs.clone();
    padded.resize(padded_len, 0);

    // 6 head words: source, asset, amount, minProfit, offset(=192), v3Pool
    let offset_swap = 6u64 * 32;
    format!(
        "{}{:0>64}{:0>64}{:0>64}{:0>64}{:0>64}{:0>64}{:0>64}{}",
        selector,
        format!("{:x}", source),
        asset_hex,
        format!("{:x}", amount.parse::<u128>().unwrap_or(0)),
        format!("{:x}", min_profit.parse::<u128>().unwrap_or(0)),
        format!("{:x}", offset_swap),
        v3_hex,
        format!("{:x}", legs.len()),
        hex::encode(&padded)
    )
}

/// Uniswap-V3 `pool.swap(address recipient, bool zeroForOne,
///                       int256 amountSpecified, uint160 sqrtPriceLimitX96,
///                       bytes data)` — selector 0x128acb08.
///
/// ZeroRiskArb detects this selector on `toPool` legs and pays the owed
/// input inside `uniswapV3SwapCallback` instead of pre-transferring —
/// pre-transferring would double-pay since the pool collects via callback.
/// `callback_data` is passed through to the callback verbatim; the contract
/// expects `abi.encode(address tokenIn)` so it knows which token to send.
/// `amount_specified` is signed int256: >0 = exact input, <0 = exact output.
/// `sqrt_price_limit_x96` is a decimal string because uint160 exceeds u128.
pub fn encode_v3_pool_swap(
    recipient: &str,
    zero_for_one: bool,
    amount_specified: i128,
    sqrt_price_limit_x96: &str,
    callback_data: &str,
) -> String {
    let recip = recipient.trim_start_matches("0x").to_lowercase();
    // int256 → 32-byte two's complement.
    let amt_hex = if amount_specified >= 0 {
        format!("{:0>64}", format!("{:x}", amount_specified as u128))
    } else {
        let mag = (-amount_specified) as u128;
        format!("ffffffffffffffffffffffffffffffff{:032x}", mag.wrapping_neg())
    };
    let limit = alloy::primitives::U256::from_str_radix(sqrt_price_limit_x96.trim(), 10)
        .unwrap_or(alloy::primitives::U256::ZERO);
    let cb = hex::decode(callback_data.trim_start_matches("0x")).unwrap_or_default();
    let padded_len = (cb.len() + 31) / 32 * 32;
    let mut padded = cb.clone();
    padded.resize(padded_len, 0);

    format!(
        "{}{:0>64}{:0>64}{}{:0>64}{:0>64}{:0>64}{}",
        "0x128acb08",
        recip,
        format!("{:x}", if zero_for_one { 1u8 } else { 0u8 }),
        amt_hex,
        format!("{:x}", limit),
        format!("{:x}", 160u64), // bytes data offset = 5 * 32
        format!("{:x}", cb.len()),
        hex::encode(&padded)
    )
}

/// abi.encode(address) — a single left-padded word, used as V3 callback data.
pub fn encode_address_word(addr: &str) -> String {
    format!("{:0>64}", addr.trim_start_matches("0x").to_lowercase())
}

/// Uniswap-V2-style `pair.swap(uint256 amount0Out, uint256 amount1Out,
///                            address to, bytes data)`.
pub fn encode_v2_pair_swap(amount0_out: u128, amount1_out: u128, to: &str) -> String {
    let to_hex = to.trim_start_matches("0x").to_lowercase();
    format!(
        "{}{:0>64}{:0>64}{:0>64}{:0>64}{:0>64}",
        "0x022c0d9f",
        format!("{:x}", amount0_out),
        format!("{:x}", amount1_out),
        to_hex,
        format!("{:x}", 128u64), // offset to empty bytes
        format!("{:x}", 0u64)
    )
}

// ─── Smart Account address derivation ─────────────────────────────────

/// Derive the counterfactual SimpleAccount address for an EOA owner.
/// This uses CREATE2: keccak256(0xff ++ factory ++ salt ++ keccak256(initCode))
/// The exact address is deterministic and does not require deployment.
pub fn smart_account_address(owner_addr: &str) -> String {
    // For now, return a placeholder — the actual address is computed by calling
    // SimpleAccountFactory.getAddress(owner, 0) on-chain, or by Pimlico's
    // getSenderAddress. We'll compute it during the first UserOp submission.
    format!("SA:{}", owner_addr)
}

/// Get the owner's address from the private key.
pub fn owner_address_from_key(private_key: &str) -> Result<String, String> {
    let pk_hex = private_key.trim_start_matches("0x");
    let pk_bytes = hex::decode(pk_hex)
        .map_err(|e| format!("invalid private key hex: {e}"))?;
    if pk_bytes.len() != 32 {
        return Err(format!("private key must be 32 bytes, got {}", pk_bytes.len()));
    }
    let signing_key = SigningKey::from_bytes((&pk_bytes[..]).into())
        .map_err(|e| format!("invalid signing key: {e}"))?;
    let verifying_key = signing_key.verifying_key();
    let public_key_bytes = verifying_key.to_encoded_point(false);
    let public_key_uncompressed = &public_key_bytes.as_bytes()[1..];
    let hash = keccak256(public_key_uncompressed);
    Ok(format!("0x{}", hex::encode(&hash[12..])))
}

// ─── UserOperation builder ─────────────────────────────────────────────

/// Build a gasless UserOperation for a ZeroRiskArb.execute() call.
///
/// The UserOp targets the SimpleAccount which calls ZeroRiskArb.execute().
/// Gas is sponsored by Pimlico's verifying paymaster — no native balance needed.
pub async fn build_and_send_userop(
    pimlico: &PimlicoClient,
    chain_id: u64,
    private_key: &str,
    arb_contract: &str,
    arb_calldata: &str,
) -> Result<GaslessResult, String> {
    if !pimlico.is_configured() {
        return Err("Pimlico API key not configured — gasless execution unavailable".to_string());
    }

    let owner_addr = owner_address_from_key(private_key)?;

    // Build factory + factoryData for first-time deployment
    let factory_data = encode_create_account(&owner_addr, 0);

    // Build callData: SimpleAccount.execute(arbContract, 0, arbCalldata)
    let call_data = encode_execute(arb_contract, 0, arb_calldata);

    // Step 1: Get gas prices from Pimlico
    let gas_prices = pimlico.gas_price(chain_id).await?;
    let fast = &gas_prices["fast"];
    let max_fee = fast["maxFeePerGas"]
        .as_str()
        .unwrap_or("0x59682f00");
    let max_priority = fast["maxPriorityFeePerGas"]
        .as_str()
        .unwrap_or("0x59682f00");

    // Step 2: Build the unsigned UserOperation
    // nonce = 0 for first-time account (will be overridden if account exists)
    let user_op = json!({
        "sender": "0x0000000000000000000000000000000000000000", // will be filled by getSenderAddress
        "nonce": "0x0",
        "factory": SIMPLE_ACCOUNT_FACTORY,
        "factoryData": factory_data,
        "callData": call_data,
        "callGasLimit": "0x0",
        "verificationGasLimit": "0x0",
        "preVerificationGas": "0x0",
        "maxFeePerGas": max_fee,
        "maxPriorityFeePerGas": max_priority,
        "signature": "0xfffffffffffffffffffffffffffffff0000000000000000000000000000000007aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa1c"
    });

    // Step 3: Compute sender address via getSenderAddress
    let sender = compute_sender_address(pimlico, chain_id, &user_op).await?;

    // Check if account is already deployed
    let account_deployed = is_account_deployed(chain_id, &sender).await;

    let mut user_op = user_op;
    user_op["sender"] = json!(sender);

    // If account already deployed, remove factory fields
    if account_deployed {
        user_op.as_object_mut().unwrap().remove("factory");
        user_op.as_object_mut().unwrap().remove("factoryData");
        // Get nonce from EntryPoint
        let nonce = get_account_nonce(pimlico, chain_id, &sender).await
            .unwrap_or("0x0".to_string());
        user_op["nonce"] = json!(nonce);
    }

    // Step 4: Sponsor the UserOperation via Pimlico paymaster
    info!(
        chain_id = chain_id,
        sender = %sender,
        "requesting Pimlico paymaster sponsorship"
    );
    let sponsor_result = pimlico
        .sponsor_user_operation(chain_id, &user_op, ENTRY_POINT_V07)
        .await?;

    // Apply sponsor result fields
    if let Some(obj) = sponsor_result.as_object() {
        for (k, v) in obj {
            user_op[k] = v.clone();
        }
    }

    // Step 5: Sign the UserOperation
    let signature = sign_user_operation(&user_op, chain_id, private_key)?;
    user_op["signature"] = json!(signature);

    // Step 6: Submit to bundler
    info!(
        chain_id = chain_id,
        sender = %sender,
        "submitting gasless UserOperation to Pimlico bundler"
    );
    let user_op_hash = pimlico
        .send_user_operation(chain_id, &user_op, ENTRY_POINT_V07)
        .await?;

    info!(
        chain_id = chain_id,
        user_op_hash = %user_op_hash,
        "UserOperation submitted — polling for receipt"
    );

    // Step 7: Poll for receipt (up to 120 seconds)
    let receipt = poll_user_op_receipt(pimlico, chain_id, &user_op_hash, 120).await?;

    let tx_hash = receipt["receipt"]["transactionHash"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let success = receipt["success"].as_bool().unwrap_or(false);

    Ok(GaslessResult {
        user_op_hash,
        tx_hash,
        success,
        sender,
        chain_id,
    })
}

/// Deploy a contract gaslessly via CREATE2 through the smart account.
pub async fn deploy_contract_gasless(
    pimlico: &PimlicoClient,
    chain_id: u64,
    private_key: &str,
    creation_bytecode: &str,
) -> Result<GaslessResult, String> {
    if !pimlico.is_configured() {
        return Err("Pimlico API key not configured".to_string());
    }

    let owner_addr = owner_address_from_key(private_key)?;
    let factory_data = encode_create_account(&owner_addr, 0);

    // callData = execute(address(0), 0, creation_bytecode)
    // When dest=address(0) and value includes CREATE2 salt, SimpleAccount
    // forwards the call which deploys the contract.
    // Actually, for deployment we need the SmartAccount to use CREATE or CREATE2.
    // SimpleAccount.execute() with dest=0x can deploy:
    // We'll use executeBatch or a custom deploy helper.
    // Simpler: wrap the deploy bytecode as a self-deploying tx.
    let call_data = encode_execute(
        "0x0000000000000000000000000000000000000000",
        0,
        creation_bytecode,
    );

    let gas_prices = pimlico.gas_price(chain_id).await?;
    let fast = &gas_prices["fast"];

    let user_op = json!({
        "sender": "0x0000000000000000000000000000000000000000",
        "nonce": "0x0",
        "factory": SIMPLE_ACCOUNT_FACTORY,
        "factoryData": factory_data,
        "callData": call_data,
        "callGasLimit": "0x0",
        "verificationGasLimit": "0x0",
        "preVerificationGas": "0x0",
        "maxFeePerGas": fast["maxFeePerGas"].as_str().unwrap_or("0x59682f00"),
        "maxPriorityFeePerGas": fast["maxPriorityFeePerGas"].as_str().unwrap_or("0x59682f00"),
        "signature": "0xfffffffffffffffffffffffffffffff0000000000000000000000000000000007aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa1c"
    });

    let sender = compute_sender_address(pimlico, chain_id, &user_op).await?;
    let mut user_op = user_op;
    user_op["sender"] = json!(sender);

    let sponsor_result = pimlico
        .sponsor_user_operation(chain_id, &user_op, ENTRY_POINT_V07)
        .await?;
    if let Some(obj) = sponsor_result.as_object() {
        for (k, v) in obj {
            user_op[k] = v.clone();
        }
    }

    let signature = sign_user_operation(&user_op, chain_id, private_key)?;
    user_op["signature"] = json!(signature);

    let user_op_hash = pimlico
        .send_user_operation(chain_id, &user_op, ENTRY_POINT_V07)
        .await?;

    let receipt = poll_user_op_receipt(pimlico, chain_id, &user_op_hash, 120).await?;
    let tx_hash = receipt["receipt"]["transactionHash"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let success = receipt["success"].as_bool().unwrap_or(false);

    Ok(GaslessResult {
        user_op_hash,
        tx_hash,
        success,
        sender,
        chain_id,
    })
}

// ─── Result type ──────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct GaslessResult {
    pub user_op_hash: String,
    pub tx_hash: String,
    pub success: bool,
    pub sender: String,
    pub chain_id: u64,
}

// ─── Internal helpers ─────────────────────────────────────────────────

/// Compute the counterfactual sender address from factory + factoryData
/// by calling eth_call to EntryPoint.getSenderAddress(initCode).
async fn compute_sender_address(
    pimlico: &PimlicoClient,
    chain_id: u64,
    user_op: &Value,
) -> Result<String, String> {
    // The bundler's eth_estimateUserOperationGas or a direct RPC
    // getSenderAddress call can derive the address. Pimlico fills in the
    // sender if we use their estimation endpoint.
    //
    // Alternative: use the factory + factoryData to compute CREATE2 address.
    // For SimpleAccountFactory, the address is:
    //   CREATE2(factory, salt, keccak256(proxyBytecode ++ implementation ++ initData))
    //
    // We use the Pimlico estimation endpoint which returns the sender.
    let est = pimlico
        .estimate_user_operation_gas(chain_id, user_op, ENTRY_POINT_V07)
        .await;

    match est {
        Ok(result) => {
            // If estimation succeeds, the sender in the UserOp is valid
            // (the estimate wouldn't pass validation with an incorrect sender).
            // But we need to get the actual sender address from the initCode.
            // Use a direct RPC approach: call getSenderAddress on the EntryPoint.
            get_sender_from_factory(chain_id, user_op).await
        }
        Err(e) => {
            // AA errors like "AA20 account not deployed" contain the address
            // in the error context. Try to extract it.
            warn!("Pimlico estimation error (expected for new accounts): {}", e);
            get_sender_from_factory(chain_id, user_op).await
        }
    }
}

/// Call SimpleAccountFactory.getAddress(owner, salt) to compute the sender.
async fn get_sender_from_factory(
    chain_id: u64,
    user_op: &Value,
) -> Result<String, String> {
    let factory_data = user_op["factoryData"]
        .as_str()
        .unwrap_or("");
    if factory_data.len() < 10 {
        return Err("No factoryData in UserOp".to_string());
    }

    // Extract owner and salt from createAccount(address,uint256) calldata
    let data_hex = factory_data.trim_start_matches("0x");
    if data_hex.len() < 8 + 128 {
        return Err("factoryData too short".to_string());
    }
    // Skip selector (8 chars = 4 bytes), owner is next 64 chars (32 bytes, last 40 are address)
    let owner_padded = &data_hex[8..72];
    let owner = format!("0x{}", &owner_padded[24..]);
    // salt is next 64 chars
    let salt_hex = &data_hex[72..136];
    let salt = u64::from_str_radix(salt_hex.trim_start_matches('0'), 16).unwrap_or(0);

    // Call getAddress(address,uint256) on the factory via any chain RPC
    let get_addr_selector = "0x8cb84e18"; // keccak256("getAddress(address,uint256)")[:4]
    let call_data = format!(
        "{}{:0>64}{:0>64}",
        get_addr_selector,
        owner.trim_start_matches("0x"),
        format!("{:x}", salt)
    );

    let chain = crate::chains::get_chains()
        .into_iter()
        .find(|c| c.id == chain_id);

    let rpc_url = chain
        .map(|c| c.rpc_url.clone())
        .unwrap_or_default();

    if rpc_url.is_empty() {
        return Err(format!("No RPC URL for chain {}", chain_id));
    }

    let client = reqwest::Client::new();
    let body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "eth_call",
        "params": [{
            "to": SIMPLE_ACCOUNT_FACTORY,
            "data": call_data,
        }, "latest"]
    });

    let resp = client
        .post(&rpc_url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("getAddress RPC call failed: {e}"))?;

    let raw: Value = resp.json().await
        .map_err(|e| format!("getAddress parse failed: {e}"))?;

    if let Some(err) = raw.get("error") {
        return Err(format!("getAddress reverted: {:?}", err));
    }

    let result = raw["result"]
        .as_str()
        .unwrap_or("");

    if result.len() < 66 {
        return Err(format!("getAddress returned unexpected: {result}"));
    }

    // Result is a 32-byte ABI-encoded address — last 40 hex chars
    let addr_hex = &result.trim_start_matches("0x");
    let address = format!("0x{}", &addr_hex[24..]);
    Ok(address)
}

/// Check if an account is already deployed (has code).
async fn is_account_deployed(chain_id: u64, address: &str) -> bool {
    let chain = crate::chains::get_chains()
        .into_iter()
        .find(|c| c.id == chain_id);
    let rpc_url = match chain {
        Some(c) => c.rpc_url.clone(),
        None => return false,
    };
    let client = reqwest::Client::new();
    let body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "eth_getCode",
        "params": [address, "latest"]
    });
    let resp = client.post(&rpc_url).json(&body).send().await;
    match resp {
        Ok(r) => {
            let raw: Value = r.json().await.unwrap_or_default();
            let code = raw["result"].as_str().unwrap_or("0x");
            code != "0x" && code != "0x0"
        }
        Err(_) => false,
    }
}

/// Get the nonce for an already-deployed account from the EntryPoint.
async fn get_account_nonce(
    pimlico: &PimlicoClient,
    chain_id: u64,
    sender: &str,
) -> Result<String, String> {
    // Use EntryPoint.getNonce(sender, 0) via RPC
    let chain = crate::chains::get_chains()
        .into_iter()
        .find(|c| c.id == chain_id)
        .ok_or("chain not found")?;

    let client = reqwest::Client::new();
    // getNonce(address sender, uint192 key) → uint256
    let selector = "0x35567e1a";
    let call_data = format!(
        "{}{:0>64}{:0>64}",
        selector,
        sender.trim_start_matches("0x"),
        "0" // key = 0
    );

    let body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "eth_call",
        "params": [{
            "to": ENTRY_POINT_V07,
            "data": call_data,
        }, "latest"]
    });

    let resp = client
        .post(&chain.rpc_url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("getNonce failed: {e}"))?;

    let raw: Value = resp.json().await
        .map_err(|e| format!("getNonce parse failed: {e}"))?;

    let result = raw["result"]
        .as_str()
        .unwrap_or("0x0")
        .to_string();

    Ok(result)
}

/// Sign a UserOperation for EntryPoint v0.7.
///
/// The hash is: keccak256(abi.encode(userOpHash, entryPoint, chainId))
/// where userOpHash = keccak256(packUserOp(userOp))
fn sign_user_operation(
    user_op: &Value,
    chain_id: u64,
    private_key: &str,
) -> Result<String, String> {
    let pk_hex = private_key.trim_start_matches("0x");
    let pk_bytes = hex::decode(pk_hex)
        .map_err(|e| format!("invalid private key: {e}"))?;
    let signing_key = SigningKey::from_bytes((&pk_bytes[..]).into())
        .map_err(|e| format!("invalid signing key: {e}"))?;

    // Compute userOpHash per EIP-4337 v0.7:
    // 1. Pack the UserOp fields
    // 2. Hash the packed data
    // 3. Hash with entryPoint address and chainId

    let packed = pack_user_op_v07(user_op)?;
    let inner_hash = keccak256(&packed);

    // Final hash: keccak256(abi.encode(innerHash, entryPoint, chainId))
    let entry_point_bytes = hex::decode(ENTRY_POINT_V07.trim_start_matches("0x"))
        .map_err(|e| format!("invalid entrypoint: {e}"))?;

    let mut encode_data = Vec::new();
    encode_data.extend_from_slice(inner_hash.as_ref());
    // Pad entryPoint to 32 bytes
    encode_data.extend_from_slice(&[0u8; 12]);
    encode_data.extend_from_slice(&entry_point_bytes);
    // Pad chainId to 32 bytes
    let chain_bytes = chain_id.to_be_bytes();
    encode_data.extend_from_slice(&[0u8; 24]);
    encode_data.extend_from_slice(&chain_bytes);

    let final_hash = keccak256(&encode_data);

    // Sign with Ethereum personal_sign prefix
    let prefixed_msg = format!(
        "\x19Ethereum Signed Message:\n32{}",
        String::from_utf8_lossy(final_hash.as_ref())
    );
    // Actually for ERC-4337, the hash is signed directly (no prefix)
    let (sig, recovery_id) = signing_key
        .sign_prehash(final_hash.as_ref())
        .map_err(|e| format!("signing failed: {e}"))?;

    let sig_bytes = sig.to_bytes();
    let r = &sig_bytes[..32];
    let s = &sig_bytes[32..64];
    let v = 27 + recovery_id.to_byte();

    let mut full_sig = Vec::with_capacity(65);
    full_sig.extend_from_slice(r);
    full_sig.extend_from_slice(s);
    full_sig.push(v);

    Ok(format!("0x{}", hex::encode(&full_sig)))
}

/// Pack a v0.7 UserOperation for hashing.
fn pack_user_op_v07(user_op: &Value) -> Result<Vec<u8>, String> {
    let sender = hex::decode(
        user_op["sender"].as_str().unwrap_or("").trim_start_matches("0x")
    ).map_err(|e| format!("bad sender: {e}"))?;

    let nonce_hex = user_op["nonce"].as_str().unwrap_or("0x0");
    let nonce = u128::from_str_radix(
        nonce_hex.trim_start_matches("0x"),
        16,
    ).unwrap_or(0);

    // Hash initCode (factory + factoryData) or empty
    let init_code_hash = if let Some(factory) = user_op["factory"].as_str() {
        let factory_data = user_op["factoryData"].as_str().unwrap_or("");
        let mut init_code = hex::decode(factory.trim_start_matches("0x"))
            .unwrap_or_default();
        init_code.extend_from_slice(
            &hex::decode(factory_data.trim_start_matches("0x")).unwrap_or_default()
        );
        keccak256(&init_code)
    } else {
        keccak256(&[])
    };

    let call_data = hex::decode(
        user_op["callData"].as_str().unwrap_or("0x").trim_start_matches("0x")
    ).unwrap_or_default();
    let call_data_hash = keccak256(&call_data);

    let call_gas = parse_hex_u128(user_op["callGasLimit"].as_str().unwrap_or("0x0"));
    let ver_gas = parse_hex_u128(user_op["verificationGasLimit"].as_str().unwrap_or("0x0"));
    let pre_ver_gas = parse_hex_u128(user_op["preVerificationGas"].as_str().unwrap_or("0x0"));
    let max_fee = parse_hex_u128(user_op["maxFeePerGas"].as_str().unwrap_or("0x0"));
    let max_priority = parse_hex_u128(user_op["maxPriorityFeePerGas"].as_str().unwrap_or("0x0"));

    // Pack accountGasLimits = verificationGasLimit (16 bytes) || callGasLimit (16 bytes)
    let account_gas_limits = pack_two_u128(ver_gas, call_gas);
    // Pack gasFees = maxPriorityFeePerGas (16 bytes) || maxFeePerGas (16 bytes)
    let gas_fees = pack_two_u128(max_priority, max_fee);

    // Hash paymasterAndData
    let paymaster_hash = if let Some(pm) = user_op["paymaster"].as_str() {
        let pm_ver_gas = parse_hex_u128(user_op["paymasterVerificationGasLimit"].as_str().unwrap_or("0x0"));
        let pm_post_gas = parse_hex_u128(user_op["paymasterPostOpGasLimit"].as_str().unwrap_or("0x0"));
        let pm_data = user_op["paymasterData"].as_str().unwrap_or("");

        let mut paymaster_and_data = hex::decode(pm.trim_start_matches("0x"))
            .unwrap_or_default();
        // Pack gas limits as 16-byte each
        paymaster_and_data.extend_from_slice(&pm_ver_gas.to_be_bytes()[..]);
        paymaster_and_data.extend_from_slice(&pm_post_gas.to_be_bytes()[..]);
        paymaster_and_data.extend_from_slice(
            &hex::decode(pm_data.trim_start_matches("0x")).unwrap_or_default()
        );
        keccak256(&paymaster_and_data)
    } else {
        keccak256(&[])
    };

    // ABI encode: sender(32) + nonce(32) + initCodeHash(32) + callDataHash(32)
    //           + accountGasLimits(32) + preVerificationGas(32) + gasFees(32) + paymasterHash(32)
    let mut packed = Vec::with_capacity(256);

    // sender — left-padded to 32 bytes
    packed.extend_from_slice(&[0u8; 12]);
    packed.extend_from_slice(&sender);

    // nonce — 32 bytes
    packed.extend_from_slice(&[0u8; 16]);
    packed.extend_from_slice(&nonce.to_be_bytes());

    // initCodeHash — 32 bytes
    packed.extend_from_slice(init_code_hash.as_ref());

    // callDataHash — 32 bytes
    packed.extend_from_slice(call_data_hash.as_ref());

    // accountGasLimits — 32 bytes (verGas || callGas)
    packed.extend_from_slice(&account_gas_limits);

    // preVerificationGas — 32 bytes
    packed.extend_from_slice(&[0u8; 16]);
    packed.extend_from_slice(&pre_ver_gas.to_be_bytes());

    // gasFees — 32 bytes (maxPriority || maxFee)
    packed.extend_from_slice(&gas_fees);

    // paymasterAndDataHash — 32 bytes
    packed.extend_from_slice(paymaster_hash.as_ref());

    Ok(packed)
}

fn parse_hex_u128(s: &str) -> u128 {
    let trimmed = s.trim_start_matches("0x");
    if trimmed.is_empty() { return 0; }
    u128::from_str_radix(trimmed, 16).unwrap_or(0)
}

fn pack_two_u128(high: u128, low: u128) -> [u8; 32] {
    let mut out = [0u8; 32];
    out[..16].copy_from_slice(&high.to_be_bytes());
    out[16..].copy_from_slice(&low.to_be_bytes());
    out
}

/// Poll for a UserOperation receipt, retrying every 3 seconds.
async fn poll_user_op_receipt(
    pimlico: &PimlicoClient,
    chain_id: u64,
    user_op_hash: &str,
    timeout_secs: u64,
) -> Result<Value, String> {
    let start = std::time::Instant::now();
    let poll_interval = std::time::Duration::from_secs(3);

    loop {
        if start.elapsed().as_secs() > timeout_secs {
            return Err(format!(
                "UserOp {} not confirmed after {}s",
                user_op_hash, timeout_secs
            ));
        }

        match pimlico
            .user_operation_receipt(chain_id, user_op_hash, ENTRY_POINT_V07)
            .await
        {
            Ok(receipt) if !receipt.is_null() => return Ok(receipt),
            Ok(_) => {} // null = not yet mined
            Err(e) => {
                // Some errors mean "not found yet"
                if !e.contains("could not find") && !e.contains("not found") {
                    warn!("receipt poll error: {}", e);
                }
            }
        }

        tokio::time::sleep(poll_interval).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_owner_address_from_key() {
        // Known test vector: private key → address
        let pk = "019ea201e7c9759554119f3ca5dd0a8abf659be1f75d672a1760faeece78251a";
        let addr = owner_address_from_key(pk).unwrap();
        assert_eq!(
            addr.to_lowercase(),
            "0x2ef34d88ec4ebbd5543fff2784d5adbc01f14d56"
        );
    }

    #[test]
    fn test_encode_create_account() {
        let result = encode_create_account(
            "0x2eF34d88EC4EBBd5543fFF2784D5AdbC01f14D56",
            0,
        );
        assert!(result.starts_with("0x5fbfb9cf"));
        assert!(result.contains("2ef34d88ec4ebbd5543fff2784d5adbc01f14d56"));
    }

    #[test]
    fn test_encode_execute() {
        let result = encode_execute(
            "0xdead000000000000000000000000000000000001",
            0,
            "0xaabbccdd",
        );
        assert!(result.starts_with("0xb61d27f6"));
    }

    #[test]
    fn test_parse_hex() {
        assert_eq!(parse_hex_u128("0x59682f00"), 0x59682f00);
        assert_eq!(parse_hex_u128("0x0"), 0);
        assert_eq!(parse_hex_u128("0x"), 0);
    }

    #[test]
    fn test_pack_two_u128() {
        let packed = pack_two_u128(1, 2);
        assert_eq!(packed[15], 1);
        assert_eq!(packed[31], 2);
    }

    #[test]
    fn test_encode_v2_pair_swap() {
        // swap(uint256,uint256,address,bytes) → selector 0x022c0d9f.
        let data = encode_v2_pair_swap(0, 1_000, "0x2eF34d88EC4EBBd5543fFF2784D5AdbC01f14D56");
        assert!(data.starts_with("0x022c0d9f"));
        // to-address is the third word.
        let word3 = &data[10 + 128..10 + 192];
        assert_eq!(
            word3,
            "0000000000000000000000002ef34d88ec4ebbd5543fff2784d5adbc01f14d56"
        );
        // bytes offset = 0x80 (128 decimal) as a 32-byte word, empty tail.
        assert!(data.contains(&format!("{:0>64x}", 128u64)));
        assert!(data.ends_with(&format!("{:0>64x}", 0u64)));
    }

    #[test]
    fn test_encode_v3_pool_swap() {
        // swap(address,bool,int256,uint160,bytes) → selector 0x128acb08.
        let data = encode_v3_pool_swap(
            "0x00000000000000000000000000000000000000aa",
            true,
            1_000,
            "4295128740",
            &encode_address_word("0x00000000000000000000000000000000000000bb"),
        );
        assert!(data.starts_with("0x128acb08"));
        // Layout: sel(4B) + 5 head words + len word + one padded data word.
        assert_eq!(data.len(), 2 + (4 + 160 + 32 + 32) * 2);
        // recipient is word0 of the head.
        assert!(data[10..10 + 64].ends_with("00aa"));
        // zeroForOne = 1 at word1.
        assert_eq!(
            &data[10 + 64..10 + 128],
            "0000000000000000000000000000000000000000000000000000000000000001"
        );
        // callback data carries abi.encode(tokenIn) = left-padded address.
        assert!(data.ends_with("000000000000000000000000000000000000000000000000000000bb"));

        // Negative amountSpecified (exact-out) → int256 two's complement.
        let neg = encode_v3_pool_swap(
            "0x00000000000000000000000000000000000000aa",
            false,
            -5,
            "1",
            &encode_address_word("0x00000000000000000000000000000000000000bb"),
        );
        assert!(neg.contains(
            "fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffb"
        ));
    }

    #[test]
    fn test_encode_swap_legs_layout() {
        let legs = vec![
            SwapLegInput {
                target: "0x1111111111111111111111111111111111111111".into(),
                token_in: "0x2222222222222222222222222222222222222222".into(),
                to_pool: true,
                amount_in: "1000".into(),
                data: "0x022c0d9f".into(),
            },
            SwapLegInput {
                target: "0x3333333333333333333333333333333333333333".into(),
                token_in: "0x4444444444444444444444444444444444444444".into(),
                to_pool: false,
                amount_in: "0".into(),
                data: "0x".into(),
            },
        ];
        let enc = encode_swap_legs(&legs);
        let words: Vec<&str> = enc
            .as_bytes()
            .chunks(64)
            .map(|c| std::str::from_utf8(c).unwrap())
            .collect();

        // word0 = 0x20 (array offset), word1 = 2 (length).
        assert_eq!(u128::from_str_radix(words[0], 16).unwrap(), 32);
        assert_eq!(u128::from_str_radix(words[1], 16).unwrap(), 2);
        // Head region starts at word2: two offsets relative to it.
        // element size = 5 heads + 1 len + padded data (4B → 32B) = 224.
        assert_eq!(u128::from_str_radix(words[2], 16).unwrap(), 64);
        assert_eq!(u128::from_str_radix(words[3], 16).unwrap(), 64 + 224);
        // Element 0 tail at word4: target, token_in, toPool=1, amountIn=1000,
        // data offset 160.
        assert!(words[4].ends_with("1111111111111111111111111111111111111111"));
        assert!(words[5].ends_with("2222222222222222222222222222222222222222"));
        assert_eq!(u128::from_str_radix(words[6], 16).unwrap(), 1);
        assert_eq!(u128::from_str_radix(words[7], 16).unwrap(), 1000);
        assert_eq!(u128::from_str_radix(words[8], 16).unwrap(), 160);
    }

    #[test]
    fn test_encode_arb_execute_direct() {
        let calldata = encode_arb_execute_direct(
            6,
            "0xe5D7C2a44FfDDf6b295A15C148167daaAf5Cf34f",
            "1000000000000000000",
            "500000000000000000",
            "0x20", // minimal legs payload
            "0x0000000000000000000000000000000000000001",
        );
        assert!(calldata.starts_with("0x6c8cac5a"));
        // Head words (each 64 hex chars) follow the "0x" + 4-byte selector.
        // word4 = bytes offset = 192; word5 = v3Pool.
        let word4 = &calldata[10 + 4 * 64..10 + 5 * 64];
        let word5 = &calldata[10 + 5 * 64..10 + 6 * 64];
        assert_eq!(u128::from_str_radix(word4, 16).unwrap(), 192);
        assert!(word5.ends_with("000000000000000000000001"));
    }
}
