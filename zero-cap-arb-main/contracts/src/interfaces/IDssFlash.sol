// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/// @title IDssFlash
/// @notice MakerDAO DssFlash — ERC-3156 flash mint of DAI (0% fee).
/// @dev Deployed on Ethereum mainnet only at:
///      0x60744434d6339a6B27d73d9Eda62b6F66a0a04FA
///      Can mint up to the governance-set debt ceiling of DAI.
///      Fee (`toll`) is currently set to 0 by Maker governance.
interface IDssFlash {
    function flashLoan(
        address receiver,
        address token,
        uint256 amount,
        bytes calldata data
    ) external returns (bool);

    function maxFlashLoan(address token) external view returns (uint256);
    function flashFee(address token, uint256 amount) external view returns (uint256);
}

/// @title IERC3156FlashBorrower
/// @notice Standard ERC-3156 callback — used by DssFlash and any ERC-3156 lender.
interface IERC3156FlashBorrower {
    function onFlashLoan(
        address initiator,
        address token,
        uint256 amount,
        uint256 fee,
        bytes calldata data
    ) external returns (bytes32);
}
