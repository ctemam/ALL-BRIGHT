// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/// @title IBalancerVault
/// @notice Minimal Balancer V2 Vault interface for flash loans (0% fee)
interface IBalancerVault {
    function flashLoan(
        address recipient,
        address[] calldata tokens,
        uint256[] calldata amounts,
        bytes calldata userData
    ) external;
}

/// @title IFlashLoanRecipient
/// @notice Callback interface for Balancer flash loans
interface IFlashLoanRecipient {
    function receiveFlashLoan(
        address[] calldata tokens,
        uint256[] calldata amounts,
        uint256[] calldata feeAmounts,
        bytes calldata userData
    ) external;
}
