// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/// @title IAaveV3Pool
/// @notice Minimal Aave V3 Pool interface for flashLoanSimple
interface IAaveV3Pool {
    error CallerNotPool();
    error FlashLoanNotApproved();

    function flashLoanSimple(
        address receiverAddress,
        address asset,
        uint256 amount,
        bytes calldata params,
        uint16 referralCode
    ) external;

    function FLASHLOAN_PREMIUM_TOTAL() external view returns (uint128);
}

/// @title IFlashLoanSimpleReceiver
/// @notice Interface for Aave V3 flash loan simple receiver
interface IFlashLoanSimpleReceiver {
    function executeOperation(
        address asset,
        uint256 amount,
        uint256 premium,
        address initiator,
        bytes calldata params
    ) external returns (bool);
}
