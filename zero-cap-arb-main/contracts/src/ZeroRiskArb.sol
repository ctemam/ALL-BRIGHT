// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {IERC20} from "./interfaces/IERC20.sol";
import {IAaveV3Pool, IFlashLoanSimpleReceiver} from "./interfaces/IAaveV3Pool.sol";
import {IRadiantV2Pool} from "./interfaces/IRadiantV2Pool.sol";
import {ISparkPool} from "./interfaces/ISparkPool.sol";
import {IBalancerVault, IFlashLoanRecipient} from "./interfaces/IBalancerVault.sol";
import {IMorphoBlue, IMorphoFlashLoanCallback} from "./interfaces/IMorphoBlue.sol";
import {IDssFlash, IERC3156FlashBorrower} from "./interfaces/IDssFlash.sol";
import {IUniswapV3Pool, IUniswapV3FlashCallback, IUniswapV3SwapCallback} from "./interfaces/IUniswapV3Flash.sol";
import {IVeloraAugustus} from "./interfaces/IVeloraAugustus.sol";

/// @title ZeroRiskArb
/// @notice Zero-capital arbitrage using flash loans + Velora Market API (ex-ParaSwap)
/// @dev Users pay 0 upfront. Gas settled via ERC-4337 paymasters (Pimlico).
///      If the trade yields < minProfit the entire tx reverts — user loses nothing.
///      Six flash-loan sources, three at 0% fee:
///        0 = Aave V3    (0.05%)    4 = Morpho Blue  (0%)
///        1 = Radiant V2  (varies)  5 = MakerDAO DSS (0%, DAI only)
///        2 = Spark       (0.05%)   6 = Uniswap V3   (pool fee, ~0.01-0.3%)
///        3 = Balancer V2 (0%)
contract ZeroRiskArb is
    IFlashLoanSimpleReceiver,
    IFlashLoanRecipient,
    IMorphoFlashLoanCallback,
    IERC3156FlashBorrower,
    IUniswapV3FlashCallback,
    IUniswapV3SwapCallback
{
    // ─── Constants & Storage ───────────────────────────

    address public immutable owner;
    address public immutable veloraAugustus;
    address public immutable tokenTransferProxy;

    address public aaveV3Pool;
    address public radiantV2Pool;
    address public sparkPool;
    address public balancerVault;
    address public morphoBlue;
    address public dssFlash;

    /// @notice Routers/executors approved to receive token approvals in
    ///         direct-route mode. Pool legs (`toPool`) only receive plain
    ///         transfers — never approvals — so they need no entry.
    mapping(address => bool) public allowedTargets;

    uint8 private _activeSource;

    /// @dev Transient V3-swap guard: armed to the pool address for the
    ///      duration of one direct-route V3 leg, cleared after the call.
    ///      `uniswapV3SwapCallback` only pays while msg.sender matches it,
    ///      so no arbitrary caller can pull tokens through the callback.
    address private _v3CallbackPool;

    /// @notice One swap step of a direct route.
    /// @param target   Router or pool address the leg calls.
    /// @param tokenIn  Token the leg spends.
    /// @param toPool   true → transfer tokenIn to target, then call `data`
    ///                 (e.g. a V2 pair's swap()); false → approve tokenIn to
    ///                 target, then call `data` (router target must be
    ///                 whitelisted first).
    /// @param amountIn Only used for toPool legs: exact amount transferred.
    /// @param data     Calldata sent to `target`.
    struct SwapLeg {
        address target;
        address tokenIn;
        bool    toPool;
        uint256 amountIn;
        bytes   data;
    }

    /// @dev ERC-3156 success return value.
    bytes32 private constant ERC3156_CALLBACK_SUCCESS = keccak256("ERC3156FlashBorrower.onFlashLoan");

    // ─── Events ────────────────────────────────────────

    event ArbitrageExecuted(
        address indexed user,
        address indexed token,
        uint256 profit,
        uint256 fee,
        uint8 source,
        uint256 ts
    );

    event PoolsUpdated(address aave, address radiant, address spark, address balancer, address morpho, address dss);
    event TargetSet(address indexed target, bool allowed);

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
    error TargetNotAllowed();

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

    /// @notice Set all flash loan pool addresses. Pass address(0) for unused sources.
    function setPools(
        address _aave,
        address _radiant,
        address _spark,
        address _balancer,
        address _morpho,
        address _dss
    ) external onlyOwner {
        aaveV3Pool = _aave;
        radiantV2Pool = _radiant;
        sparkPool = _spark;
        balancerVault = _balancer;
        morphoBlue = _morpho;
        dssFlash = _dss;
        emit PoolsUpdated(_aave, _radiant, _spark, _balancer, _morpho, _dss);
    }

    /// @notice Allow/disallow a router/executor target for direct-route legs.
    ///         Only affects legs that receive token approvals; pool legs are
    ///         transfer-only and never read this map.
    function setAllowedTarget(address target, bool allowed) external onlyOwner {
        allowedTargets[target] = allowed;
        emit TargetSet(target, allowed);
    }

    // ─── Entry Point ───────────────────────────────────

    /// @param source 0=AaveV3, 1=RadiantV2, 2=Spark, 3=Balancer(0%), 4=Morpho(0%), 5=MakerDAI(0%), 6=UniV3
    /// @param asset  Token to borrow (e.g. WETH)
    /// @param amount  Exact borrow amount
    /// @param minProfit  Minimum profit; reverts if unmet
    /// @param swapData  Velora Market API calldata (ex-ParaSwap /swap)
    /// @param flashbots  Whether to pay block.coinbase a tip
    /// @param v3Pool  Only for source=6: the Uniswap V3 pool to borrow from
    function execute(
        uint8 source,
        address asset,
        uint256 amount,
        uint256 minProfit,
        bytes calldata swapData,
        bool flashbots,
        address v3Pool
    ) external {
        _run(source, asset, amount, minProfit, swapData, flashbots, v3Pool, false);
    }

    /// @notice Direct-route variant for chains without a Velora deployment.
    ///         `swapData` is `abi.encode(SwapLeg[])`: each leg either calls a
    ///         whitelisted router (approval-based) or a pool directly
    ///         (transfer + precomputed `pair.swap` calldata).
    /// @dev    No flashbots tip in this mode.
    function executeDirect(
        uint8 source,
        address asset,
        uint256 amount,
        uint256 minProfit,
        bytes calldata swapData,
        address v3Pool
    ) external {
        _run(source, asset, amount, minProfit, swapData, false, v3Pool, true);
    }

    function _run(
        uint8 source,
        address asset,
        uint256 amount,
        uint256 minProfit,
        bytes calldata swapData,
        bool flashbots,
        address v3Pool,
        bool direct
    ) internal {
        _activeSource = source;
        // msg.sender — not tx.origin: under ERC-4337 the bundler is tx.origin
        // and our smart account is the direct caller. Routing profit to
        // tx.origin would send it to the bundler.
        bytes memory params = abi.encode(asset, minProfit, swapData, flashbots, msg.sender, direct);

        if (source == 0) {
            if (aaveV3Pool == address(0)) revert PoolUnset();
            IAaveV3Pool(aaveV3Pool).flashLoanSimple(
                address(this), asset, amount, params, 0
            );
        } else if (source == 1) {
            if (radiantV2Pool == address(0)) revert PoolUnset();
            address[] memory a = new address[](1); a[0] = asset;
            uint256[] memory n = new uint256[](1); n[0] = amount;
            uint256[] memory m = new uint256[](1); m[0] = 0;
            IRadiantV2Pool(radiantV2Pool).flashLoan(
                address(this), a, n, m, address(this), params, 0
            );
        } else if (source == 2) {
            if (sparkPool == address(0)) revert PoolUnset();
            ISparkPool(sparkPool).flashLoanSimple(
                address(this), asset, amount, params, 0
            );
        } else if (source == 3) {
            // ── Balancer V2: 0% flash loan fee ──
            if (balancerVault == address(0)) revert PoolUnset();
            address[] memory tokens = new address[](1);
            tokens[0] = asset;
            uint256[] memory amounts = new uint256[](1);
            amounts[0] = amount;
            IBalancerVault(balancerVault).flashLoan(
                address(this), tokens, amounts, params
            );
        } else if (source == 4) {
            // ── Morpho Blue: 0% flash loan fee ──
            if (morphoBlue == address(0)) revert PoolUnset();
            IMorphoBlue(morphoBlue).flashLoan(
                asset, amount, abi.encode(params, asset, amount)
            );
        } else if (source == 5) {
            // ── MakerDAO DssFlash: 0% fee, DAI only ──
            if (dssFlash == address(0)) revert PoolUnset();
            IDssFlash(dssFlash).flashLoan(
                address(this), asset, amount, params
            );
        } else if (source == 6) {
            // ── Uniswap V3 pool flash: fee = pool swap fee ──
            require(v3Pool != address(0), "ZeroRiskArb: v3Pool required");
            address token0 = IUniswapV3Pool(v3Pool).token0();
            (uint256 a0, uint256 a1) = asset == token0
                ? (amount, uint256(0))
                : (uint256(0), amount);
            IUniswapV3Pool(v3Pool).flash(
                address(this), a0, a1,
                abi.encode(params, asset, amount, v3Pool)
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

    // ─── Balancer V2 Callback (0% fee) ─────────────────

    function receiveFlashLoan(
        address[] calldata tokens,
        uint256[] calldata amounts,
        uint256[] calldata feeAmounts,
        bytes calldata userData
    ) external override {
        if (msg.sender != balancerVault) revert BadCaller();
        // Balancer fee is 0, but pass it through for correctness
        _handleFlashLoan(tokens[0], amounts[0], feeAmounts[0], userData);
    }

    // ─── Morpho Blue Callback (0% fee) ────────────────

    function onMorphoFlashLoan(uint256 assets, bytes calldata data) external override {
        if (msg.sender != morphoBlue) revert BadCaller();
        // Decode the outer wrapper: (original params, asset address, amount)
        (bytes memory params, address asset,) = abi.decode(data, (bytes, address, uint256));
        // Morpho requires approve-based repayment (0 fee)
        _approve(asset, morphoBlue);
        _handleFlashLoan(asset, assets, 0, params);
    }

    // ─── MakerDAO DssFlash Callback (0% fee, DAI only) ──

    function onFlashLoan(
        address initiator,
        address token,
        uint256 amount,
        uint256 fee,
        bytes calldata data
    ) external override returns (bytes32) {
        if (msg.sender != dssFlash) revert BadCaller();
        require(initiator == address(this), "ZeroRiskArb: bad initiator");
        // DssFlash requires approve-based repayment
        _approve(token, dssFlash);
        _handleFlashLoan(token, amount, fee, data);
        return ERC3156_CALLBACK_SUCCESS;
    }

    // ─── Uniswap V3 Flash Callback ──────────────────────

    function uniswapV3FlashCallback(
        uint256 fee0,
        uint256 fee1,
        bytes calldata data
    ) external override {
        // Decode: (original params, asset, amount, v3Pool)
        (bytes memory params, address asset, uint256 amount, address v3Pool) =
            abi.decode(data, (bytes, address, uint256, address));
        require(msg.sender == v3Pool, "ZeroRiskArb: bad v3 caller");
        // Determine which fee applies
        address token0 = IUniswapV3Pool(v3Pool).token0();
        uint256 fee = asset == token0 ? fee0 : fee1;
        _handleFlashLoan(asset, amount, fee, params);
    }

    // ─── Uniswap V3 Swap Callback (direct-route legs) ───
    //
    // A V3 pool leg calls pool.swap(...); the pool streams the output to us
    // immediately and collects its input through this hook. The `data` arg
    // we passed to swap() carries abi.encode(tokenIn) so the callback knows
    // which token to send back. Payment is only made to the armed pool.
    function uniswapV3SwapCallback(
        int256 amount0Delta,
        int256 amount1Delta,
        bytes calldata data
    ) external override {
        if (_v3CallbackPool == address(0) || msg.sender != _v3CallbackPool) {
            revert BadCaller();
        }
        address tokenIn = abi.decode(data, (address));
        uint256 owed = amount0Delta > 0 ? uint256(amount0Delta) : uint256(amount1Delta);
        if (owed == 0) revert BadCaller();
        _transfer(tokenIn, msg.sender, owed);
    }

    // ─── Core Logic ────────────────────────────────────

    function _handleFlashLoan(
        address asset,
        uint256 amount,
        uint256 premium,
        bytes memory params
    ) internal {
        (address token, uint256 minProfit, bytes memory swapData, bool flashbots, address user, bool direct) =
            abi.decode(params, (address, uint256, bytes, bool, address, bool));

        // Snapshot the source before any external call: a leg target could
        // re-enter execute() and overwrite _activeSource, which would send
        // the repayment down the wrong branch.
        uint8 src = _activeSource;

        if (direct) {
            SwapLeg[] memory legs = abi.decode(swapData, (SwapLeg[]));
            for (uint256 i = 0; i < legs.length; i++) {
                SwapLeg memory leg = legs[i];
                // V3 pools collect their input inside uniswapV3SwapCallback —
                // pre-transferring tokenIn would double-pay. Detect them by
                // the swap() selector and arm the callback guard instead.
                bool isV3Swap = leg.data.length >= 4
                    && bytes4(leg.data) == bytes4(0x128acb08);
                if (leg.toPool) {
                    if (isV3Swap) {
                        _v3CallbackPool = leg.target;
                    } else {
                        // Pool leg: push tokenIn, then call (e.g. pair.swap()).
                        // No approval is granted — the final balance/debt check
                        // is the only guard needed. amountIn == 0 sends the
                        // contract's whole balance of tokenIn — used for later
                        // legs whose input is the previous leg's output.
                        uint256 amt = leg.amountIn == 0
                            ? IERC20(leg.tokenIn).balanceOf(address(this))
                            : leg.amountIn;
                        _transfer(leg.tokenIn, leg.target, amt);
                    }
                } else {
                    // Router leg: approvals go only to owner-whitelisted
                    // targets.
                    if (!allowedTargets[leg.target]) revert TargetNotAllowed();
                    _approve(leg.tokenIn, leg.target);
                }
                (bool ok, bytes memory ret) = leg.target.call(leg.data);
                if (isV3Swap) _v3CallbackPool = address(0);
                if (!ok) {
                    if (ret.length > 0) assembly { revert(add(32, ret), mload(ret)) }
                    revert SwapFailed();
                }
            }
        } else {
            // Approve ParaSwap to pull tokens
            _approve(token, tokenTransferProxy);

            // Execute the ParaSwap swap
            (bool ok, bytes memory ret) = veloraAugustus.call(swapData);
            if (!ok) {
                if (ret.length > 0) assembly { revert(add(32, ret), mload(ret)) }
                revert SwapFailed();
            }
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

        // Repay flash loan per source.
        // Balancer & UniV3: direct transfer back to the vault/pool.
        // Morpho & DssFlash: approve-based pull (handled in their callbacks).
        // Aave/Radiant/Spark: approve msg.sender to pull.
        if (src == 3) {
            _transfer(token, balancerVault, debt);
        } else if (src == 6) {
            // UniV3 pool pulls via transferFrom after callback
            _transfer(token, msg.sender, debt);
        } else if (src == 4 || src == 5) {
            // Morpho/DssFlash: approval set in their callbacks before this call
        } else {
            _approve(token, msg.sender);
        }

        // Send profit to user
        if (profit > 0) _transfer(token, user, profit);

        emit ArbitrageExecuted(user, token, profit, premium, src, block.timestamp);
    }

    // ─── Helpers ───────────────────────────────────────

    function _checkCaller() internal view {
        uint8 s = _activeSource;
        address c = msg.sender;
        if (
            (s == 0 && c == aaveV3Pool) ||
            (s == 1 && c == radiantV2Pool) ||
            (s == 2 && c == sparkPool) ||
            (s == 3 && c == balancerVault) ||
            (s == 4 && c == morphoBlue) ||
            (s == 5 && c == dssFlash)
            // source 6 (UniV3) is checked in its own callback
        ) return;
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
