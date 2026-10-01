// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/// @title IUniswapV3Pool (flash-only subset)
/// @notice Uniswap V3 pool flash loans — fee is the pool's swap fee.
/// @dev For 0.01% (1 bps) pools the fee is effectively near-zero.
///      Available on all chains where Uniswap V3 is deployed.
///      The caller receives both token0 and token1 amounts; set one to 0
///      for a single-token flash.
interface IUniswapV3Pool {
    function flash(
        address recipient,
        uint256 amount0,
        uint256 amount1,
        bytes calldata data
    ) external;

    function token0() external view returns (address);
    function token1() external view returns (address);
    function fee() external view returns (uint24);

    /// @notice Swap token0 for token1 or vice versa. The pool calls
    ///         `uniswapV3SwapCallback` on msg.sender to collect the input;
    ///         output is sent to `recipient` before the callback returns.
    ///         `amountSpecified` > 0 = exact input, < 0 = exact output.
    function swap(
        address recipient,
        bool zeroForOne,
        int256 amountSpecified,
        uint160 sqrtPriceLimitX96,
        bytes calldata data
    ) external returns (int256 amount0, int256 amount1);
}

/// @title IUniswapV3FlashCallback
/// @notice Callback invoked by a Uniswap V3 pool during flash().
interface IUniswapV3FlashCallback {
    function uniswapV3FlashCallback(
        uint256 fee0,
        uint256 fee1,
        bytes calldata data
    ) external;
}

/// @title IUniswapV3SwapCallback
/// @notice Callback invoked by a Uniswap V3 pool during swap(). The caller
///         must transfer the owed input token to the pool inside this hook.
interface IUniswapV3SwapCallback {
    function uniswapV3SwapCallback(
        int256 amount0Delta,
        int256 amount1Delta,
        bytes calldata data
    ) external;
}
