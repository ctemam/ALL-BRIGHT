// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/// @title IVeloraAugustus
/// @notice Velora Augustus Swapper interface (ex-ParaSwap V5)
/// @dev The Augustus contract receives calldata built by the Velora Market API
///      and executes the multi-step swaps internally.
///      Address: 0x6a000f20005980200259b80c5102003040001068
interface IVeloraAugustus {
    function simpleSwap(bytes calldata data) external payable returns (uint256 receivedAmount);
    function megaSwap(bytes calldata data) external payable returns (uint256 receivedAmount);
    function getTokenTransferProxy() external view returns (address);
}
