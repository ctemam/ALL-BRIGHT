//! U11 — revm local EVM dry-run (Rusty-Sando CacheDB lineage).
//!
//! The same read-only `execute()` call the remote `eth_call` gate runs,
//! executed inside a local EVM whose account/code/storage state is
//! fetched lazily over JSON-RPC and cached for the run. Semantics are
//! identical to `eth_call` (no signature, state discarded); the win is
//! zero remote `eth_call` quota per check once state is warm — and the
//! same engine later replays routes against indexed state offline.
//!
//! Enabled by `REVM_DRYRUN=true`; when unset the remote `eth_call`
//! path runs unchanged. Fail-closed: a DB/transport miss falls back to
//! the remote call rather than guessing state.
//!
//! This module is an independent implementation and does not copy
//! proprietary source code.

use std::collections::HashMap;

use alloy::primitives::{keccak256, Address, Bytes, B256, U256};
use anyhow::Result;
use serde_json::json;

use revm::context::TxEnv;
use revm::context_interface::result::ExecutionResult;
use revm::database_interface::{Database, DBErrorMarker};
use revm::state::AccountInfo;
use revm::{Context, ExecuteEvm, MainBuilder, MainContext};

/// Verdict of a local dry-run — mirrors `eth_call` outcomes exactly:
/// `Ok` the call executed; `Revert` the EVM ran and it failed (the real
/// tx would revert identically — always gates); `Db` a state fetch
/// failed (transport class — caller falls back to remote `eth_call`).
pub enum DryVerdict {
    Ok(Bytes),
    Revert(String),
    Db(String),
}

#[derive(Debug)]
pub struct DbErr(String);

impl std::fmt::Display for DbErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for DbErr {}
impl DBErrorMarker for DbErr {}

/// Lazily-populated state view over one JSON-RPC endpoint. Every miss
/// costs one round-trip; hits are free. `Handle::block_on` inside a
/// `spawn_blocking` thread is the documented revm/AlloyDB pattern.
struct LazyRpcDb {
    url: String,
    client: reqwest::Client,
    rt: tokio::runtime::Handle,
    accts: HashMap<Address, Option<AccountInfo>>,
    codes: HashMap<B256, revm::bytecode::Bytecode>,
    slots: HashMap<(Address, U256), U256>,
    hashes: HashMap<u64, B256>,
}

impl LazyRpcDb {
    fn new(url: String) -> Self {
        Self {
            url,
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
            rt: tokio::runtime::Handle::current(),
            accts: HashMap::new(),
            codes: HashMap::new(),
            slots: HashMap::new(),
            hashes: HashMap::new(),
        }
    }

    fn rpc(&self, method: &str, params: serde_json::Value) -> Result<serde_json::Value, DbErr> {
        let fut = self
            .client
            .post(&self.url)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
            .send();
        let resp = self
            .rt
            .block_on(fut)
            .map_err(|e| DbErr(format!("{method} send: {e}")))?;
        let v: serde_json::Value = self
            .rt
            .block_on(resp.json())
            .map_err(|e| DbErr(format!("{method} json: {e}")))?;
        if let Some(err) = v.get("error") {
            return Err(DbErr(format!(
                "{method} rpc: {}",
                err["message"].as_str().unwrap_or("err")
            )));
        }
        Ok(v["result"].clone())
    }
}

fn hex_bytes(v: &serde_json::Value) -> Bytes {
    let s = v.as_str().unwrap_or("0x");
    Bytes::from(
        alloy::primitives::hex::decode(s.trim_start_matches("0x")).unwrap_or_default(),
    )
}

fn hex_u256(v: &serde_json::Value) -> U256 {
    v.as_str()
        .map(|s| U256::from_str_radix(s.trim_start_matches("0x"), 16).unwrap_or(U256::ZERO))
        .unwrap_or(U256::ZERO)
}

impl Database for LazyRpcDb {
    type Error = DbErr;

    fn basic(&mut self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        if let Some(a) = self.accts.get(&address) {
            return Ok(a.clone());
        }
        let a = format!("{address:?}");
        let code = hex_bytes(&self.rpc("eth_getCode", json!([a, "latest"]))?);
        if code.is_empty() {
            // Still need balance/nonce for EOAs the tx touches.
            let bal = self.rpc("eth_getBalance", json!([a, "latest"]))?;
            let nonce = self.rpc("eth_getTransactionCount", json!([a, "latest"]))?;
            let info = Some(AccountInfo {
                balance: hex_u256(&bal),
                nonce: hex_u256(&nonce).to::<u64>(),
                code_hash: revm::primitives::keccak256(&[] as &[u8]),
                code: None,
            });
            self.accts.insert(address, info.clone());
            return Ok(info);
        }
        let bal = self.rpc("eth_getBalance", json!([a, "latest"]))?;
        let nonce = self.rpc("eth_getTransactionCount", json!([a, "latest"]))?;
        let bc = revm::bytecode::Bytecode::new_legacy(code.clone());
        let hash = keccak256(&code);
        self.codes.insert(hash, bc.clone());
        let info = Some(AccountInfo {
            balance: hex_u256(&bal),
            nonce: hex_u256(&nonce).to::<u64>(),
            code_hash: hash,
            code: Some(bc),
        });
        self.accts.insert(address, info.clone());
        Ok(info)
    }

    fn code_by_hash(&mut self, code_hash: B256) -> Result<revm::bytecode::Bytecode, Self::Error> {
        self.codes
            .get(&code_hash)
            .cloned()
            .ok_or_else(|| DbErr(format!("code_by_hash miss {code_hash}")))
    }

    fn storage(&mut self, address: Address, index: U256) -> Result<U256, Self::Error> {
        let key = (address, index);
        if let Some(v) = self.slots.get(&key) {
            return Ok(*v);
        }
        let v = hex_u256(&self.rpc(
            "eth_getStorageAt",
            json!([format!("{address:?}"), format!("{index:#066x}"), "latest"]),
        )?);
        self.slots.insert(key, v);
        Ok(v)
    }

    fn block_hash(&mut self, number: u64) -> Result<B256, Self::Error> {
        if let Some(h) = self.hashes.get(&number) {
            return Ok(*h);
        }
        let v = self.rpc(
            "eth_getBlockByNumber",
            json!([format!("0x{number:x}"), false]),
        )?;
        let h = v["hash"].as_str().unwrap_or("0x");
        let b: B256 = h
            .parse()
            .map_err(|e: alloy::primitives::hex::FromHexError| {
                DbErr(format!("block_hash parse: {e}"))
            })?;
        self.hashes.insert(number, b);
        Ok(b)
    }
}

/// Run the `execute()` calldata in a local EVM — the same verdict
/// semantics as `eth_call`: `Ok` output bytes, `Revert` with the
/// decoded revert payload, `Db` when state couldn't be fetched.
///
/// Runs the whole thing on a blocking thread (blocking RPC inside
/// `Database::storage` would stall the async executor).
pub async fn dry_run(
    rpc_url: String,
    caller: Address,
    to: Address,
    data: Bytes,
) -> DryVerdict {
    let out = tokio::task::spawn_blocking(move || {
        let mut db = LazyRpcDb::new(rpc_url);

        // Latest block context — timestamp/number/basefee matter to
        // any code path that reads them.
        let head = db
            .rpc("eth_getBlockByNumber", json!(["latest", false]))
            .unwrap_or_else(|_| json!(null));
        let mut evm = Context::mainnet()
            .with_db(db)
            .modify_block_chained(|b| {
                b.number = hex_u256(&head["number"]);
                b.timestamp = hex_u256(&head["timestamp"]);
                b.basefee = hex_u256(&head["baseFeePerGas"]).to::<u64>();
                b.gas_limit = 30_000_000;
            })
            .modify_cfg_chained(|c| {
                // eth_call semantics: no signature — nonce check off.
                // (gas_price=0 below makes balance/fee checks moot.)
                c.disable_nonce_check = true;
            })
            .build_mainnet();
        let tx = TxEnv::builder()
            .caller(caller)
            .kind(revm::primitives::TxKind::Call(to))
            .data(data)
            .gas_limit(8_000_000)
            .gas_price(0)
            .build();
        let Ok(tx) = tx else {
            return DryVerdict::Db("tx build failed".into());
        };
        match evm.transact(tx) {
            Ok(res) => match res.result {
                ExecutionResult::Success { output, .. } => {
                    DryVerdict::Ok(Bytes::from(output.into_data().to_vec()))
                }
                ExecutionResult::Revert { output, .. } => DryVerdict::Revert(format!(
                    "revert 0x{}",
                    alloy::primitives::hex::encode(&output)
                )),
                ExecutionResult::Halt { reason, .. } => {
                    DryVerdict::Revert(format!("halt {reason:?}"))
                }
            },
            Err(e) => DryVerdict::Db(format!("{e:?}")),
        }
    })
    .await;
    match out {
        Ok(v) => v,
        Err(e) => DryVerdict::Db(format!("join: {e}")),
    }
}

/// `REVM_DRYRUN=true` gate — read once at startup of each check.
pub fn enabled() -> bool {
    std::env::var("REVM_DRYRUN").is_ok_and(|v| v == "true" || v == "1")
}
