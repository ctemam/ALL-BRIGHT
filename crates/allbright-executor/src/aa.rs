//! Account-abstraction ops layer — **gasless mode via Pimlico**.
//!
//! The EOA (`PRIVATE_KEY`) owns an ERC-4337 smart account (ZeroDev Kernel
//! v3.1, ECDSA validator). Pimlico supplies:
//!   - the bundler (`eth_sendUserOperation`, `eth_estimateUserOperationGas`,
//!     `eth_getUserOperationReceipt` — via alloy's typed `Erc4337Api` /
//!     `PackedUserOperation`, EP v0.7)
//!   - the verifying paymaster (`pm_sponsorUserOperation`) — gas is
//!     *sponsored*: UserOps execute with zero native-token spend.
//!
//! Vendor-specific calls that remain manual by necessity (no standard
//! ERC-4337 equivalent): `pm_sponsorUserOperation` and
//! `pimlico_getUserOperationGasPrice` (bundler-compatible gas tiers).
//! `pimlico_getUserOperationStatus` is gone — outcome tracking is built
//! on the typed `eth_getUserOperationReceipt` (GAP-18/19/21/26).
//!
//! Scope per AGENTS.md §18: ops + profit transfer only — never the
//! execution hot path.
//!
//! All contract constants below were verified by bytecode presence on-chain
//! before use (tools + aa_probe); none are assumed.

use alloy::dyn_abi::DynSolValue;
use alloy::eips::BlockNumberOrTag;
use alloy::primitives::{hex, keccak256, Address, Bytes, FixedBytes, B256, U256};
use alloy::providers::ext::Erc4337Api;
use alloy::providers::{Provider, ProviderBuilder, RootProvider};
use alloy::rpc::types::{
    Log as RpcLog, PackedUserOperation, SendUserOperation, UserOperationReceipt,
};
use alloy::signers::local::PrivateKeySigner;
use alloy::signers::Signer;
use alloy::sol;
use alloy::sol_types::SolCall;
use anyhow::Result;
use serde_json::json;
use std::collections::{BTreeSet, HashMap};
use std::sync::{Mutex, OnceLock};

use allbright_core::config::Config;
use allbright_core::events::Chain;

// ---- Verified on-chain constants (Kernel v3.1 / EntryPoint v0.7) ----
pub const ENTRYPOINT_V07: &str = "0x0000000071727De22E5E9d8BAf0edAc6f37da032";
pub const KERNEL_FACTORY: &str = "0xaac5D4240AF87249B3f71BC8E4A2cae074A3E419";
pub const ECDSA_VALIDATOR: &str = "0x845ADb2C711129d4f3966735eD98a9F09fC4cE57";
#[allow(dead_code)]
pub const KERNEL_IMPL: &str = "0xBAC849bB641841b44E965fB01A4Bf5F074f84b4D";

/// Deterministic-deployment proxy (CREATE2 deployer) — bytecode verified
/// present on all 10 enabled chains on 2026-10-08. Calling it with
/// `salt(32) ‖ initcode` deploys at a predictable address.
pub const CREATE2_PROXY: &str = "0x4e59b44847b379578588920cA78FbF26c0B4956C";

/// Predicted CREATE2 address for `initcode` deployed through the proxy.
pub fn create2_address(salt: B256, initcode: &[u8]) -> Address {
    let mut p = vec![0xffu8];
    p.extend_from_slice(CREATE2_PROXY.parse::<Address>().unwrap().as_slice());
    p.extend_from_slice(salt.as_slice());
    p.extend_from_slice(&keccak256(initcode)[..]);
    Address::from_slice(&keccak256(p)[12..])
}

sol! {
    interface IKernelFactory {
        function getAddress(bytes data, bytes32 salt) external view returns (address);
        function createAccount(bytes data, bytes32 salt) external payable returns (address);
    }
    interface IKernel {
        function initialize(bytes21 _rootValidator, address hook, bytes validatorData, bytes hookData, bytes[] initConfig) external;
        function execute(bytes32 mode, bytes executionCalldata) external;
    }
    interface IAllbrightExecutor {
        function sweepNative(address to) external;
        /// On-chain profit authority (contracts/AllbrightExecutor.sol):
        /// emitted by `_runRoute` with the MEASURED balance delta.
        event Executed(address indexed token, uint256 input, uint256 output, uint256 profit);
        /// Emitted by `sweep`/`sweepNative` — the settlement evidence the
        /// transfer ledger is written from (GAP-21).
        event Swept(address indexed token, address indexed to, uint256 amount);
    }
}

/// Allbright chain key → Pimlico v2 network slug.
pub fn pimlico_chain(chain: &str) -> &str {
    match chain {
        "eth" => "ethereum",
        "arb" => "arbitrum",
        "op" => "optimism",
        "poly" => "polygon",
        "bsc" => "binance",
        "avax" => "avalanche",
        "base" => "base",
        "linea" => "linea",
        "scroll" => "scroll",
        "blast" => "blast",
        other => other,
    }
}

// ---------- GAP-31: in-process pending-nonce allocator ----------
//
// `EntryPoint.getNonce` only counts *landed* ops — an op sitting in the
// bundler mempool doesn't advance it, so two submissions inside one
// inclusion window would share a nonce (AA10-class bundler rejection).
// The allocator tracks a high-water mark + the set of in-flight nonces
// per (chain, account); on-chain reads resync it, terminal outcomes
// free or consume individual nonces.

#[derive(Default)]
struct NonceSlot {
    /// Next nonce to hand out when the on-chain read doesn't move us
    /// forward (in-flight ops keep it ahead of `getNonce`).
    next: U256,
    /// Nonces of ops submitted but not yet terminally resolved.
    inflight: BTreeSet<U256>,
}

static NONCES: OnceLock<Mutex<HashMap<(u64, Address), NonceSlot>>> = OnceLock::new();

fn nonce_table() -> &'static Mutex<HashMap<(u64, Address), NonceSlot>> {
    NONCES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Allocate the next usable nonce given the fresh on-chain value.
/// `max(next, onchain)` covers both directions: a landed op advances
/// `onchain` past our mark, an in-flight op keeps `next` ahead.
fn alloc_nonce(chain_id: u64, account: Address, onchain: U256) -> U256 {
    let mut m = nonce_table().lock().unwrap();
    let slot = m.entry((chain_id, account)).or_default();
    // Landed ops: on-chain nonce moved past them — drop stale markers.
    slot.inflight.retain(|n| *n >= onchain);
    let n = slot.next.max(onchain);
    slot.inflight.insert(n);
    slot.next = n + U256::from(1u64);
    n
}

/// Free a nonce whose op never landed (bundler rejection, evicted,
/// poll timeout). EP v0.7 key-0 nonces are sequential — an unfreed gap
/// would stall every later op.
fn free_nonce(chain_id: u64, account: Address, nonce: U256) {
    let mut m = nonce_table().lock().unwrap();
    if let Some(slot) = m.get_mut(&(chain_id, account)) {
        slot.inflight.remove(&nonce);
        if nonce < slot.next {
            slot.next = nonce;
        }
    }
}

/// Mark a nonce consumed on-chain (receipt exists — landed ops consume
/// their nonce whether the call succeeded or reverted).
fn consume_nonce(chain_id: u64, account: Address, nonce: U256) {
    let mut m = nonce_table().lock().unwrap();
    if let Some(slot) = m.get_mut(&(chain_id, account)) {
        slot.inflight.remove(&nonce);
    }
}

/// userOpHash → allocated nonce, so the outcome poller can resync the
/// allocator without threading nonce state through every caller.
#[derive(Clone, Copy)]
struct PendingOp {
    chain_id: u64,
    account: Address,
    nonce: U256,
}

static PENDING_OPS: OnceLock<Mutex<HashMap<String, PendingOp>>> = OnceLock::new();

fn pending_ops() -> &'static Mutex<HashMap<String, PendingOp>> {
    PENDING_OPS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn pending_op_insert(user_op_hash: &str, chain_id: u64, account: Address, nonce: U256) {
    pending_ops()
        .lock()
        .unwrap()
        .insert(user_op_hash.to_string(), PendingOp { chain_id, account, nonce });
}

/// Terminal outcome: a receipt was produced — the nonce is consumed
/// on-chain regardless of `success`. Call once per submitted op.
pub fn note_userop_included(user_op_hash: &str) {
    if let Some(op) = pending_ops().lock().unwrap().remove(user_op_hash) {
        consume_nonce(op.chain_id, op.account, op.nonce);
    }
}

/// The op never landed (send rejection handled internally, mempool
/// eviction, or poll timeout): free its nonce for reissue.
pub fn note_userop_dropped(user_op_hash: &str) {
    if let Some(op) = pending_ops().lock().unwrap().remove(user_op_hash) {
        free_nonce(op.chain_id, op.account, op.nonce);
    }
}

// ---------- GAP-19/21: typed receipt-event decoding ----------

/// Decoded `AllbrightExecutor.Executed` — the on-chain MEASURED profit,
/// the only number that may be credited as realized.
#[derive(Debug, Clone)]
pub struct ExecutedEvent {
    pub token: Address,
    pub input: U256,
    pub output: U256,
    pub profit: U256,
}

/// Decoded `AllbrightExecutor.Swept` — settlement evidence for the
/// transfer ledger (`data/transfers.jsonl`).
#[derive(Debug, Clone)]
pub struct SweptEvent {
    pub token: Address,
    pub to: Address,
    pub amount: U256,
}

#[derive(Debug, Default)]
pub struct ExecEvents {
    /// Last `Executed` in the log stream (a route emits at most one).
    pub executed: Option<ExecutedEvent>,
    /// First `Swept` in the log stream.
    pub swept: Option<SweptEvent>,
}

/// Decode `Executed`/`Swept` from a `UserOperationReceipt.logs`.
/// `emitter` = the deployed `AllbrightExecutor` contract — logs from
/// other contracts (EntryPoint, pools, tokens) are ignored either way:
/// `log_decode` signature-checks topic0.
pub fn decode_receipt_events(logs: &[RpcLog], emitter: Option<Address>) -> ExecEvents {
    let mut out = ExecEvents::default();
    for log in logs {
        if let Some(e) = emitter {
            if log.address() != e {
                continue;
            }
        }
        if let Ok(l) = log.log_decode::<IAllbrightExecutor::Executed>() {
            let d = l.data();
            out.executed = Some(ExecutedEvent {
                token: d.token,
                input: d.input,
                output: d.output,
                profit: d.profit,
            });
            continue;
        }
        if let Ok(l) = log.log_decode::<IAllbrightExecutor::Swept>() {
            let d = l.data();
            if out.swept.is_none() {
                out.swept = Some(SweptEvent {
                    token: d.token,
                    to: d.to,
                    amount: d.amount,
                });
            }
        }
    }
    out
}

// ---------- GAP-26: finality evidence ----------
//
// Inclusion ≠ finality. Deterministic `finalized` exists where the
// chain defines it natively (Ethereum epochs, BSC BEP-648
// fast-finality ~0.65s, Polygon milestones ~2–5s). On single-sequencer
// L2s `finalized` means L1 finality lagging hours–days, so `safe`
// (batch posted to L1) is the meaningful local tag. Chains supporting
// neither tag fall back to a small confirmation depth.

/// Tag preference order per chain — first tag that resolves wins.
fn finality_tags(chain: Chain) -> &'static [BlockNumberOrTag] {
    use BlockNumberOrTag::{Finalized, Safe};
    match chain {
        // Deterministic `finalized` — verified semantics, not assumed.
        Chain::Eth | Chain::Bsc | Chain::Poly => &[Finalized, Safe],
        // Single-sequencer L2s + Avalanche: `safe` is the meaningful
        // local tag; `finalized` tried as a secondary in case the
        // endpoint doesn't expose `safe`.
        Chain::Arb | Chain::Op | Chain::Base | Chain::Linea | Chain::Scroll
        | Chain::Blast | Chain::Avax => &[Safe, Finalized],
    }
}

/// Fallback confirmation depth (blocks buried) used only when neither
/// `finalized` nor `safe` resolves on the endpoint — approximates each
/// chain's observed reorg horizon; conservative, small.
fn confirmation_depth(chain: Chain) -> u64 {
    match chain {
        Chain::Eth => 12,
        Chain::Bsc => 15,
        Chain::Poly => 20,
        Chain::Avax => 20,
        // Sequencer reorgs are rare; depth covers RPC-tag gaps only.
        Chain::Arb | Chain::Op | Chain::Base | Chain::Linea | Chain::Scroll
        | Chain::Blast => 8,
    }
}

#[derive(Clone)]
pub struct PimlicoClient {
    api_key: String,
    client: reqwest::Client,
}

/// Kernel initialize calldata bound to an EOA owner via the ECDSA validator.
/// rootValidator id = 0x01 (validator) ‖ validator address.
pub fn kernel_init_data(owner: Address) -> Bytes {
    let mut id = [0u8; 21];
    id[0] = 0x01;
    id[1..].copy_from_slice(ECDSA_VALIDATOR.parse::<Address>().unwrap().as_slice());
    let root_validator = FixedBytes::<21>::from(id);
    IKernel::initializeCall {
        _rootValidator: root_validator,
        hook: Address::ZERO,
        validatorData: Bytes::from(owner.as_slice().to_vec()),
        hookData: Bytes::new(),
        initConfig: Vec::new(),
    }
    .abi_encode()
    .into()
}

/// Deployment callData for the factory: `createAccount(init, salt=0)`.
pub fn factory_deploy_calldata(init_data: &Bytes) -> Bytes {
    IKernelFactory::createAccountCall {
        data: init_data.clone(),
        salt: FixedBytes::<32>::ZERO,
    }
    .abi_encode()
    .into()
}

/// Account-level callData: ERC-7579 `execute(mode, executionCalldata)`.
/// Mode 0x00…00 = CALLTYPE_SINGLE + EXECTYPE_DEFAULT; executionCalldata =
/// encodePacked(dest(20) ‖ value(32) ‖ func).
pub fn kernel_execute_calldata(dest: Address, value: U256, func: Bytes) -> Bytes {
    let mut exec = Vec::with_capacity(52 + func.len());
    exec.extend_from_slice(dest.as_slice());
    exec.extend_from_slice(&value.to_be_bytes::<32>());
    exec.extend_from_slice(&func);
    IKernel::executeCall {
        mode: FixedBytes::<32>::ZERO,
        executionCalldata: Bytes::from(exec),
    }
    .abi_encode()
    .into()
}

/// Build the typed EP v0.7 `PackedUserOperation` — the same wire shape
/// the old hand-encoder produced (`factory`/`factoryData`, split
/// paymaster fields, absent paymaster fields when unsponsored).
#[allow(clippy::too_many_arguments)]
fn packed_op(
    sender: Address,
    nonce: U256,
    factory: Option<Address>,
    factory_data: &Bytes,
    call_data: &Bytes,
    call_gas: U256,
    verification_gas: U256,
    pre_verification_gas: U256,
    max_fee: U256,
    max_prio: U256,
    paymaster: Option<Address>,
    paymaster_data: &Bytes,
    pm_ver_gas: U256,
    pm_post_gas: U256,
    signature: &Bytes,
) -> PackedUserOperation {
    PackedUserOperation {
        sender,
        nonce,
        factory,
        factory_data: factory.map(|_| factory_data.clone()),
        call_data: call_data.clone(),
        call_gas_limit: call_gas,
        verification_gas_limit: verification_gas,
        pre_verification_gas,
        max_fee_per_gas: max_fee,
        max_priority_fee_per_gas: max_prio,
        paymaster,
        paymaster_verification_gas_limit: paymaster.map(|_| pm_ver_gas),
        paymaster_post_op_gas_limit: paymaster.map(|_| pm_post_gas),
        paymaster_data: paymaster.map(|_| paymaster_data.clone()),
        signature: signature.clone(),
    }
}

/// ERC-4337 v0.7 userOpHash: keccak( abi.encode( keccak(packedOp), EP, chainId ) ).
/// initCode = factory ‖ factoryData; paymasterAndData = paymaster ‖
/// paymasterVerificationGasLimit(16B) ‖ paymasterPostOpGasLimit(16B) ‖ paymasterData.
#[allow(clippy::too_many_arguments)]
fn user_op_hash(
    sender: Address,
    nonce: U256,
    init_code: &Bytes,
    call_data: &Bytes,
    call_gas: U256,
    verification_gas: U256,
    pre_verification_gas: U256,
    max_fee: U256,
    max_prio: U256,
    paymaster: Address,
    paymaster_data: &Bytes,
    pm_ver_gas: U256,
    pm_post_gas: U256,
    chain_id: u64,
) -> B256 {
    let mut paymaster_and_data = Vec::new();
    if paymaster != Address::ZERO {
        paymaster_and_data.extend_from_slice(paymaster.as_slice());
        let ver: FixedBytes<16> = pm_ver_gas.to_be_bytes::<32>()[16..].try_into().unwrap();
        let post: FixedBytes<16> = pm_post_gas.to_be_bytes::<32>()[16..].try_into().unwrap();
        paymaster_and_data.extend_from_slice(&ver[..]);
        paymaster_and_data.extend_from_slice(&post[..]);
        paymaster_and_data.extend_from_slice(paymaster_data);
    }
    let paymaster_and_data = Bytes::from(paymaster_and_data);
    let account_gas_limits: U256 = (verification_gas << 128) | call_gas;
    let gas_fees: U256 = (max_prio << 128) | max_fee;
    let packed = keccak256(
        DynSolValue::Tuple(vec![
            DynSolValue::Address(sender),
            DynSolValue::Uint(nonce, 256),
            DynSolValue::FixedBytes(keccak256(init_code), 32),
            DynSolValue::FixedBytes(keccak256(call_data), 32),
            DynSolValue::FixedBytes(account_gas_limits.into(), 32),
            DynSolValue::Uint(pre_verification_gas, 256),
            DynSolValue::FixedBytes(gas_fees.into(), 32),
            DynSolValue::FixedBytes(keccak256(paymaster_and_data), 32),
        ])
        .abi_encode(),
    );
    keccak256(
        DynSolValue::Tuple(vec![
            DynSolValue::FixedBytes(packed, 32),
            DynSolValue::Address(ENTRYPOINT_V07.parse().unwrap()),
            DynSolValue::Uint(U256::from(chain_id), 256),
        ])
        .abi_encode(),
    )
}

fn jhex_to_u256(v: &serde_json::Value, key: &str) -> U256 {
    v.get(key)
        .and_then(|x| x.as_str())
        .and_then(|s| U256::from_str_radix(s.trim_start_matches("0x"), 16).ok())
        .unwrap_or_default()
}

/// A submitted gasless sweep — the caller polls this UserOp to a
/// confirmed `Swept` event (GAP-21) before touching `last_swept`.
#[derive(Debug, Clone)]
pub struct SweepSubmission {
    pub chain: Chain,
    /// Public RPC for the finality check (not the bundler endpoint).
    pub rpc_url: String,
    /// The AllbrightExecutor contract — emits `Swept`; log filter.
    pub executor: Address,
    pub user_op_hash: String,
    pub to: String,
}

impl PimlicoClient {
    pub fn new(api_key: String) -> Self {
        PimlicoClient {
            api_key,
            client: reqwest::Client::new(),
        }
    }

    fn endpoint(&self, chain: &str) -> String {
        format!("https://api.pimlico.io/v2/{chain}/rpc?apikey={}", self.api_key)
    }

    /// GAP-27 — the bundler endpoint carries `?apikey=<secret>` in the
    /// URL, and reqwest/alloy error Display embeds the request URL.
    /// Every error string that leaves this layer has the key stripped.
    fn redact(&self, msg: String) -> String {
        if self.api_key.is_empty() {
            return msg;
        }
        msg.replace(&format!("apikey={}", self.api_key), "apikey=[redacted]")
            .replace(&self.api_key, "[redacted]")
    }

    /// Alloy provider over an endpoint URL — bundler or public RPC.
    /// The URL may carry credentials: constructed, never logged.
    fn provider_for(&self, url: &str) -> Result<RootProvider> {
        let url: reqwest::Url = url
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid rpc endpoint"))?;
        Ok(ProviderBuilder::default().connect_http(url))
    }

    async fn rpc(
        &self,
        chain: &str,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let body = json!({"jsonrpc":"2.0","id":1,"method":method,"params":params});
        let resp = self
            .client
            .post(self.endpoint(chain))
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                anyhow::anyhow!(
                    "pimlico {method}: {}",
                    self.redact(e.without_url().to_string())
                )
            })?;
        let v: serde_json::Value = resp.json().await.map_err(|e| {
            anyhow::anyhow!(
                "pimlico {method}: {}",
                self.redact(e.without_url().to_string())
            )
        })?;
        if let Some(err) = v.get("error") {
            anyhow::bail!("pimlico {method}: {}", self.redact(err.to_string()));
        }
        Ok(v["result"].clone())
    }

    /// Public-RPC helper raced across an endpoint pool — first
    /// transport-success wins; JSON-RPC errors are definitive (the node
    /// executed the call) and also win the race. All-fail → last error.
    async fn public_rpc_pool(
        &self,
        rpc_urls: &[String],
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value> {
        use futures_util::stream::{FuturesUnordered, StreamExt};
        let mut futs = FuturesUnordered::new();
        for url in rpc_urls.iter().take(3) {
            let params = params.clone();
            futs.push(async move {
                let body = json!({"jsonrpc":"2.0","id":1,"method":method,"params":params});
                let res: Result<serde_json::Value> = async {
                    let v: serde_json::Value = self
                        .client
                        .post(url.as_str())
                        .json(&body)
                        .send()
                        .await
                        .map_err(|e| anyhow::anyhow!("{e}"))?
                        .json()
                        .await
                        .map_err(|e| anyhow::anyhow!("{e}"))?;
                    if let Some(err) = v.get("error") {
                        anyhow::bail!("{err}");
                    }
                    Ok(v["result"].clone())
                }
                .await;
                res
            });
        }
        let mut last = anyhow::anyhow!("empty rpc pool");
        while let Some(res) = futs.next().await {
            match res {
                Ok(v) => return Ok(v),
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    /// Public-RPC helper (free public endpoint) for eth_call/eth_getCode.
    async fn public_rpc(
        &self,
        rpc_url: &str,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let body = json!({"jsonrpc":"2.0","id":1,"method":method,"params":params});
        let v: serde_json::Value = self
            .client
            .post(rpc_url)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                anyhow::anyhow!("{method}: {}", self.redact(e.without_url().to_string()))
            })?
            .json()
            .await
            .map_err(|e| {
                anyhow::anyhow!("{method}: {}", self.redact(e.without_url().to_string()))
            })?;
        if let Some(err) = v.get("error") {
            anyhow::bail!("{method}: {}", self.redact(err.to_string()));
        }
        Ok(v["result"].clone())
    }

    /// Derive the Kernel account address for `owner` on this chain.
    /// Accepts an endpoint pool — a single-element slice behaves as
    /// before; multi-endpoint pools race (§36.9).
    pub async fn account_address(&self, rpc_urls: &[String], owner: Address) -> Result<Address> {
        let init = kernel_init_data(owner);
        let call_data = IKernelFactory::getAddressCall {
            data: init,
            salt: FixedBytes::<32>::ZERO,
        }
        .abi_encode();
        let res = self
            .public_rpc_pool(
                rpc_urls,
                "eth_call",
                json!([{ "to": KERNEL_FACTORY, "data": format!("0x{}", hex::encode(call_data)) }, "latest"]),
            )
            .await?;
        let s = res.as_str().unwrap_or_default();
        Ok(format!("0x{}", &s[s.len().saturating_sub(40)..]).parse()?)
    }

    /// EntryPoint.getNonce(sender, key=0) over the public RPC pool —
    /// the ON-CHAIN nonce only; the allocator adds in-flight ops on top.
    async fn entrypoint_nonce(&self, rpc_urls: &[String], account: Address) -> Result<U256> {
        let res = self
            .public_rpc_pool(
                rpc_urls,
                "eth_call",
                json!([{ "to": ENTRYPOINT_V07, "data": format!("0x{}", hex::encode(nonce_calldata(account))) }, "latest"]),
            )
            .await?;
        Ok(U256::from_str_radix(
            res.as_str().unwrap_or("0x0").trim_start_matches("0x"),
            16,
        )
        .unwrap_or_default())
    }

    /// ERC-20 `decimals()` via the public RPC — measured on-chain,
    /// used to convert the `Executed.profit` raw units to USD.
    pub async fn token_decimals(&self, rpc_url: &str, token: Address) -> Result<u8> {
        let res = self
            .public_rpc(
                rpc_url,
                "eth_call",
                json!([{ "to": format!("{token:?}"), "data": format!("0x{}", hex::encode(&keccak256(b"decimals()")[..4])) }, "latest"]),
            )
            .await?;
        let raw = hex::decode(res.as_str().unwrap_or("0x").trim_start_matches("0x"))
            .unwrap_or_default();
        raw.last()
            .copied()
            .filter(|d| *d > 0 && *d <= 36)
            .ok_or_else(|| anyhow::anyhow!("decimals() unreadable for {token:?}"))
    }

    /// Typed `eth_getUserOperationReceipt` — the single outcome path
    /// (GAP-18/19/21/26). `Ok(None)` = still pending (bundler returns
    /// `null`); `Ok(Some)` = the op landed — check `success`.
    ///
    /// Fetches raw JSON first because bundlers return `reason` as
    /// plain text (e.g. "AA23 reverted") which doesn't satisfy alloy's
    /// `Bytes` type — normalized to hex before the typed decode. A
    /// missing `paymaster` (self-funded ops) defaults to zero address.
    pub async fn user_op_receipt(
        &self,
        chain_key: &str,
        user_op_hash: &str,
    ) -> Result<Option<UserOperationReceipt>> {
        let chain = pimlico_chain(chain_key);
        let bundler = self.provider_for(&self.endpoint(chain))?;
        let hash: Bytes = user_op_hash
            .parse()
            .map_err(|_| anyhow::anyhow!("bad userOpHash {user_op_hash}"))?;
        let v: serde_json::Value = bundler
            .client()
            .request("eth_getUserOperationReceipt", (hash,))
            .await
            .map_err(|e| {
                anyhow::anyhow!(
                    "eth_getUserOperationReceipt: {}",
                    self.redact(e.to_string())
                )
            })?;
        if v.is_null() {
            return Ok(None);
        }
        let mut v = v;
        if let Some(obj) = v.as_object_mut() {
            // Plain-text revert reasons → hex so `Bytes` parses.
            if let Some(r) = obj.get("reason").and_then(|r| r.as_str()) {
                if !r.starts_with("0x") {
                    let hexed = format!("0x{}", hex::encode(r.as_bytes()));
                    obj.insert("reason".into(), serde_json::Value::String(hexed));
                }
            }
            if obj.get("paymaster").map_or(true, |p| p.is_null()) {
                obj.insert(
                    "paymaster".into(),
                    serde_json::Value::String(format!("{:?}", Address::ZERO)),
                );
            }
        }
        let receipt: UserOperationReceipt = serde_json::from_value(v)
            .map_err(|e| anyhow::anyhow!("userOp receipt decode: {e}"))?;
        Ok(Some(receipt))
    }

    /// Back-compat shim over the typed receipt: `(included, success)`.
    pub async fn user_op_outcome(&self, chain_key: &str, user_op_hash: &str) -> Result<(bool, bool)> {
        Ok(match self.user_op_receipt(chain_key, user_op_hash).await? {
            Some(r) => (true, r.success),
            None => (false, false),
        })
    }

    /// Highest block number the endpoint reports under `tag`; `None`
    /// when the tag is unsupported (error) or the block is absent.
    async fn tag_block(&self, prov: &RootProvider, tag: BlockNumberOrTag) -> Option<u64> {
        prov.get_block_by_number(tag)
            .await
            .ok()
            .flatten()
            .map(|b| b.header.inner.number)
    }

    /// GAP-26 — true only once `receipt_block` sits under a
    /// finalized/safe block (per-chain tag order) or `confirmation_depth`
    /// blocks as last resort. Fail-closed: no evidence → not final.
    pub async fn is_finalized(&self, rpc_url: &str, chain: Chain, receipt_block: u64) -> bool {
        let Ok(prov) = self.provider_for(rpc_url) else {
            return false;
        };
        for tag in finality_tags(chain) {
            if let Some(n) = self.tag_block(&prov, *tag).await {
                return n >= receipt_block;
            }
        }
        match self.tag_block(&prov, BlockNumberOrTag::Latest).await {
            Some(tip) => tip >= receipt_block + confirmation_depth(chain),
            None => false,
        }
    }

    /// Assemble → sponsor → (optionally) send a gasless UserOp.
    /// `dry_run=true` stops after sponsorship — proves the paymaster works
    /// with zero on-chain effect.
    pub async fn sponsored_user_op(
        &self,
        chain_key: &str,
        chain_id: u64,
        rpc_urls: &[String],
        owner: &PrivateKeySigner,
        account_call_data: Bytes,
        dry_run: bool,
    ) -> Result<String> {
        let chain = pimlico_chain(chain_key);
        let eoa: Address = owner.address();
        let init = kernel_init_data(eoa);
        let account = self.account_address(rpc_urls, eoa).await?;

        // factory/factoryData only when the account isn't deployed yet.
        let code = self
            .public_rpc_pool(rpc_urls, "eth_getCode", json!([format!("{account:?}"), "latest"]))
            .await?;
        let deployed = code.as_str().map(|c| c.len() > 4).unwrap_or(false);
        let (factory, factory_data): (Option<Address>, Bytes) = if deployed {
            (None, Bytes::new())
        } else {
            (
                Some(KERNEL_FACTORY.parse::<Address>()?),
                factory_deploy_calldata(&init),
            )
        };
        // initCode (factory ‖ factoryData) for the v0.7 packed hash.
        let mut ic = Vec::new();
        if let Some(f) = factory {
            ic.extend_from_slice(f.as_slice());
        }
        ic.extend_from_slice(&factory_data);
        let init_code: Bytes = ic.into();

        // GAP-31 — pending-nonce allocator: on-chain getNonce + the
        // count of our in-flight ops. Two submissions in one inclusion
        // window must not share a nonce (AA10-class bundler rejection).
        let onchain = self.entrypoint_nonce(rpc_urls, account).await?;
        let nonce = alloc_nonce(chain_id, account, onchain);

        let out = self
            .build_and_send(
                chain,
                chain_id,
                owner,
                account,
                nonce,
                factory,
                &factory_data,
                &init_code,
                account_call_data,
                deployed,
                dry_run,
            )
            .await;
        match out {
            Err(e) => {
                // Assemble/sponsor/send failed — the op never reached the
                // mempool; its nonce is free again.
                free_nonce(chain_id, account, nonce);
                Err(e)
            }
            Ok(s) => {
                if dry_run {
                    // Never entered a mempool — the nonce is still free.
                    free_nonce(chain_id, account, nonce);
                }
                Ok(s)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn build_and_send(
        &self,
        chain: &str,
        chain_id: u64,
        owner: &PrivateKeySigner,
        account: Address,
        nonce: U256,
        factory: Option<Address>,
        factory_data: &Bytes,
        init_code: &Bytes,
        account_call_data: Bytes,
        deployed: bool,
        dry_run: bool,
    ) -> Result<String> {
        // Gas price from the bundler (fast tier) — vendor-specific, no
        // standard equivalent; kept for bundler compatibility.
        let gp = self
            .rpc(chain, "pimlico_getUserOperationGasPrice", json!([]))
            .await?;
        let fast = &gp["fast"];
        let max_fee = jhex_to_u256(fast, "maxFeePerGas");
        let max_prio = jhex_to_u256(fast, "maxPriorityFeePerGas");

        // Well-formed dummy signature: ECDSAValidator reverts on malformed
        // sigs (InvalidSignature via ecrecover), so estimation needs a
        // properly-encoded 65-byte sig — content is ignored when it
        // recovers to a non-owner address (sigFailed is tolerated by
        // simulateValidation).
        let dummy_sig = Bytes::from(
            owner
                .sign_message(B256::ZERO.as_slice())
                .await?
                .as_bytes()
                .to_vec(),
        );
        let zero_pm: Option<Address> = None;
        // Estimate with zero gas price: without a paymaster the unfunded
        // account can't pay prefund (AA21). A sponsored op pays nothing.
        //
        // NOTE (alloy API reality): `UserOperationGasEstimation` field
        // names serialize as `verificationGas`/`paymasterVerificationGas`,
        // but EP v0.7 bundlers (Pimlico) answer with the spec names
        // `verificationGasLimit`/`paymasterVerificationGasLimit` — the
        // typed `estimate_user_operation_gas` would fail deserialization.
        // The op itself is the typed `PackedUserOperation`; the estimate
        // call stays on the raw method for response-shape compatibility.
        let est_op = packed_op(
            account,
            nonce,
            factory,
            factory_data,
            &account_call_data,
            U256::from(2_000_000u64),
            U256::from(2_000_000u64),
            U256::from(100_000u64),
            U256::ZERO,
            U256::ZERO,
            zero_pm,
            &Bytes::new(),
            U256::ZERO,
            U256::ZERO,
            &dummy_sig,
        );
        let est = self
            .rpc(
                chain,
                "eth_estimateUserOperationGas",
                json!([serde_json::to_value(&est_op)?, ENTRYPOINT_V07]),
            )
            .await?;
        let mut call_gas = jhex_to_u256(&est, "callGasLimit");
        let mut ver_gas = jhex_to_u256(&est, "verificationGasLimit");
        let mut pvg = jhex_to_u256(&est, "preVerificationGas");
        // Sponsorship must see the REAL gas prices — the paymaster's
        // signature covers them; changing them after sponsorship yields
        // AA34 (paymaster signature error).
        let sponsor_op = packed_op(
            account,
            nonce,
            factory,
            factory_data,
            &account_call_data,
            call_gas,
            ver_gas,
            pvg,
            max_fee,
            max_prio,
            zero_pm,
            &Bytes::new(),
            U256::ZERO,
            U256::ZERO,
            &dummy_sig,
        );

        // Paymaster sponsorship — the gasless step (vendor-specific).
        let pm = self
            .rpc(
                chain,
                "pm_sponsorUserOperation",
                json!([serde_json::to_value(&sponsor_op)?, ENTRYPOINT_V07]),
            )
            .await?;
        // v0.7 returns split paymaster fields; v0.6 shape defensively mapped.
        let paymaster: Address = pm["paymaster"]
            .as_str()
            .and_then(|s| s.parse().ok())
            .unwrap_or_else(|| {
                pm["paymasterAndData"]
                    .as_str()
                    .and_then(|s| s.get(2..42))
                    .and_then(|s| format!("0x{s}").parse().ok())
                    .unwrap_or_default()
            });
        let paymaster_data = Bytes::from(
            pm["paymasterData"]
                .as_str()
                .and_then(|s| s.strip_prefix("0x"))
                .and_then(|s| hex::decode(s).ok())
                .or_else(|| {
                    pm["paymasterAndData"].as_str().and_then(|s| {
                        s.trim_start_matches("0x")
                            .get(104..)
                            .and_then(|s| hex::decode(s).ok())
                    })
                })
                .unwrap_or_default(),
        );
        let pm_ver_gas = jhex_to_u256(&pm, "paymasterVerificationGasLimit");
        let pm_post_gas = jhex_to_u256(&pm, "paymasterPostOpGasLimit");
        // Paymaster may tighten gas limits — merge if present.
        if let Some(v) = pm.get("callGasLimit").and_then(|x| x.as_str()) {
            call_gas = U256::from_str_radix(v.trim_start_matches("0x"), 16).unwrap_or(call_gas);
        }
        if let Some(v) = pm.get("verificationGasLimit").and_then(|x| x.as_str()) {
            ver_gas =
                U256::from_str_radix(v.trim_start_matches("0x"), 16).unwrap_or(ver_gas);
        }
        if let Some(v) = pm.get("preVerificationGas").and_then(|x| x.as_str()) {
            pvg = U256::from_str_radix(v.trim_start_matches("0x"), 16).unwrap_or(pvg);
        }

        if dry_run {
            return Ok(format!(
                "DRY-RUN ok: account={account:?} deployed={deployed} paymaster={paymaster:?} pmData={}B",
                paymaster_data.len()
            ));
        }

        // Hash + sign. The ECDSA validator recovers over
        // toEthSignedMessageHash(userOpHash) → sign_message applies the
        // "\x19Ethereum Signed Message" prefix.
        let h = user_op_hash(
            account,
            nonce,
            init_code,
            &account_call_data,
            call_gas,
            ver_gas,
            pvg,
            max_fee,
            max_prio,
            paymaster,
            &paymaster_data,
            pm_ver_gas,
            pm_post_gas,
            chain_id,
        );
        let sig = owner.sign_message(h.as_slice()).await?;
        let signature = Bytes::from(sig.as_bytes().to_vec());

        let final_op = packed_op(
            account,
            nonce,
            factory,
            factory_data,
            &account_call_data,
            call_gas,
            ver_gas,
            pvg,
            max_fee,
            max_prio,
            Some(paymaster),
            &paymaster_data,
            pm_ver_gas,
            pm_post_gas,
            &signature,
        );

        // Standard ERC-4337 submission via alloy's typed API (GAP-18).
        let bundler = self.provider_for(&self.endpoint(chain))?;
        let entry_point: Address = ENTRYPOINT_V07.parse()?;
        let sent = bundler
            .send_user_operation(SendUserOperation::EntryPointV07(final_op), entry_point)
            .await
            .map_err(|e| {
                anyhow::anyhow!("eth_sendUserOperation: {}", self.redact(e.to_string()))
            })?;
        let hash = format!("0x{}", hex::encode(&sent.user_op_hash));
        // Register so the outcome poller can resync the nonce allocator.
        pending_op_insert(&hash, chain_id, account, nonce);
        Ok(hash)
    }

    /// Gasless contract deployment: the Kernel smart account calls the
    /// CREATE2 proxy inside a Pimlico-sponsored UserOp — zero native gas
    /// required on any chain. Returns (predicted address, op result).
    /// `initcode` must already include constructor args.
    pub async fn deploy_via_sponsored_userop(
        &self,
        chain_key: &str,
        chain_id: u64,
        rpc_urls: &[String],
        owner: &PrivateKeySigner,
        initcode: Bytes,
        dry_run: bool,
    ) -> Result<(Address, String)> {
        let salt = B256::ZERO;
        let predicted = create2_address(salt, &initcode);
        let mut proxy_data = Vec::with_capacity(32 + initcode.len());
        proxy_data.extend_from_slice(salt.as_slice());
        proxy_data.extend_from_slice(&initcode);
        let call = kernel_execute_calldata(
            CREATE2_PROXY.parse()?,
            U256::ZERO,
            Bytes::from(proxy_data),
        );
        let res = self
            .sponsored_user_op(chain_key, chain_id, rpc_urls, owner, call, dry_run)
            .await?;
        Ok((predicted, res))
    }

    /// Gasless profit sweep: smart account calls
    /// `AllbrightExecutor.sweepNative(profitWallet)` through a sponsored
    /// UserOp. Dormant by construction — needs a deployed executor
    /// (`ALLBRIGHT_EXECUTOR_<CHAIN>`), a configured `PROFIT_WALLET`, and
    /// `PRIVATE_KEY`; the caller gates on accumulated profit.
    ///
    /// Returns `Ok(None)` when deferred (no key/executor), else the
    /// submission handle the caller tracks to a CONFIRMED `Swept`
    /// event — `last_swept` advances only on confirmation (GAP-21).
    pub async fn send_gasless_sweep(
        &self,
        cfg: &Config,
        pending_usd: f64,
        _marker: u64,
        to: &str,
    ) -> Result<Option<SweepSubmission>> {
        let Some(key) = cfg.signing_key() else {
            tracing::warn!("sweep: PRIVATE_KEY unset");
            return Ok(None);
        };
        // First chain with a deployed executor contract wins.
        let Some((chain, executor)) = Chain::ALL
            .into_iter()
            .filter_map(|c| cfg.executor_address(c).map(|a| (c, a)))
            .next()
        else {
            tracing::debug!("sweep: no ALLBRIGHT_EXECUTOR_<CHAIN> configured — deferred");
            return Ok(None);
        };
        let rpcs = cfg
            .rpc_pools
            .get(&chain)
            .cloned()
            .filter(|v| !v.is_empty())
            .or_else(|| cfg.rpc_urls.get(&chain).cloned().map(|u| vec![u]));
        let Some(rpcs) = rpcs else {
            anyhow::bail!("sweep: no RPC for {}", chain.name());
        };
        let executor_addr: Address = executor.parse()?;
        let profit_wallet: Address = to.parse()?;
        let signer: PrivateKeySigner = key.parse()?;

        let sweep =
            IAllbrightExecutor::sweepNativeCall { to: profit_wallet }.abi_encode();
        let call = kernel_execute_calldata(executor_addr, U256::ZERO, sweep.into());
        let res = self
            .sponsored_user_op(chain.name(), chain.chain_id(), &rpcs, &signer, call, false)
            .await?;
        tracing::info!(chain = %chain.name(), pending_usd, dest = %to, userop = %res,
            "gasless sweep submitted");
        Ok(Some(SweepSubmission {
            chain,
            rpc_url: rpcs[0].clone(),
            executor: executor_addr,
            user_op_hash: res,
            to: to.to_string(),
        }))
    }
}

/// EntryPoint.getNonce(sender, key) selector payload.
fn nonce_calldata(sender: Address) -> Vec<u8> {
    // getNonce(address,uint192) selector = 0x35567e1a
    let mut d = hex::decode("35567e1a").unwrap();
    let mut arg = [0u8; 32];
    arg[12..].copy_from_slice(sender.as_slice());
    d.extend_from_slice(&arg);
    d.extend_from_slice(&[0u8; 32]); // key = 0
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::primitives::Log as PrimLog;
    use alloy::sol_types::SolEvent;

    fn rpc_log(addr: Address, data: alloy::primitives::LogData) -> RpcLog {
        RpcLog {
            inner: PrimLog { address: addr, data },
            ..Default::default()
        }
    }

    // ---- GAP-31: nonce allocator ----

    #[test]
    fn nonce_allocator_sequences_inflight() {
        let chain = 9_900_001u64;
        let acct = Address::from([0x11; 20]);
        // Fresh account: onchain 5 → 5, 6, 7 while nothing lands.
        assert_eq!(alloc_nonce(chain, acct, U256::from(5)), U256::from(5));
        assert_eq!(alloc_nonce(chain, acct, U256::from(5)), U256::from(6));
        assert_eq!(alloc_nonce(chain, acct, U256::from(5)), U256::from(7));
    }

    #[test]
    fn nonce_allocator_resyncs_on_inclusion() {
        let chain = 9_900_002u64;
        let acct = Address::from([0x22; 20]);
        assert_eq!(alloc_nonce(chain, acct, U256::from(3)), U256::from(3));
        assert_eq!(alloc_nonce(chain, acct, U256::from(3)), U256::from(4));
        // Both landed: getNonce now returns 5 — allocator follows.
        assert_eq!(alloc_nonce(chain, acct, U256::from(5)), U256::from(5));
    }

    #[test]
    fn nonce_allocator_reissues_freed() {
        let chain = 9_900_003u64;
        let acct = Address::from([0x33; 20]);
        let a = alloc_nonce(chain, acct, U256::from(8));
        let b = alloc_nonce(chain, acct, U256::from(8));
        assert_eq!(b, a + U256::from(1u64));
        // `a`'s op never landed → freed → reissued before `b`+1.
        free_nonce(chain, acct, a);
        assert_eq!(alloc_nonce(chain, acct, U256::from(8)), a);
    }

    #[test]
    fn pending_op_terminal_paths() {
        let chain = 9_900_004u64;
        let acct = Address::from([0x44; 20]);
        let n = alloc_nonce(chain, acct, U256::from(2));
        let hash = format!("0x{}", hex::encode([0xabu8; 32]));
        pending_op_insert(&hash, chain, acct, n);
        // Dropped op frees the nonce for reissue.
        note_userop_dropped(&hash);
        assert_eq!(alloc_nonce(chain, acct, U256::from(2)), n);
        // Included op consumes it — never reissued.
        let n2 = alloc_nonce(chain, acct, U256::from(3));
        assert_eq!(n2, n + U256::from(1u64));
        pending_op_insert(&hash, chain, acct, n2);
        note_userop_included(&hash);
        assert_eq!(alloc_nonce(chain, acct, U256::from(3)), n2 + U256::from(1u64));
    }

    // ---- GAP-19/21: receipt event decoding ----

    #[test]
    fn decodes_executed_and_swept() {
        let emitter = Address::from([0xAA; 20]);
        let token = Address::from([0x55; 20]);
        let dest = Address::from([0x77; 20]);

        let exec = IAllbrightExecutor::Executed {
            token,
            input: U256::from(1_000u64),
            output: U256::from(1_070u64),
            profit: U256::from(70u64),
        };
        let swept = IAllbrightExecutor::Swept {
            token: Address::ZERO,
            to: dest,
            amount: U256::from(9_999u64),
        };
        let logs = vec![
            rpc_log(emitter, exec.encode_log_data()),
            rpc_log(emitter, swept.encode_log_data()),
        ];
        let evs = decode_receipt_events(&logs, Some(emitter));
        let e = evs.executed.expect("Executed decoded");
        assert_eq!(e.profit, U256::from(70u64));
        assert_eq!(e.token, token);
        let s = evs.swept.expect("Swept decoded");
        assert_eq!(s.amount, U256::from(9_999u64));
        assert_eq!(s.to, dest);
    }

    #[test]
    fn wrong_emitter_is_filtered() {
        let emitter = Address::from([0xAA; 20]);
        let other = Address::from([0xBB; 20]);
        let exec = IAllbrightExecutor::Executed {
            token: Address::ZERO,
            input: U256::from(1u64),
            output: U256::from(2u64),
            profit: U256::from(1u64),
        };
        let logs = vec![rpc_log(other, exec.encode_log_data())];
        let evs = decode_receipt_events(&logs, Some(emitter));
        assert!(evs.executed.is_none());
    }

    // ---- GAP-18: wire shape of the typed op ----

    #[test]
    fn packed_op_serializes_ep07_fields() {
        let op = packed_op(
            Address::from([0x01; 20]),
            U256::from(3u64),
            Some(Address::from([0x02; 20])),
            &Bytes::from(vec![0xde, 0xad]),
            &Bytes::from(vec![0xbe, 0xef]),
            U256::from(1u64),
            U256::from(2u64),
            U256::from(3u64),
            U256::from(4u64),
            U256::from(5u64),
            Some(Address::from([0x06; 20])),
            &Bytes::from(vec![0xaa]),
            U256::from(7u64),
            U256::from(8u64),
            &Bytes::from(vec![0x09]),
        );
        let v = serde_json::to_value(&op).unwrap();
        assert!(v.get("factory").is_some());
        assert!(v.get("factoryData").is_some());
        assert!(v.get("paymaster").is_some());
        assert!(v.get("paymasterData").is_some());
        assert!(v.get("paymasterVerificationGasLimit").is_some());
        assert!(v.get("paymasterPostOpGasLimit").is_some());
        // EP v0.6 field names must NOT appear.
        assert!(v.get("initCode").is_none());
        assert!(v.get("paymasterAndData").is_none());
        assert_eq!(v["nonce"].as_str().unwrap(), "0x3");
        assert!(v["signature"].as_str().unwrap().starts_with("0x"));
    }

    #[test]
    fn packed_op_omits_absent_fields() {
        let op = packed_op(
            Address::ZERO,
            U256::ZERO,
            None,
            &Bytes::new(),
            &Bytes::new(),
            U256::ZERO,
            U256::ZERO,
            U256::ZERO,
            U256::ZERO,
            U256::ZERO,
            None,
            &Bytes::new(),
            U256::ZERO,
            U256::ZERO,
            &Bytes::new(),
        );
        let v = serde_json::to_value(&op).unwrap();
        assert!(v.get("factory").is_none());
        assert!(v.get("factoryData").is_none());
        assert!(v.get("paymaster").is_none());
        assert!(v.get("paymasterData").is_none());
    }

    // ---- GAP-27: redaction ----

    #[test]
    fn redact_strips_api_key() {
        let c = PimlicoClient::new("SECRETKEY123".into());
        let msg =
            "POST https://api.pimlico.io/v2/bsc/rpc?apikey=SECRETKEY123 failed".to_string();
        let out = c.redact(msg);
        assert!(!out.contains("SECRETKEY123"));
        assert!(out.contains("[redacted]"));
    }
}
