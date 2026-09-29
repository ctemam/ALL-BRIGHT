//! MultiCall3 batch reads.
//!
//! The scanner previously issued one `eth_call` per DEX. With 10-15 DEXes per
//! chain across 6 chains that is 60-90 sequential round trips per scan, which
//! is both slow and a reliable way to hit free-endpoint rate limits.
//!
//! MultiCall3 (`0xcA11bde05977b3631167028862bE2a173976CA11`) is deployed at the
//! same address on every EVM chain, so a single batched `eth_call` can read
//! reserves for many pools at once. This module encodes `aggregate3` calldata
//! and decodes the packed `(bool, bytes)` return array.
//!
//! Solidity reference:
//! ```solidity
//! struct Call3 { address target; bool allowFailure; bytes callData; }
//! struct Result3 { bool success; bytes returnData; }
//! function aggregate3(Call3[] calldata calls) public payable returns (Result3[] memory returnData);
//! ```

use alloy::primitives::{Address, Bytes, U256};
use alloy::providers::Provider;

/// Canonical MultiCall3 deployment, identical across chains.
pub const MULTICALL3_ADDRESS: Address = Address::new([
    0xca, 0x11, 0xbd, 0xe0, 0x59, 0x77, 0xb3, 0x63, 0x11, 0x67, 0x02, 0x88, 0x62, 0xbe, 0x2a, 0x17,
    0x39, 0x76, 0xca, 0x11,
]);

/// One sub-call within a batch.
#[derive(Debug, Clone)]
pub struct SubCall {
    pub target: Address,
    pub call_data: Vec<u8>,
    /// When true a reverting sub-call is tolerated and returned as
    /// `success = false` instead of reverting the whole batch.
    pub allow_failure: bool,
}

impl SubCall {
    pub fn new(target: Address, call_data: Vec<u8>) -> Self {
        Self {
            target,
            call_data,
            // A single bad pool must not void the read of every other pool.
            allow_failure: true,
        }
    }
}

/// Outcome of one sub-call.
#[derive(Debug, Clone)]
pub struct SubResult {
    pub success: bool,
    pub return_data: Vec<u8>,
}

/// `token0()` / `token1()` â€” pool token ordering.
pub fn token0_call_data() -> Vec<u8> {
    let mut data = Vec::with_capacity(4);
    data.extend_from_slice(&alloy::primitives::keccak256("token0()")[..4]);
    data
}

pub fn token1_call_data() -> Vec<u8> {
    let mut data = Vec::with_capacity(4);
    data.extend_from_slice(&alloy::primitives::keccak256("token1()")[..4]);
    data
}

pub fn get_reserves_call_data() -> Vec<u8> {
    let mut data = Vec::with_capacity(4);
    data.extend_from_slice(&alloy::primitives::keccak256("getReserves()")[..4]);
    data
}

pub fn slot0_call_data() -> Vec<u8> {
    let mut data = Vec::with_capacity(4);
    data.extend_from_slice(&alloy::primitives::keccak256("slot0()")[..4]);
    data
}

/// `liquidity()` - a V3 pool's in-range `L`, used as a depth proxy.
pub fn liquidity_call_data() -> Vec<u8> {
    let mut data = Vec::with_capacity(4);
    data.extend_from_slice(&alloy::primitives::keccak256("liquidity()")[..4]);
    data
}

pub fn decimals_call_data() -> Vec<u8> {
    let mut data = Vec::with_capacity(4);
    data.extend_from_slice(&alloy::primitives::keccak256("decimals()")[..4]);
    data
}

/// ABI-encode `aggregate3(Call3[])` calldata.
///
/// Layout: selector, then a dynamic array of 3-field structs. Each struct is
/// head-encoded (target as a left-padded address, allowFailure as a word,
/// callData offset) with the bytes tail-encoded after the struct array.
pub fn encode_aggregate3(calls: &[SubCall]) -> Bytes {
    let n = calls.len();
    let mut out = Vec::new();
    out.extend_from_slice(&alloy::primitives::keccak256("aggregate3((address,bool,bytes)[])")[..4]);

    // Head of the parameter block: the argument is a dynamic array, so the
    // first word is its offset from the start of the args (always 0x20).
    out.extend_from_slice(&word_u64(32));
    // Then the array length.
    out.extend_from_slice(&word_u64(n as u64));

    // Each element is a *dynamic* tuple, so the array body starts with a table
    // of offsets, one per element, measured from the first byte AFTER the
    // length word. The table itself occupies `n` words, so the first element
    // begins at `n * 32` — not at 0.
    //
    // For one element this must emit offset[0] == 0x20, not 0. Pinned against
    // foundry's encoder in `encode_matches_foundry_reference_bytes`.
    let head_and_len = 3 * 32 + 32;
    let table_bytes = n as u64 * 32;
    let mut offsets = Vec::with_capacity(n);
    let mut cursor: u64 = table_bytes;
    for c in calls {
        offsets.push(cursor);
        // The payload occupies `len + padding` bytes, where the padding rounds
        // it up to a 32-byte multiple. Counting only the padding would
        // under-count each element and corrupt every later offset.
        let padded = c.call_data.len() as u64 + pad32(c.call_data.len()) as u64;
        cursor += head_and_len as u64 + padded;
    }
    for off in &offsets {
        out.extend_from_slice(&word_u64(*off));
    }

    for c in calls {
        // Tuple head: address (left-padded), allowFailure, offset to bytes
        // (relative to the start of this tuple = 3 words).
        out.extend_from_slice(&[0u8; 12]);
        out.extend_from_slice(c.target.as_slice());
        out.extend_from_slice(&word_u64(u64::from(c.allow_failure)));
        out.extend_from_slice(&word_u64(96));
        // Tuple tail: length + padded payload.
        out.extend_from_slice(&word_u64(c.call_data.len() as u64));
        out.extend_from_slice(&c.call_data);
        out.extend(std::iter::repeat_n(0u8, pad32(c.call_data.len())));
    }

    Bytes::from(out)
}

/// Number of zero bytes needed to round `len` up to a 32-byte boundary.
fn pad32(len: usize) -> usize {
    (32 - (len % 32)) % 32
}

/// Decode the `Result3[]` returned by `aggregate3`.
///
/// Each element is `(bool success, bytes returnData)`. A reverting sub-call is
/// tolerated: it decodes as `success = false` with empty data, so one bad pool
/// cannot invalidate the rest of the batch.
pub fn decode_aggregate3(return_data: &Bytes, num_calls: usize) -> Vec<SubResult> {
    let raw = return_data.as_ref();
    let mut out = Vec::with_capacity(num_calls);

    // Need at least: 32-byte array length + one offset word.
    if raw.len() < 64 {
        return out;
    }
    // The first word is the array length; offsets are relative to the end of it.
    let base = 32usize;
    // Offsets may not be in ascending order in malformed data, but they are
    // sequential in practice; read each element at its own declared offset.
    for i in 0..num_calls {
        let off_pos = base + i * 32;
        if off_pos + 32 > raw.len() {
            break;
        }
        let rel = read_word(raw, off_pos);
        let elem = match usize::try_from(rel) {
            Ok(v) => base.checked_add(v),
            Err(_) => None,
        };
        let Some(elem) = elem else {
            out.push(SubResult {
                success: false,
                return_data: Vec::new(),
            });
            continue;
        };
        if elem + 64 > raw.len() {
            out.push(SubResult {
                success: false,
                return_data: Vec::new(),
            });
            continue;
        }

        let success = read_word(raw, elem) != 0;
        // Offset of returnData within the tuple, relative to the tuple start.
        let data_rel = match usize::try_from(read_word(raw, elem + 32)) {
            Ok(v) => v,
            Err(_) => {
                out.push(SubResult {
                    success,
                    return_data: Vec::new(),
                });
                continue;
            }
        };
        let Some(data_at) = elem.checked_add(data_rel) else {
            out.push(SubResult {
                success,
                return_data: Vec::new(),
            });
            continue;
        };
        if data_at + 32 > raw.len() {
            out.push(SubResult {
                success,
                return_data: Vec::new(),
            });
            continue;
        }
        let len = match usize::try_from(read_word(raw, data_at)) {
            Ok(v) => v,
            Err(_) => {
                out.push(SubResult {
                    success,
                    return_data: Vec::new(),
                });
                continue;
            }
        };
        let start = data_at + 32;
        let end = start.saturating_add(len);
        let bytes = if end <= raw.len() {
            raw[start..end].to_vec()
        } else {
            Vec::new()
        };
        out.push(SubResult {
            success,
            return_data: bytes,
        });
    }

    out
}

/// Encode a `u64` as a 32-byte ABI word.
///
/// Written by hand rather than via `U256::to_be_bytes`, whose byte order is
/// version-dependent in ruint; ABI encoding must be unambiguous big-endian.
fn word_u64(v: u64) -> [u8; 32] {
    let mut w = [0u8; 32];
    w[24..].copy_from_slice(&v.to_be_bytes());
    w
}

/// Read a 32-byte big-endian word at `pos`, or zero if out of bounds.
///
/// Returns only the low 8 bytes. That is correct for ABI offsets and array
/// lengths (always small) but it must NOT be used for on-chain amounts: see
/// `read_u256`.
fn read_word(raw: &[u8], pos: usize) -> u64 {
    if pos + 32 > raw.len() {
        return 0;
    }
    let mut b = [0u8; 8];
    b.copy_from_slice(&raw[pos + 24..pos + 32]);
    u64::from_be_bytes(b)
}

/// Read a full 32-byte big-endian word at `pos` as a `U256`, or zero if the
/// payload is short.
///
/// Every on-chain amount goes through this. `read_word` truncates to the low 8
/// bytes, which silently corrupted real values: a `getReserves()` side holding
/// 100 WETH is 1e20 base units, more than `u64::MAX` (~1.8e19), so the reserve
/// was cut down and the price derived from it was fabricated. A V3
/// `sqrtPriceX96` is a `uint160` and would lose almost all of its significant
/// bits the same way.
fn read_u256(raw: &[u8], pos: usize) -> U256 {
    if pos + 32 > raw.len() {
        return U256::ZERO;
    }
    U256::from_be_slice(&raw[pos..pos + 32])
}

/// Decoded `getReserves()`: (reserve0, reserve1).
///
/// Full-width `U256`: a reserve legitimately exceeds `u64::MAX` (100 WETH is
/// 1e20 base units), and the old narrow decode cut it down, fabricating the
/// price derived from it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reserves {
    pub reserve0: U256,
    pub reserve1: U256,
}

/// Decode `getReserves()` return data. Returns `None` if the payload is short.
pub fn decode_reserves(data: &[u8]) -> Option<Reserves> {
    if data.len() < 96 {
        return None;
    }
    Some(Reserves {
        reserve0: read_u256(data, 0),
        reserve1: read_u256(data, 32),
    })
}

/// Decoded `slot0()`: (sqrtPriceX96, tick).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Slot0 {
    pub sqrt_price_x96: U256,
    pub tick: i32,
}

/// Decode `slot0()` return data.
///
/// `sqrtPriceX96` is a `uint160` and must be read full width, or nearly every
/// significant bit is lost. `tick` is a signed int24 carried in the low three
/// bytes of the second word, so it is sign-extended from `data[29..32]`.
pub fn decode_slot0(data: &[u8]) -> Option<Slot0> {
    if data.len() < 64 {
        return None;
    }
    // `tick` is an int24 in the low 3 bytes of the second word, and must be
    // sign-extended into i32: a tick of -1 is 0xFFFFFF, which reads as
    // +16777215 unless the sign bit is propagated. The previous version copied
    // 3 bytes into a 4-byte array, which panicked outright on every call.
    let low = [data[29], data[30], data[31]];
    let mut t = [0u8; 4];
    t[1..].copy_from_slice(&low);
    if low[0] & 0x80 != 0 {
        t[0] = 0xFF;
    }
    Some(Slot0 {
        sqrt_price_x96: read_u256(data, 0),
        tick: i32::from_be_bytes(t),
    })
}

/// Decode a V3 pool's in-range liquidity `L`.
pub fn decode_liquidity(data: &[u8]) -> Option<U256> {
    if data.len() < 32 {
        return None;
    }
    Some(read_u256(data, 0))
}

/// Decode a 32-byte left-padded ABI address word. Zero means "no pool".
pub fn decode_address(data: &[u8]) -> Option<Address> {
    if data.len() < 32 {
        return None;
    }
    let word = U256::from_be_slice(&data[..32]);
    if word.is_zero() {
        return None;
    }
    // An ABI address word is left-padded, so the address is the low 20 bytes.
    // `Address::from([u8; 32])` is not a valid conversion.
    let mut bytes = [0u8; 20];
    bytes.copy_from_slice(&word.to_be_bytes::<32>()[12..]);
    Some(Address::new(bytes))
}

/// Decode `decimals()` return data. Returns `None` if the payload is short or
/// the value does not fit in a u8.
pub fn decode_decimals(data: &[u8]) -> Option<u8> {
    if data.len() < 32 {
        return None;
    }
    u8::try_from(read_word(data, 0)).ok()
}

/// Convert a raw uint256 amount to a human-readable float by its decimals.
pub fn scale_amount(raw: U256, decimals: u8) -> f64 {
    let divisor = 10f64.powi(decimals as i32);
    if divisor == 0.0 || !divisor.is_finite() {
        return 0.0;
    }
    match raw.to_string().parse::<f64>() {
        Ok(v) => v / divisor,
        Err(_) => 0.0,
    }
}

/// Derive a token price denominated in WETH from constant-product reserves.
///
/// `price_token_in_weth = reserve_weth / reserve_token` after both sides are
/// scaled to human units. Returns 0.0 for an empty pool rather than a
/// fabricated price, so callers can treat 0.0 as "no data".
pub fn price_from_reserves(
    reserve_token: U256,
    reserve_weth: U256,
    token_decimals: u8,
    weth_decimals: u8,
) -> f64 {
    if reserve_token.is_zero() || reserve_weth.is_zero() {
        return 0.0;
    }
    let rt = scale_amount(reserve_token, token_decimals);
    let rw = scale_amount(reserve_weth, weth_decimals);
    if rt <= 0.0 || rw <= 0.0 {
        return 0.0;
    }
    rw / rt
}

/// 2^96, the fixed-point scale in a V3 pool's `sqrtPriceX96`.
const Q96: f64 = 79_228_162_514_264_337_593_543_950_336.0;

/// Convert a full-width `sqrtPriceX96` to the raw `token1/token0` ratio.
///
/// Returns 0.0 for a zero or unrepresentable value so callers can treat 0.0 as
/// "no data" rather than pricing against a fabricated ratio.
pub fn sqrt_price_ratio(sqrt_price_x96: U256) -> f64 {
    if sqrt_price_x96.is_zero() {
        return 0.0;
    }
    let s = match sqrt_price_x96.to_string().parse::<f64>() {
        Ok(v) => v,
        Err(_) => return 0.0,
    };
    if !s.is_finite() || s <= 0.0 {
        return 0.0;
    }
    s / Q96
}

/// Price of `token` in `quote`, both in human units, from a V3 `sqrtPriceX96`.
///
/// `sqrtPriceX96` encodes `sqrt(token1/token0)` in *raw* base units, so the
/// decimals of both sides must be applied to get a human price. `token_is_token0`
/// says which side of the pool `token` is on; it is derived from address
/// ordering (`token0` is always the lower address), which is deterministic and
/// needs no extra RPC call.
///
/// Returns 0.0 on any degenerate input rather than a plausible-looking number.
pub fn v3_price_in_quote(
    sqrt_price_x96: U256,
    token_decimals: u8,
    quote_decimals: u8,
    token_is_token0: bool,
) -> f64 {
    let ratio = sqrt_price_ratio(sqrt_price_x96);
    if ratio <= 0.0 {
        return 0.0;
    }
    let price_raw = ratio * ratio;
    if !price_raw.is_finite() || price_raw <= 0.0 {
        return 0.0;
    }
    let (dec0, dec1) = if token_is_token0 {
        (token_decimals, quote_decimals)
    } else {
        (quote_decimals, token_decimals)
    };
    let pow0 = 10f64.powi(dec0 as i32);
    let pow1 = 10f64.powi(dec1 as i32);
    let price = if token_is_token0 {
        // `token` is token0, so `quote` is token1: the raw ratio is quote-per-token.
        price_raw * pow0 / pow1
    } else {
        // `token` is token1, so the raw ratio is token-per-quote; invert it.
        pow1 / (price_raw * pow0)
    };
    if !price.is_finite() || price <= 0.0 {
        return 0.0;
    }
    price
}

/// Current-tick depth of one side of a V3 pool, in `quote` units.
///
/// A V3 pool does not hold reserves; it holds in-range liquidity `L`. The
/// virtual reserves implied by `L` at the current tick are `x0 = L / sqrtP` and
/// `x1 = L * sqrtP` (raw units). This returns twice the quote-side virtual
/// reserve, matching the `reserve_quote * 2` convention used for V2 depth.
///
/// NOTE: this is the depth at the *current tick only*, not the depth available
/// across the full range a large trade would cross. It is an honest lower bound
/// for comparing venues, not a slippage model.
pub fn v3_depth_in_quote(
    sqrt_price_x96: U256,
    liquidity: U256,
    quote_decimals: u8,
    token_is_token0: bool,
) -> f64 {
    let ratio = sqrt_price_ratio(sqrt_price_x96);
    if ratio <= 0.0 || liquidity.is_zero() {
        return 0.0;
    }
    let l = match liquidity.to_string().parse::<f64>() {
        Ok(v) => v,
        Err(_) => return 0.0,
    };
    if !l.is_finite() || l <= 0.0 {
        return 0.0;
    }
    // Quote side is token1 when the token is token0, and token0 otherwise.
    let quote_raw = if token_is_token0 {
        l * ratio
    } else {
        l / ratio
    };
    if !quote_raw.is_finite() || quote_raw <= 0.0 {
        return 0.0;
    }
    let scaled = quote_raw / 10f64.powi(quote_decimals as i32);
    if !scaled.is_finite() || scaled <= 0.0 {
        return 0.0;
    }
    scaled * 2.0
}

/// ABI-encode an address as a left-padded 32-byte word.
///
/// Both addresses in a two-address call need this. Emitting a bare 20-byte
/// address makes the calldata 12 bytes short of the 68 that
/// `f( address, address )` requires, and Solidity's decoder reverts on
/// truncated calldata rather than padding it.
fn address_word(a: Address) -> [u8; 32] {
    let mut w = [0u8; 32];
    w[12..].copy_from_slice(a.as_slice());
    w
}

pub fn factory_get_pair_call_data(a: Address, b: Address) -> Vec<u8> {
    let mut data = Vec::with_capacity(68);
    data.extend_from_slice(&alloy::primitives::keccak256("getPair(address,address)")[..4]);
    data.extend_from_slice(&address_word(a));
    data.extend_from_slice(&address_word(b));
    data
}

/// ABI-encode a V3 factory's pool lookup.
///
/// `sig` is passed in rather than hardcoded because the two V3 families in the
/// registry use different third-argument types, and the 4-byte selector is
/// derived from the signature *text*: `getPool(address,address,uint24)` for the
/// Uniswap V3 family, `getPool(address,address,int24)` for Velodrome Slipstream
/// (which keys pools by tick spacing, not fee). Those are different selectors,
/// so reusing one for the other silently returns the zero address.
pub fn factory_get_pool_call_data(a: Address, b: Address, fee: u32, sig: &str) -> Vec<u8> {
    let mut data = Vec::with_capacity(100);
    data.extend_from_slice(&alloy::primitives::keccak256(sig.as_bytes())[..4]);
    data.extend_from_slice(&address_word(a));
    data.extend_from_slice(&address_word(b));
    // `fee` / tick spacing fits in 3 bytes (`uint24` / `int24`), left-padded to
    // a full ABI word. Every value in the registry is far below 2^23, so the
    // int24 and uint24 encodings coincide and no sign handling is needed.
    let mut fee_word = [0u8; 32];
    fee_word[28..32].copy_from_slice(&fee.to_be_bytes());
    data.extend_from_slice(&fee_word);
    data
}

/// Execute a batch through MultiCall3 and return the decoded per-call results.
///
/// This is the single network round trip the scanner uses to read many pools at
/// once. Individual sub-calls are allowed to fail, so one broken pool never
/// invalidates the batch.
pub async fn execute(
    provider: &crate::radar_scanner::HttpProvider,
    calls: &[SubCall],
) -> Result<Vec<SubResult>, Box<dyn std::error::Error + Send + Sync>> {
    if calls.is_empty() {
        return Ok(Vec::new());
    }
    let calldata = encode_aggregate3(calls);
    // Use `raw_request` rather than the `Provider::call` trait method: the
    // latter requires the eth_call request/response traits, and we want the
    // raw returndata exactly as MultiCall3 emitted it.
    let request = serde_json::json!({
        "to": MULTICALL3_ADDRESS,
        "data": format!("0x{}", hex::encode(&calldata)),
    });
    let raw: serde_json::Value = provider.client().request("eth_call", [request]).await?;
    let hex_str = raw
        .as_str()
        .ok_or("multicall3 returned a non-hex result")?
        .trim_start_matches("0x");
    let bytes = hex::decode(hex_str).map_err(|e| format!("bad multicall3 hex: {e}"))?;

    // MultiCall3's aggregate3 returns `Result3[]` directly, with no
    // surrounding ABI wrapper.
    Ok(decode_aggregate3(&Bytes::from(bytes), calls.len()))
}

// NOTE: pool orientation (`token0()`) is read inside the scanner's single
// batched MultiCall3 phase-2 call rather than through a per-pool helper, so a
// pool whose orientation cannot be read is skipped instead of priced. The
// previous per-pool helper fell back to "assume token0" on error, which
// inverted the price on a reversed pool and reported a spread built from a read
// that never succeeded. See `quote_venues_on_chain` in `radar_scanner.rs`.

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(n: u8) -> Address {
        Address::new([n; 20])
    }

    /// Build a `Result3[]` return payload for test decoding.
    ///
    /// ABI offsets in a dynamic array are relative to the start of the offset
    /// table (i.e. immediately after the length word), not to the start of the
    /// element data. So element `i`'s offset includes the table itself:
    /// `32 * n + sum(sizes of preceding elements)`. Each element is
    /// `(bool, bytes)` — 96 bytes of head/tail plus the padded payload.
    fn encode_results(items: &[(bool, Vec<u8>)]) -> Bytes {
        let table_len = 32 * items.len() as u64;

        let mut layout: Vec<(u64, u64)> = Vec::with_capacity(items.len());
        let mut cursor = table_len;
        for (_, data) in items {
            let size = 96 + ((32 - (data.len() % 32)) % 32) as u64;
            layout.push((cursor, size));
            cursor += size;
        }

        let mut out = Vec::new();
        out.extend_from_slice(&word_u64(items.len() as u64));
        for (offset, _) in &layout {
            out.extend_from_slice(&word_u64(*offset));
        }
        for ((success, data), _) in items.iter().zip(layout.iter()) {
            out.extend_from_slice(&word_u64(u64::from(*success)));
            out.extend_from_slice(&word_u64(64)); // offset to bytes, within the tuple
            out.extend_from_slice(&word_u64(data.len() as u64));
            out.extend_from_slice(data);
            let pad = (32 - (data.len() % 32)) % 32;
            out.extend(std::iter::repeat_n(0u8, pad));
        }
        Bytes::from(out)
    }

    #[test]
    fn encode_starts_with_aggregate3_selector() {
        let calls = vec![SubCall::new(addr(1), get_reserves_call_data())];
        let data = encode_aggregate3(&calls);
        let expected = &alloy::primitives::keccak256("aggregate3((address,bool,bytes)[])")[..4];
        assert_eq!(&data[..4], expected);
    }

    #[test]
    fn encode_empty_batch_is_still_valid_calldata() {
        let data = encode_aggregate3(&[]);
        // selector(4) + array offset(32) + length(32) = 68 bytes.
        assert_eq!(data.len(), 68);
        assert!(decode_aggregate3(&data, 0).is_empty());
    }

    /// The encoder must agree with an independent ABI encoder byte-for-byte.
    ///
    /// `REFERENCE` was produced by foundry and is reproducible with:
    ///
    /// ```text
    /// cast calldata "aggregate3((address,bool,bytes)[])" \
    ///   "[(0x00000000000000000000000000000000000000AA,true,0x0102),
    ///     (0x00000000000000000000000000000000000000BB,true,0x0902f1ac)]"
    /// ```
    ///
    /// This is pinned against a second implementation rather than against
    /// reasoning about the ABI, because the failure mode of a wrong offset table
    /// is an aggregator that reverts, which the scanner surfaces as "no pools
    /// found" instead of as the encoding bug it actually is.
    #[test]
    fn encode_matches_foundry_reference_bytes() {
        const REFERENCE: &str = "82ad56cb00000000000000000000000000000000000000000000000000000000000000200000000000000000000000000000000000000000000000000000000000000002000000000000000000000000000000000000000000000000000000000000004000000000000000000000000000000000000000000000000000000000000000e000000000000000000000000000000000000000000000000000000000000000aa000000000000000000000000000000000000000000000000000000000000000100000000000000000000000000000000000000000000000000000000000000600000000000000000000000000000000000000000000000000000000000000002010200000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000bb0000000000000000000000000000000000000000000000000000000000000001000000000000000000000000000000000000000000000000000000000000006000000000000000000000000000000000000000000000000000000000000000040902f1ac00000000000000000000000000000000000000000000000000000000";
        // Built explicitly: the `addr` helper fills all twenty bytes, which would
        // not match the single-byte addresses foundry was given above.
        let a: Address = "0x00000000000000000000000000000000000000AA"
            .parse()
            .expect("valid address");
        let b: Address = "0x00000000000000000000000000000000000000BB"
            .parse()
            .expect("valid address");
        let calls = vec![
            SubCall::new(a, vec![0x01, 0x02]), // 2 bytes -> one padded word
            SubCall::new(b, get_reserves_call_data()), // 4 bytes -> one padded word
        ];
        assert_eq!(hex::encode(encode_aggregate3(&calls)), REFERENCE);
    }

    /// Guards the full ABI layout, which a selector-only check would miss:
    /// selector, 0x20 array offset, length, then a per-element offset table
    /// (dynamic tuples), then the tuple head/tail encodings. The offsets are
    /// spelled out here so a failure names the word that moved;
    /// [`encode_matches_foundry_reference_bytes`] is the authority on the bytes.
    #[test]
    fn encode_abi_layout_is_correct() {
        let calls = vec![
            SubCall::new(addr(0xAA), vec![0x01, 0x02]), // 2 bytes -> one padded word
            SubCall::new(addr(0xBB), get_reserves_call_data()), // 4 bytes -> one padded word
        ];
        let d = encode_aggregate3(&calls);
        let w = |i: usize| read_word(&d, i);

        assert_eq!(
            &d[..4],
            &alloy::primitives::keccak256("aggregate3((address,bool,bytes)[])")[..4]
        );
        assert_eq!(w(4), 32, "array offset must be 0x20");
        assert_eq!(w(36), 2, "array length");

        // Layout: selector(4) | 0x20(32) | length(32) | offsets(2*32) | tuples.
        // The offset table starts at byte 68 and occupies 2 words, so the first
        // element's data begins at 68 + 64 = 132. The first offset is therefore
        // 64 (0x40), NOT 0 — the table itself is skipped.
        assert_eq!(w(68), 64, "element 0 offset");
        // Each element is 3 head words (96) + 1 length word (32) + 1 padded
        // payload word (32) = 160.
        assert_eq!(w(100), 64 + 160, "element 1 offset");

        // Within a tuple head: the address is left-padded into 32 bytes, so the
        // 20 address bytes sit at head+12 .. head+32.
        let head0 = 132;
        assert_eq!(&d[head0 + 12..head0 + 32], &[0xAA; 20], "element 0 target");
        assert_eq!(w(head0 + 32), 1, "element 0 allowFailure");
        assert_eq!(w(head0 + 64), 96, "element 0 offset-to-bytes");
        assert_eq!(w(head0 + 96), 2, "element 0 callData length");
        assert_eq!(
            &d[head0 + 128..head0 + 130],
            &[0x01, 0x02],
            "element 0 payload"
        );

        let head1 = 132 + 160;
        assert_eq!(&d[head1 + 12..head1 + 32], &[0xBB; 20], "element 1 target");
        assert_eq!(w(head1 + 32), 1, "element 1 allowFailure");
        assert_eq!(w(head1 + 64), 96, "element 1 offset-to-bytes");
        assert_eq!(w(head1 + 96), 4, "element 1 callData length");
        assert_eq!(
            &d[head1 + 128..head1 + 132],
            &get_reserves_call_data()[..],
            "element 1 payload"
        );
    }

    #[test]
    fn encode_includes_target_and_selector() {
        let target = addr(0xAB);
        let inner = get_reserves_call_data();
        let data = encode_aggregate3(&[SubCall::new(target, inner.clone())]);
        assert!(data.as_ref().windows(20).any(|w| w == target.as_slice()));
        assert!(data.as_ref().windows(4).any(|w| w == inner.as_slice()));
    }

    #[test]
    fn decode_round_trips_success_with_data() {
        let payload = encode_results(&[(true, vec![0xAA; 64])]);
        let out = decode_aggregate3(&payload, 1);
        assert_eq!(out.len(), 1);
        assert!(out[0].success);
        assert_eq!(out[0].return_data.len(), 64);
        assert_eq!(out[0].return_data[0], 0xAA);
    }

    #[test]
    fn decode_tolerates_failed_subcall() {
        // A reverting pool must not invalidate the rest of the batch.
        let payload = encode_results(&[(false, vec![]), (true, vec![0x11; 32])]);
        let out = decode_aggregate3(&payload, 2);
        assert_eq!(out.len(), 2);
        assert!(!out[0].success);
        assert!(out[0].return_data.is_empty());
        assert!(out[1].success);
        assert_eq!(out[1].return_data.len(), 32);
    }

    #[test]
    fn decode_handles_truncated_payload_without_panicking() {
        for n in [0usize, 1, 8, 31, 32, 33, 64] {
            let truncated = Bytes::from(vec![0u8; n]);
            let _ = decode_aggregate3(&truncated, 4); // must not panic
        }
    }

    #[test]
    fn decode_reserves_reads_both_words() {
        let mut data = vec![0u8; 96];
        data[0..32].copy_from_slice(&word_u64(1_000));
        data[32..64].copy_from_slice(&word_u64(2_000));
        let r = decode_reserves(&data).expect("decodes");
        assert_eq!(r.reserve0, U256::from(1_000u64));
        assert_eq!(r.reserve1, U256::from(2_000u64));
    }

    /// Reserves routinely exceed `u64::MAX` (100 WETH is 1e20 base units). The
    /// old narrow decode kept only the low 8 bytes, fabricating the price
    /// derived from the truncated value.
    #[test]
    fn decode_reserves_keeps_values_wider_than_u64() {
        let hundred_weth: U256 = U256::from(10u64).pow(U256::from(20));
        assert!(hundred_weth > U256::from(u64::MAX), "test premise");
        let mut data = vec![0u8; 96];
        data[0..32].copy_from_slice(&hundred_weth.to_be_bytes::<32>());
        data[32..64].copy_from_slice(&word_u64(7));
        let r = decode_reserves(&data).expect("decodes");
        assert_eq!(r.reserve0, hundred_weth, "reserve was truncated");
        assert_eq!(r.reserve1, U256::from(7u64));
    }

    /// `sqrtPriceX96` is a `uint160`; a narrow read returns garbage.
    #[test]
    fn decode_slot0_keeps_full_width_sqrt_price() {
        // 2^96 is price 1 for an equal-decimals pair.
        let one: U256 = U256::from(2u64).pow(U256::from(96));
        let mut data = vec![0u8; 64];
        data[0..32].copy_from_slice(&one.to_be_bytes::<32>());
        let s = decode_slot0(&data).expect("decodes");
        assert_eq!(s.sqrt_price_x96, one);
        assert_eq!(s.tick, 0);
        assert!(
            s.sqrt_price_x96 > U256::from(u64::MAX),
            "sqrt price must not be truncated"
        );
    }

    #[test]
    fn decode_slot0_sign_extends_negative_tick() {
        let mut data = vec![0u8; 64];
        // -1 as a two's-complement int24 in the low three bytes.
        data[29..32].copy_from_slice(&[0xff, 0xff, 0xff]);
        let s = decode_slot0(&data).expect("decodes");
        assert_eq!(s.tick, -1, "int24 must be sign-extended");
    }

    #[test]
    fn decode_liquidity_reads_full_width() {
        let l: U256 = U256::from(10u64).pow(U256::from(24));
        let mut data = vec![0u8; 32];
        data.copy_from_slice(&l.to_be_bytes::<32>());
        assert_eq!(decode_liquidity(&data), Some(l));
        assert!(decode_liquidity(&[0u8; 16]).is_none());
    }

    #[test]
    fn decode_address_rejects_zero_and_reads_low_20_bytes() {
        let mut data = vec![0u8; 32];
        assert!(decode_address(&data).is_none(), "zero means no pool");
        data[12..32].copy_from_slice(&[0xABu8; 20]);
        assert_eq!(decode_address(&data), Some(Address::new([0xABu8; 20])));
    }

    /// `getPair(address,address)` needs exactly two left-padded 32-byte words.
    /// Emitting a bare 20-byte address leaves the calldata 12 bytes short, and
    /// Solidity's decoder reverts on truncated calldata, so no pool would ever
    /// resolve and every V2 venue would silently report "no market".
    #[test]
    fn get_pair_calldata_is_two_full_abi_words() {
        let a = addr(1);
        let b = addr(2);
        let data = factory_get_pair_call_data(a, b);
        assert_eq!(data.len(), 68, "selector + 2 ABI words");
        assert_eq!(
            &data[..4],
            &alloy::primitives::keccak256("getPair(address,address)")[..4]
        );
        assert_eq!(data[4..16], [0u8; 12], "first address must be left-padded");
        assert_eq!(data[16..36], [1u8; 20]);
        // The second address needs the same padding; omitting it was the bug.
        assert_eq!(
            data[36..48],
            [0u8; 12],
            "second address must be left-padded"
        );
        assert_eq!(data[48..68], [2u8; 20]);
    }

    /// The two V3 families take different third arguments, so their selectors
    /// differ. Reusing one for the other resolves no pool at all.
    #[test]
    fn get_pool_selectors_differ_between_v3_families() {
        let a = addr(1);
        let b = addr(2);
        let uni = factory_get_pool_call_data(a, b, 500, "getPool(address,address,uint24)");
        let slip = factory_get_pool_call_data(a, b, 500, "getPool(address,address,int24)");
        assert_ne!(
            uni[..4],
            slip[..4],
            "uint24 and int24 selectors must differ"
        );
        assert_eq!(uni.len(), 100, "selector + 3 ABI words");
        // Words: [4..36] = a, [36..68] = b, [68..100] = tier.
        assert_eq!(uni[36..48], [0u8; 12], "second address must be left-padded");
        assert_eq!(uni[68..96], [0u8; 28], "tier must be left-padded");
        assert_eq!(uni[96..100], 500u32.to_be_bytes());
    }

    /// A V3 quote must apply both decimals and the pool's price orientation.
    #[test]
    fn v3_price_matches_sqrt_price_and_decimals() {
        let q96: U256 = U256::from(2u64).pow(U256::from(96));

        // Price 1 for an equal-decimals pair, either orientation.
        assert!((v3_price_in_quote(q96, 18, 18, true) - 1.0).abs() < 1e-12);
        assert!((v3_price_in_quote(q96, 18, 18, false) - 1.0).abs() < 1e-12);

        // USDC(6)/WETH(18) at 3000 USDC per WETH: the raw ratio is 1e18 / 3e9.
        let raw = 1e18f64 / 3e9f64;
        let q96 = (2f64).powi(96);
        let sqrt = (raw.sqrt() * q96).round() as u128;
        let sqrt_u = U256::from(sqrt);

        // token = WETH = token1, quote = USDC = token0 -> 3000 USDC per WETH.
        let price_weth = v3_price_in_quote(sqrt_u, 18, 6, false);
        assert!(
            (price_weth - 3000.0).abs() < 1.0,
            "expected ~3000 USDC per WETH, got {price_weth}"
        );
        // token = USDC = token0, quote = WETH = token1 -> ~1/3000 WETH per USDC.
        let price_usdc = v3_price_in_quote(sqrt_u, 6, 18, true);
        assert!(
            (price_usdc - 1.0 / 3000.0).abs() < 1e-6,
            "expected ~1/3000 WETH per USDC, got {price_usdc}"
        );
    }

    /// Degenerate input must yield 0.0 ("no data"), never a plausible number.
    #[test]
    fn v3_math_returns_zero_on_degenerate_input() {
        let q96: U256 = U256::from(2u64).pow(U256::from(96));
        assert_eq!(v3_price_in_quote(U256::ZERO, 18, 18, true), 0.0);
        assert_eq!(
            v3_depth_in_quote(U256::ZERO, U256::from(1u64), 18, true),
            0.0
        );
        assert_eq!(v3_depth_in_quote(q96, U256::ZERO, 18, true), 0.0);
    }

    #[test]
    fn sqrt_price_ratio_is_scale_correct() {
        let q96: U256 = U256::from(2u64).pow(U256::from(96));
        assert!((sqrt_price_ratio(q96) - 1.0).abs() < 1e-12);
        let two_x: U256 = q96 * U256::from(2u64);
        assert!((sqrt_price_ratio(two_x) - 2.0).abs() < 1e-12);
    }

    #[test]
    fn decode_reserves_rejects_short_data() {
        assert!(decode_reserves(&[0u8; 64]).is_none());
    }

    #[test]
    fn decode_decimals_reads_value() {
        let mut data = vec![0u8; 32];
        data.copy_from_slice(&word_u64(18));
        assert_eq!(decode_decimals(&data), Some(18));
    }

    #[test]
    fn decode_decimals_rejects_out_of_range() {
        let mut data = vec![0u8; 32];
        data.copy_from_slice(&word_u64(300));
        assert_eq!(decode_decimals(&data), None);
    }

    #[test]
    fn scale_amount_applies_decimals() {
        assert!((scale_amount(U256::from(1_000_000u64), 6) - 1.0).abs() < 1e-9);
        assert!((scale_amount(U256::from(100_000_000_000_000_000u64), 18) - 0.1).abs() < 1e-12);
    }

    #[test]
    fn price_from_reserves_is_reserve_ratio() {
        // 1000 tokens (18dp) against 1 WETH (18dp) => 0.001 WETH per token.
        // Note: 1000 * 10^18 overflows u64, so build the U256 by scaling.
        let reserve_token = U256::from(1_000u64) * U256::from(10u64).pow(U256::from(18));
        let p = price_from_reserves(reserve_token, U256::from(10u64).pow(U256::from(18)), 18, 18);
        assert!((p - 0.001).abs() < 1e-9, "got {p}");
    }

    #[test]
    fn price_from_reserves_handles_mixed_decimals() {
        // 1000 USDC (6dp) against 1 WETH (18dp) => 0.001 WETH per USDC.
        let reserve_token = U256::from(1_000u64) * U256::from(10u64).pow(U256::from(6));
        let p = price_from_reserves(reserve_token, U256::from(10u64).pow(U256::from(18)), 6, 18);
        assert!((p - 0.001).abs() < 1e-9, "got {p}");
    }

    #[test]
    fn price_from_reserves_returns_zero_for_empty_pool() {
        assert_eq!(
            price_from_reserves(U256::ZERO, U256::from(1u64), 18, 18),
            0.0
        );
        assert_eq!(
            price_from_reserves(U256::from(1u64), U256::ZERO, 18, 18),
            0.0
        );
    }

    #[test]
    fn multicall3_address_is_canonical() {
        // 0xcA11bde05977b3631167028862bE2a173976CA11 â€” same on every EVM chain.
        assert_eq!(
            hex::encode(MULTICALL3_ADDRESS),
            "cA11bde05977b3631167028862bE2a173976CA11".to_lowercase()
        );
    }

    #[test]
    fn selectors_are_four_bytes() {
        for d in [
            get_reserves_call_data(),
            slot0_call_data(),
            decimals_call_data(),
            token0_call_data(),
            token1_call_data(),
        ] {
            assert_eq!(d.len(), 4);
        }
    }
}
