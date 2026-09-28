// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/// @title IRadiantV2Pool
/// @notice Minimal Radiant V2 flash loan interface
interface IRadiantV2Pool {
    function flashLoan(
        address receiverAddress,
        address[] calldata assets,
        uint256[] calldata amounts,
        uint256[] calldata modes,
        address onBehalfOf,
        bytes calldata params,
        uint16 referralCode
    ) external;
}
