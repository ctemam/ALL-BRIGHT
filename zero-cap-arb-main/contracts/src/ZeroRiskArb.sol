// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {IERC20} from "./interfaces/IERC20.sol";
import {IAaveV3Pool, IFlashLoanSimpleReceiver} from "./interfaces/IAaveV3Pool.sol";
import {IRadiantV2Pool} from "./interfaces/IRadiantV2Pool.sol";
import {ISparkPool} from "./interfaces/ISparkPool.sol";
import {IVeloraAugustus} from "./interfaces/IVeloraAugustus.sol";

/// @title ZeroRiskArb
/// @notice Zero-capital arbitrage using flash loans + Velora Market API (ex-ParaSwap)
/// @dev Users pay 0 upfront. Gas settled via Flashbots tips or ERC-4337 paymasters.
///      If the trade yields < minProfit the entire tx reverts — user loses nothing.
contract ZeroRiskArb is IFlashLoanSimpleReceiver {
    // ─── Constants & Storage ───────────────────────────

    address public immutable owner;
    address public immutable veloraAugustus;
    address public immutable tokenTransferProxy;

    address public aaveV3Pool;
    address public radiantV2Pool;
    address public sparkPool;

    uint8 private _activeSource;

    // ─── Events ────────────────────────────────────────

    event ArbitrageExecuted(
        address indexed user,
        address indexed token,
        uint256 profit,
        uint256 fee,
        uint8 source,
        uint256 ts
    );

    event PoolsUpdated(address aave, address radiant, address spark);

    // ─── Errors ────────────────────────────────────────

    error NotOwner();
    error NoProfit();
    error BelowMinProfit(address token, uint256 balance, uint256 needed);
    error RepayFailed();
    error SwapFailed();
    error BadSource();
    error PoolUnset();
    error ApprovalFailed();
    error BadCaller();

    // ─── Modifiers ─────────────────────────────────────

    modifier onlyOwner() {
        if (msg.sender != owner) revert NotOwner();
        _;
    }

    // ─── Constructor ───────────────────────────────────

    constructor(address _augustus) {
        owner = msg.sender;
        veloraAugustus = _augustus;
        tokenTransferProxy = IVeloraAugustus(_augustus).getTokenTransferProxy();
    }

    // ─── Admin ─────────────────────────────────────────

    function setPools(address _aave, address _radiant, address _spark) external onlyOwner {
        aaveV3Pool = _aave;
        radiantV2Pool = _radiant;
        sparkPool = _spark;
        emit PoolsUpdated(_aave, _radiant, _spark);
    }

    // ─── Entry Point ───────────────────────────────────

    /// @param source 0=AaveV3, 1=RadiantV2, 2=Spark
    /// @param asset  Token to borrow (e.g. DAI)
    /// @param amount  Exact borrow amount
    /// @param minProfit  Minimum profit; reverts if unmet
    /// @param swapData  Velora Market API calldata (ex-ParaSwap /swap)
    /// @param flashbots  Whether to pay block.coinbase a tip
    function execute(
        uint8 source,
        address asset,
        uint256 amount,
        uint256 minProfit,
        bytes calldata swapData,
        bool flashbots
    ) external {
        _activeSource = source;

        if (source == 0) {
            if (aaveV3Pool == address(0)) revert PoolUnset();
            IAaveV3Pool(aaveV3Pool).flashLoanSimple(
                address(this), asset, amount,
                abi.encode(asset, minProfit, swapData, flashbots, tx.origin), 0
            );
        } else if (source == 1) {
            if (radiantV2Pool == address(0)) revert PoolUnset();
            address[] memory a = new address[](1); a[0] = asset;
            uint256[] memory n = new uint256[](1); n[0] = amount;
            uint256[] memory m = new uint256[](1); m[0] = 0;
            IRadiantV2Pool(radiantV2Pool).flashLoan(
                address(this), a, n, m, address(this),
                abi.encode(asset, minProfit, swapData, flashbots, tx.origin), 0
            );
        } else if (source == 2) {
            if (sparkPool == address(0)) revert PoolUnset();
            ISparkPool(sparkPool).flashLoanSimple(
                address(this), asset, amount,
                abi.encode(asset, minProfit, swapData, flashbots, tx.origin), 0
            );
        } else {
            revert BadSource();
        }
    }

    // ─── Aave V3 / Spark Callback ──────────────────────

    function executeOperation(
        address asset,
        uint256 amount,
        uint256 premium,
        address,
        bytes calldata params
    ) external returns (bool) {
        _checkCaller();
        _handleFlashLoan(asset, amount, premium, params);
        return true;
    }

    // ─── Radiant V2 Callback ───────────────────────────

    function executeOperation(
        address[] calldata assets,
        uint256[] calldata amounts,
        uint256[] calldata premiums,
        address,
        bytes calldata params
    ) external returns (bool) {
        _checkCaller();
        _handleFlashLoan(assets[0], amounts[0], premiums[0], params);
        return true;
    }

    // ─── Core Logic ────────────────────────────────────

    function _handleFlashLoan(
        address asset,
        uint256 amount,
        uint256 premium,
        bytes calldata params
    ) internal {
        (address token, uint256 minProfit, bytes memory swapData, bool flashbots, address user) =
            abi.decode(params, (address, uint256, bytes, bool, address));

        // Approve ParaSwap to pull tokens
        _approve(token, tokenTransferProxy);

        // Execute the ParaSwap swap
        (bool ok, bytes memory ret) = veloraAugustus.call(swapData);
        if (!ok) {
            if (ret.length > 0) assembly { revert(add(32, ret), mload(ret)) }
            revert SwapFailed();
        }

        // Settle
        uint256 bal = IERC20(token).balanceOf(address(this));
        uint256 debt = amount + premium;

        if (bal < debt) revert NoProfit();

        uint256 profit = bal - debt;
        if (profit < minProfit) revert BelowMinProfit(token, profit, minProfit);

        // Flashbots miner tip (10 % of profit)
        if (flashbots) {
            uint256 tip = profit / 10;
            if (tip > 0) _transfer(token, block.coinbase, tip);
            bal = IERC20(token).balanceOf(address(this));
            debt = amount + premium;
            profit = bal - debt;
            if (profit < minProfit) revert BelowMinProfit(token, profit, minProfit);
        }

        // Repay flash loan
        _transfer(token, msg.sender, debt);

        // Send profit to user
        if (profit > 0) _transfer(token, user, profit);

        emit ArbitrageExecuted(user, token, profit, premium, _activeSource, block.timestamp);
    }

    // ─── Helpers ───────────────────────────────────────

    function _checkCaller() internal view {
        uint8 s = _activeSource;
        address c = msg.sender;
        if ((s == 0 && c == aaveV3Pool) || (s == 1 && c == radiantV2Pool) || (s == 2 && c == sparkPool)) return;
        revert BadCaller();
    }

    function _approve(address token, address spender) internal {
        (bool s, bytes memory d) = token.call(abi.encodeWithSelector(IERC20.approve.selector, spender, type(uint256).max));
        if (!s || (d.length > 0 && !abi.decode(d, (bool)))) revert ApprovalFailed();
    }

    function _transfer(address token, address to, uint256 val) internal {
        (bool s, bytes memory d) = token.call(abi.encodeWithSelector(IERC20.transfer.selector, to, val));
        if (!s || (d.length > 0 && !abi.decode(d, (bool)))) revert RepayFailed();
    }

    function withdrawFees(address token) external onlyOwner {
        uint256 b = IERC20(token).balanceOf(address(this));
        if (b > 0) _transfer(token, owner, b);
    }

    receive() external payable {}
}
