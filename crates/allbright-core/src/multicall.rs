//! Multicall3 — batch view calls into a single RPC request.
//! Deployed at the same address on all 10 supported chains.
//! This is the primary lever for living comfortably inside free-tier RPC
//! rate limits: one HTTP request carries every pool-state read per chain.

use alloy::primitives::Address;
use alloy::sol;

/// Canonical Multicall3 address — identical on all target chains.
pub const MULTICALL3: &str = "0xcA11bde05977b3631167028862bE2a173976CA11";

sol! {
    #[sol(rpc)]
    interface IMulticall3 {
        struct Call3 {
            address target;
            bool allowFailure;
            bytes callData;
        }
        struct Result {
            bool success;
            bytes returnData;
        }
        function aggregate3(Call3[] calldata calls)
            external payable returns (Result[] memory returnData);
    }
}

sol! {
    #[sol(rpc)]
    interface IPairCalls {
        function getReserves() external view
            returns (uint112 reserve0, uint112 reserve1, uint32 blockTimestampLast);
        function token0() external view returns (address);
        function token1() external view returns (address);
    }
}

sol! {
    #[sol(rpc)]
    interface IV3PoolCalls {
        function slot0() external view returns (
            uint160 sqrtPriceX96,
            int24 tick,
            uint16 observationIndex,
            uint16 observationCardinality,
            uint16 observationCardinalityNext,
            uint8 feeProtocol,
            bool unlocked
        );
        function liquidity() external view returns (uint128);
    }
}

pub fn multicall3_addr() -> Address {
    MULTICALL3.parse().expect("multicall3 addr")
}
