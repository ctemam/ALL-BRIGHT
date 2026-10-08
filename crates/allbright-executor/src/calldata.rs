//! ABI encoding for `AllbrightExecutor.execute` — matches contracts/.

use alloy::primitives::{Address, U256};
use alloy::sol;
use alloy::sol_types::SolCall;

use allbright_core::events::OpportunitySignal;

sol! {
    struct RouteLeg {
        address pool;
        address tokenIn;
        address tokenOut;
        uint8 dex;       // 0 = V2, 1 = V3, 2 = stable
        uint24 fee;      // V3 fee tier (unused for V2)
    }

    function execute(
        RouteLeg[] legs,
        uint256 amountIn,
        uint256 minProfit,
        address flashProvider,
        bytes flashData
    ) external;
}

/// Build the on-chain leg list — fail closed on any unparseable
/// address (a zero address in live calldata is a silent-loss bug).
fn legs(sig: &OpportunitySignal) -> anyhow::Result<Vec<RouteLeg>> {
    sig.route
        .iter()
        .map(|l| {
            Ok(RouteLeg {
                pool: l.pool_address.parse().map_err(|_| {
                    anyhow::anyhow!("bad pool address {}", l.pool_address)
                })?,
                tokenIn: l
                    .token_in
                    .parse()
                    .map_err(|_| anyhow::anyhow!("bad token_in {}", l.token_in))?,
                tokenOut: l
                    .token_out
                    .parse()
                    .map_err(|_| anyhow::anyhow!("bad token_out {}", l.token_out))?,
                dex: match l.dex {
                    allbright_core::events::DexKind::V2 => 0,
                    allbright_core::events::DexKind::V3 => 1,
                    allbright_core::events::DexKind::Stable => {
                        anyhow::bail!("stable pools not supported by executor contract")
                    }
                },
                fee: alloy::primitives::aliases::U24::ZERO,
            })
        })
        .collect()
}

/// Full-param encoder for live submission: raw-unit `amount_in` /
/// `min_profit` and explicit flash provider decided by the caller.
pub fn encode_execute_full(
    sig: &OpportunitySignal,
    amount_in: U256,
    min_profit: U256,
    flash_provider: Address,
    flash_data: alloy::primitives::Bytes,
) -> anyhow::Result<Vec<u8>> {
    Ok(executeCall {
        legs: legs(sig)?,
        amountIn: amount_in,
        minProfit: min_profit,
        flashProvider: flash_provider,
        flashData: flash_data,
    }
    .abi_encode())
}

/// Encode the route as `execute` calldata for dry-run validity checks.
/// NOTE: token symbols → addresses must already be resolved upstream;
/// SwapLeg carries addresses in token_in/token_out when contract-grade.
pub fn encode_execute(sig: &OpportunitySignal, _executor: &str) -> Vec<u8> {
    // Human-unit amount → raw units is venue-specific; signal-level f64
    // scaled to 18 decimals as a placeholder for the contract path.
    encode_execute_full(
        sig,
        U256::from((sig.input_amount * 1e18).max(0.0) as u128),
        U256::from((sig.min_profit_usd * 1e6).max(0.0) as u128), // USDC-scale
        Address::ZERO,
        alloy::primitives::Bytes::new(),
    )
    .unwrap_or_default()
}
