// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/// @title IMorphoBlue
/// @notice Minimal Morpho Blue interface for flash loans (0% fee).
/// @dev Morpho Blue uses the same CREATE2 address on all chains:
///      0xBBBBBbbBBb9cC5e90e3b3Af64bdAF62C37EEFFCb
///      Flash loans have access to the entire Morpho balance (all markets).
///      Fee is always 0.
interface IMorphoBlue {
    function flashLoan(
        address token,
        uint256 assets,
        bytes calldata data
    ) external;
}

/// @title IMorphoFlashLoanCallback
/// @notice Callback interface invoked by Morpho Blue during a flash loan.
interface IMorphoFlashLoanCallback {
    /// @param assets The amount borrowed.
    /// @param data   Arbitrary data forwarded from the flashLoan call.
    function onMorphoFlashLoan(uint256 assets, bytes calldata data) external;
}
