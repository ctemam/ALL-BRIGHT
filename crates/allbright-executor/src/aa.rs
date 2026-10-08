//! Account-abstraction ops layer — **gasless mode via Pimlico**.
//!
//! The EOA (`PRIVATE_KEY`) owns an ERC-4337 smart account (ZeroDev Kernel
//! v3.1, ECDSA validator). Pimlico supplies:
//!   - the bundler (`eth_sendUserOperation`, `eth_estimateUserOperationGas`)
//!   - the verifying paymaster (`pm_sponsorUserOperation`) — gas is
//!     *sponsored*: UserOps execute with zero native-token spend.
//!
//! Scope per AGENTS.md §18: ops + profit transfer only — never the
//! execution hot path.
//!
//! All contract constants below were verified by bytecode presence on-chain
//! before use (tools + aa_probe); none are assumed.

use alloy::dyn_abi::DynSolValue;
use alloy::primitives::{hex, keccak256, Address, Bytes, FixedBytes, B256, U256};
use alloy::signers::local::PrivateKeySigner;
use alloy::signers::Signer;
use alloy::sol;
use alloy::sol_types::SolCall;
use anyhow::Result;
use serde_json::json;

use allbright_core::config::Config;
use allbright_core::events::Chain;

// ---- Verified on-chain constants (Kernel v3.1 / EntryPoint v0.7) ----
pub const ENTRYPOINT_V07: &str = "0x0000000071727De22E5E9d8BAf0edAc6f37da032";
pub const KERNEL_FACTORY: &str = "0xaac5D4240AF87249B3f71BC8E4A2cae074A3E419";
pub const ECDSA_VALIDATOR: &str = "0x845ADb2C711129d4f3966735eD98a9F09fC4cE57";
#[allow(dead_code)]
pub const KERNEL_IMPL: &str = "0xBAC849bB641841b44E965fB01A4Bf5F074f84b4D";

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

/// Pack a (partially filled) UserOp into the ERC-4337 v0.7 JSON shape
/// Pimlico expects: `factory`/`factoryData` (not initCode) and split
/// paymaster fields (not paymasterAndData).
fn pack_userop(
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
) -> serde_json::Value {
    let hx = |u: U256| format!("0x{:x}", u);
    let mut v = json!({
        "sender": format!("{sender:?}"),
        "nonce": hx(nonce),
        "callData": format!("0x{}", hex::encode(call_data)),
        "callGasLimit": hx(call_gas),
        "verificationGasLimit": hx(verification_gas),
        "preVerificationGas": hx(pre_verification_gas),
        "maxFeePerGas": hx(max_fee),
        "maxPriorityFeePerGas": hx(max_prio),
        "signature": format!("0x{}", hex::encode(signature)),
    });
    let m = v.as_object_mut().unwrap();
    if let Some(f) = factory {
        m.insert("factory".into(), json!(format!("{f:?}")));
        m.insert("factoryData".into(), json!(format!("0x{}", hex::encode(factory_data))));
    }
    if let Some(pm) = paymaster {
        m.insert("paymaster".into(), json!(format!("{pm:?}")));
        m.insert("paymasterData".into(), json!(format!("0x{}", hex::encode(paymaster_data))));
        m.insert("paymasterVerificationGasLimit".into(), json!(hx(pm_ver_gas)));
        m.insert("paymasterPostOpGasLimit".into(), json!(hx(pm_post_gas)));
    }
    v
}

/// ERC-4337 v0.7 userOpHash: keccak( abi.encode( keccak(packedOp), EP, chainId ) ).
/// initCode = factory ‖ factoryData; paymasterAndData = paymaster ‖
/// paymasterVerificationGasLimit(16B) ‖ paymasterPostOpGasLimit(16B) ‖ paymasterData.
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

impl PimlicoClient {
    pub fn new(api_key: String) -> Self {
        PimlicoClient { api_key, client: reqwest::Client::new() }
    }

    fn endpoint(&self, chain: &str) -> String {
        format!("https://api.pimlico.io/v2/{chain}/rpc?apikey={}", self.api_key)
    }

    async fn rpc(&self, chain: &str, method: &str, params: serde_json::Value) -> Result<serde_json::Value> {
        let body = json!({"jsonrpc":"2.0","id":1,"method":method,"params":params});
        let resp = self.client.post(self.endpoint(chain)).json(&body).send().await?;
        let v: serde_json::Value = resp.json().await?;
        if let Some(err) = v.get("error") {
            anyhow::bail!("pimlico {method}: {err}");
        }
        Ok(v["result"].clone())
    }

    /// Public-RPC helper (free public endpoint) for eth_call/eth_getCode.
    async fn public_rpc(&self, rpc_url: &str, method: &str, params: serde_json::Value) -> Result<serde_json::Value> {
        let body = json!({"jsonrpc":"2.0","id":1,"method":method,"params":params});
        let v: serde_json::Value = self.client.post(rpc_url).json(&body).send().await?.json().await?;
        if let Some(err) = v.get("error") {
            anyhow::bail!("{method}: {err}");
        }
        Ok(v["result"].clone())
    }

    /// Derive the Kernel account address for `owner` on this chain.
    pub async fn account_address(&self, rpc_url: &str, owner: Address) -> Result<Address> {
        let init = kernel_init_data(owner);
        let call_data = IKernelFactory::getAddressCall {
            data: init,
            salt: FixedBytes::<32>::ZERO,
        }
        .abi_encode();
        let res = self
            .public_rpc(
                rpc_url,
                "eth_call",
                json!([{ "to": KERNEL_FACTORY, "data": format!("0x{}", hex::encode(call_data)) }, "latest"]),
            )
            .await?;
        let s = res.as_str().unwrap_or_default();
        Ok(format!("0x{}", &s[s.len().saturating_sub(40)..]).parse()?)
    }

    /// Assemble → sponsor → (optionally) send a gasless UserOp.
    /// `dry_run=true` stops after sponsorship — proves the paymaster works
    /// with zero on-chain effect.
    pub async fn sponsored_user_op(
        &self,
        chain_key: &str,
        chain_id: u64,
        rpc_url: &str,
        owner: &PrivateKeySigner,
        account_call_data: Bytes,
        dry_run: bool,
    ) -> Result<String> {
        let chain = pimlico_chain(chain_key);
        let eoa: Address = owner.address();
        let init = kernel_init_data(eoa);
        let account = self.account_address(rpc_url, eoa).await?;

        // factory/factoryData only when the account isn't deployed yet.
        let code = self
            .public_rpc(rpc_url, "eth_getCode", json!([format!("{account:?}"), "latest"]))
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

        // Nonce from the EntryPoint (key 0).
        let nonce_res = self
            .public_rpc(
                rpc_url,
                "eth_call",
                json!([{ "to": ENTRYPOINT_V07, "data": format!("0x{}", hex::encode(nonce_calldata(account))) }, "latest"]),
            )
            .await?;
        let nonce = U256::from_str_radix(
            nonce_res.as_str().unwrap_or("0x0").trim_start_matches("0x"),
            16,
        )
        .unwrap_or_default();

        // Gas price from the bundler (fast tier).
        let gp = self.rpc(chain, "pimlico_getUserOperationGasPrice", json!([])).await?;
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
        let mut op = pack_userop(
            account, nonce, factory, &factory_data, &account_call_data,
            U256::from(2_000_000u64), U256::from(2_000_000u64), U256::from(100_000u64),
            U256::ZERO, U256::ZERO, zero_pm, &Bytes::new(), U256::ZERO, U256::ZERO, &dummy_sig,
        );

        // Estimate gas.
        let est = self
            .rpc(chain, "eth_estimateUserOperationGas", json!([op, ENTRYPOINT_V07]))
            .await?;
        let mut call_gas = jhex_to_u256(&est, "callGasLimit");
        let mut ver_gas = jhex_to_u256(&est, "verificationGasLimit");
        let mut pvg = jhex_to_u256(&est, "preVerificationGas");
        // Sponsorship must see the REAL gas prices — the paymaster's
        // signature covers them; changing them after sponsorship yields
        // AA34 (paymaster signature error).
        op = pack_userop(account, nonce, factory, &factory_data, &account_call_data,
            call_gas, ver_gas, pvg, max_fee, max_prio, zero_pm, &Bytes::new(),
            U256::ZERO, U256::ZERO, &dummy_sig);

        // Paymaster sponsorship — the gasless step.
        let pm = self
            .rpc(chain, "pm_sponsorUserOperation", json!([op, ENTRYPOINT_V07]))
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
                        s.trim_start_matches("0x").get(104..).and_then(|s| hex::decode(s).ok())
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
            ver_gas = U256::from_str_radix(v.trim_start_matches("0x"), 16).unwrap_or(ver_gas);
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
        let h = user_op_hash(account, nonce, &init_code, &account_call_data,
            call_gas, ver_gas, pvg, max_fee, max_prio,
            paymaster, &paymaster_data, pm_ver_gas, pm_post_gas, chain_id);
        let sig = owner.sign_message(h.as_slice()).await?;
        let signature = Bytes::from(sig.as_bytes().to_vec());

        let final_op = pack_userop(account, nonce, factory, &factory_data, &account_call_data,
            call_gas, ver_gas, pvg, max_fee, max_prio,
            Some(paymaster), &paymaster_data, pm_ver_gas, pm_post_gas, &signature);
        let res = self
            .rpc(chain, "eth_sendUserOperation", json!([final_op, ENTRYPOINT_V07]))
            .await?;
        Ok(res.as_str().unwrap_or_default().to_string())
    }

    /// Gasless profit sweep: smart account calls
    /// `AllbrightExecutor.sweepNative(profitWallet)` through a sponsored
    /// UserOp. Dormant by construction — needs a deployed executor
    /// (`ALLBRIGHT_EXECUTOR_<CHAIN>`), a configured `PROFIT_WALLET`, and
    /// `PRIVATE_KEY`; the caller gates on accumulated profit.
    pub async fn send_gasless_sweep(
        &self,
        cfg: &Config,
        pending_usd: f64,
        _marker: u64,
    ) -> Result<()> {
        let Some(key) = cfg.signing_key() else {
            tracing::warn!("sweep: PRIVATE_KEY unset");
            return Ok(());
        };
        // First chain with a deployed executor contract wins.
        let Some((chain, executor)) = Chain::ALL
            .into_iter()
            .filter_map(|c| cfg.executor_address(c).map(|a| (c, a)))
            .next()
        else {
            tracing::debug!("sweep: no ALLBRIGHT_EXECUTOR_<CHAIN> configured — deferred");
            return Ok(());
        };
        let Some(rpc) = cfg.rpc_urls.get(&chain).cloned() else {
            anyhow::bail!("sweep: no RPC for {}", chain.name());
        };
        let executor_addr: Address = executor.parse()?;
        let profit_wallet: Address = cfg.profit_wallet.parse()?;
        let signer: PrivateKeySigner = key.parse()?;

        let sweep = IAllbrightExecutor::sweepNativeCall { to: profit_wallet }
            .abi_encode();
        let call = kernel_execute_calldata(executor_addr, U256::ZERO, sweep.into());
        let res = self
            .sponsored_user_op(chain.name(), chain.chain_id(), &rpc, &signer, call, false)
            .await?;
        tracing::info!(chain = %chain.name(), pending_usd, userop = %res, "gasless sweep submitted");
        Ok(())
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
