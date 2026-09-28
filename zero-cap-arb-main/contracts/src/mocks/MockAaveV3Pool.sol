// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {IERC20} from "../interfaces/IERC20.sol";

/// @title MockAaveV3Pool
/// @notice Minimal mock for testing flash loan callbacks
contract MockAaveV3Pool {
    address public immutable asset;
    uint128 public constant FLASHLOAN_PREMIUM_TOTAL = 9; // 0.05% = 5bp

    constructor(address _asset) {
        asset = _asset;
    }

    function flashLoanSimple(
        address receiver,
        address _asset,
        uint256 amount,
        bytes calldata params,
        uint16
    ) external {
        // Mint tokens to receiver (simulating loan)
        uint256 premium = (amount * 5) / 10000; // 0.05%
        deal(_asset, receiver, amount);

        // Callback
        (bool success, ) = receiver.call(
            abi.encodeWithSelector(
                bytes4(keccak256("executeOperation(address,uint256,uint256,address,bytes)")),
                _asset, amount, premium, address(this), params
            )
        );
        require(success, "callback failed");

        // Verify repayment
        require(IERC20(_asset).balanceOf(receiver) >= amount + premium, "not repaid");
    }

    function deal(address token, address to, uint256 amount) internal {
        // In Foundry, use vm.deal for native, or mint ERC20
        // This mock assumes the test contract handles token minting
    }
}
