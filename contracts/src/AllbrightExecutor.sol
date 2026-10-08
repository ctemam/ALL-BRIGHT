// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

/// AllbrightExecutor — atomic, flashloan-funded arbitrage executor.
///
/// Independent implementation of publicly documented DEX/flashloan
/// interfaces (Uniswap V2/V3 pair semantics, Aave V3 flashLoanSimple,
/// DODO flashLoan). No proprietary code.
///
/// Invariants:
///   - only `owner` (the ERC-4337 smart account) can execute/sweep/admin
///   - `minProfit` is enforced on-chain: the whole tx reverts if the route
///     does not end with >= initial + minProfit of the input token
///   - `paused` blocks execution but never traps funds (sweep always works)

interface IERC20 {
    function transfer(address to, uint256 amount) external returns (bool);
    function transferFrom(address from, address to, uint256 amount) external returns (bool);
    function approve(address spender, uint256 amount) external returns (bool);
    function balanceOf(address account) external view returns (uint256);
}

interface IUniswapV2Pair {
    function token0() external view returns (address);
    function swap(uint256 amount0Out, uint256 amount1Out, address to, bytes calldata data) external;
    function getReserves() external view returns (uint112 r0, uint112 r1, uint32 ts);
}

interface IUniswapV3Pool {
    function token0() external view returns (address);
    function swap(address recipient, bool zeroForOne, int256 amountSpecified, uint160 sqrtPriceLimitX96, bytes calldata data) external returns (int256 amount0, int256 amount1);
}

interface IAaveV3Pool {
    function flashLoanSimple(address receiverAddress, address asset, uint256 amount, bytes calldata params, uint16 referralCode) external;
}

interface IDodoPool {
    function flashLoan(uint256 baseAmount, uint256 quoteAmount, address assetTo, bytes calldata data) external;
}

contract AllbrightExecutor {
    struct RouteLeg {
        address pool;
        address tokenIn;
        address tokenOut;
        uint8 dex;      // 0 = V2, 1 = V3
        uint24 fee;     // V3 fee tier (V2: fee_bps convention handled off-chain)
    }

    enum FlashKind { NONE, AAVE_V3, DODO }

    address public immutable owner;
    bool public paused;

    uint160 internal constant MIN_SQRT = 4295128739 + 1;
    uint160 internal constant MAX_SQRT = 1461446703485210103287273052203988822378723970342 - 1;

    event Executed(address indexed token, uint256 input, uint256 output, uint256 profit);
    event Swept(address indexed token, address indexed to, uint256 amount);
    event Paused(bool state);

    modifier onlyOwner() {
        require(msg.sender == owner, "not owner");
        _;
    }
    modifier whenNotPaused() {
        require(!paused, "paused");
        _;
    }

    /// Production deployment: deployed via a CREATE2 proxy called by the
    /// ERC-4337 smart account — msg.sender would be the proxy, so the
    /// owner is passed explicitly (the smart-account address).
    constructor(address _owner) {
        require(_owner != address(0), "zero owner");
        owner = _owner;
    }

    receive() external payable {}

    function setPaused(bool state) external onlyOwner {
        paused = state;
        emit Paused(state);
    }

    /// Atomic route execution, optionally flashloan-funded.
    /// `flashProvider` = address(0) → use contract-held capital.
    /// `flashData` for DODO: abi.encode(FlashKind, baseToken).
    function execute(
        RouteLeg[] calldata legs,
        uint256 amountIn,
        uint256 minProfit,
        address flashProvider,
        bytes calldata flashData
    ) external onlyOwner whenNotPaused {
        if (flashProvider == address(0)) {
            _runRoute(legs, amountIn, minProfit);
            return;
        }
        FlashKind kind = flashData.length >= 32
            ? abi.decode(flashData, (FlashKind))
            : FlashKind.AAVE_V3;

        if (kind == FlashKind.AAVE_V3) {
            // params = (legs, minProfit)
            IAaveV3Pool(flashProvider).flashLoanSimple(
                address(this),
                legs[0].tokenIn,
                amountIn,
                abi.encode(legs, minProfit),
                0
            );
        } else if (kind == FlashKind.DODO) {
            address base = abi.decode(flashData[32:], (address));
            uint256 baseAmt = base == legs[0].tokenIn ? amountIn : 0;
            uint256 quoteAmt = base == legs[0].tokenIn ? 0 : amountIn;
            IDodoPool(flashProvider).flashLoan(
                baseAmt, quoteAmt, address(this), abi.encode(legs, minProfit, amountIn)
            );
        } else {
            revert("bad flash kind");
        }
    }

    /// Aave V3 callback.
    function executeOperation(
        address asset,
        uint256 amount,
        uint256 premium,
        address initiator,
        bytes calldata params
    ) external returns (bool) {
        require(initiator == address(this), "bad initiator");
        (RouteLeg[] memory legs, uint256 minProfit) =
            abi.decode(params, (RouteLeg[], uint256));
        require(legs[0].tokenIn == asset, "asset mismatch");

        // minProfit + premium: after repaying amount+premium the contract
        // keeps >= balanceBefore + minProfit.
        _runRoute(legs, amount, minProfit + premium);
        IERC20(asset).approve(msg.sender, amount + premium);
        return true;
    }

    /// DODO callback. data = abi.encode(legs, minProfit, borrowedAmount).
    /// DODO flashloans carry no premium; repay exactly the borrow.
    function DPPFlashLoanCall(address, uint256, uint256, bytes calldata data) external {
        (RouteLeg[] memory legs, uint256 minProfit, uint256 borrowed) =
            abi.decode(data, (RouteLeg[], uint256, uint256));
        _runRoute(legs, borrowed, minProfit);
        IERC20(legs[0].tokenIn).transfer(msg.sender, borrowed);
    }

    /// Run swap legs; revert unless final balance of legs[0].tokenIn
    /// covers start + minProfit.
    function _runRoute(RouteLeg[] memory legs, uint256 amountIn, uint256 minProfit) internal {
        require(legs.length > 0, "empty route");
        address inToken = legs[0].tokenIn;
        uint256 start = IERC20(inToken).balanceOf(address(this));
        require(start >= amountIn, "insufficient balance");

        for (uint256 i = 0; i < legs.length; i++) {
            _swap(legs[i]);
        }

        uint256 end_ = IERC20(inToken).balanceOf(address(this));
        require(end_ >= start + minProfit, "minProfit");
        emit Executed(inToken, amountIn, end_ - (start - amountIn), end_ - start);
    }

    function _swap(RouteLeg memory leg) internal {
        uint256 amt = IERC20(leg.tokenIn).balanceOf(address(this));
        require(amt > 0, "no input");

        if (leg.dex == 0) {
            // V2: transfer in, swap out to next hop (or self).
            IERC20(leg.tokenIn).transfer(leg.pool, amt);
            (uint112 r0, uint112 r1,) = IUniswapV2Pair(leg.pool).getReserves();
            bool zeroForOne = leg.tokenIn == IUniswapV2Pair(leg.pool).token0();
            (uint256 rIn, uint256 rOut) = zeroForOne ? (uint256(r0), uint256(r1)) : (uint256(r1), uint256(r0));
            uint256 out = (amt * 9975 * rOut) / (rIn * 10000 + amt * 9975); // 0.25% PCS-style fee
            IUniswapV2Pair(leg.pool).swap(
                zeroForOne ? 0 : out,
                zeroForOne ? out : 0,
                address(this),
                ""
            );
        } else if (leg.dex == 1) {
            // V3 exact-in single-hop: callback pays tokenIn.
            bool zeroForOne = leg.tokenIn == IUniswapV3Pool(leg.pool).token0();
            IUniswapV3Pool(leg.pool).swap(
                address(this),
                zeroForOne,
                int256(amt),
                zeroForOne ? MIN_SQRT : MAX_SQRT,
                abi.encode(leg.tokenIn)
            );
        } else {
            revert("bad dex");
        }
    }

    /// Uniswap V3 swap callback — pays the input token to the pool.
    function uniswapV3SwapCallback(int256 amount0Delta, int256 amount1Delta, bytes calldata data) external {
        require(amount0Delta > 0 || amount1Delta > 0, "nothing owed");
        address tokenIn = abi.decode(data, (address));
        uint256 pay = amount0Delta > 0 ? uint256(amount0Delta) : uint256(amount1Delta);
        IERC20(tokenIn).transfer(msg.sender, pay);
    }

    /// Sweep profits — callable by owner; the AA layer sends this as a
    /// gasless UserOperation (Pimlico-sponsored).
    function sweep(address token, address to) external onlyOwner {
        uint256 bal = IERC20(token).balanceOf(address(this));
        if (bal > 0) {
            IERC20(token).transfer(to, bal);
            emit Swept(token, to, bal);
        }
    }

    function sweepNative(address to) external onlyOwner {
        uint256 bal = address(this).balance;
        if (bal > 0) {
            (bool ok,) = to.call{value: bal}("");
            require(ok, "native sweep");
            emit Swept(address(0), to, bal);
        }
    }
}
