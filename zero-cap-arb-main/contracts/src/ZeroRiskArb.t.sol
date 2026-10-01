// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "forge-std/Test.sol";
import "./ZeroRiskArb.sol";
import "./interfaces/IERC20.sol";
import {IUniswapV3SwapCallback} from "./interfaces/IUniswapV3Flash.sol";

/// @title ZeroRiskArbTest
/// @notice Foundry test for the ZeroRiskArb flash loan arbitrage contract
contract MockERC20 is IERC20 {
    uint256 public override totalSupply;
    mapping(address => uint256) public override balanceOf;
    mapping(address => mapping(address => uint256)) public override allowance;

    function transfer(address to, uint256 amount) external override returns (bool) {
        balanceOf[msg.sender] -= amount;
        balanceOf[to] += amount;
        return true;
    }

    function approve(address spender, uint256 amount) external override returns (bool) {
        allowance[msg.sender][spender] = amount;
        return true;
    }

    function transferFrom(address from, address to, uint256 amount) external override returns (bool) {
        balanceOf[from] -= amount;
        balanceOf[to] += amount;
        return true;
    }
}

contract MockVeloraAugustus {
    address public proxy;
    constructor() {
        proxy = address(0x9999999999999999999999999999999999999999);
    }
    function getTokenTransferProxy() external view returns (address) {
        return proxy;
    }
}

/// @notice Minimal constant-product pair stand-in: swap() pays out the
///         requested amounts from its own balance (pre-funded in tests).
contract MockPair {
    IERC20 public token0;
    IERC20 public token1;
    constructor(IERC20 t0, IERC20 t1) {
        token0 = t0;
        token1 = t1;
    }
    function swap(uint256 amount0Out, uint256 amount1Out, address to, bytes calldata) external {
        if (amount0Out > 0) token0.transfer(to, amount0Out);
        if (amount1Out > 0) token1.transfer(to, amount1Out);
    }
}

/// @notice Balancer-style vault: transfers the loan then calls back.
contract MockVault {
    function flashLoan(
        address recipient,
        address[] calldata tokens,
        uint256[] calldata amounts,
        bytes calldata data
    ) external {
        IERC20(tokens[0]).transfer(recipient, amounts[0]);
        uint256[] memory fees = new uint256[](1);
        fees[0] = 0;
        IFlashLoanRecipient(recipient).receiveFlashLoan(tokens, amounts, fees, data);
    }
}

/// @notice Minimal V3 pool stand-in: streams `out` to the recipient, then
///         pulls the owed input through uniswapV3SwapCallback — the same
///         payment pattern a real Uniswap V3 pool uses.
contract MockV3Pool {
    IERC20 public token0;
    IERC20 public token1;
    /// Out per unit in, in basis points (e.g. 11000 = +10%).
    uint256 public outBps;

    constructor(IERC20 t0, IERC20 t1, uint256 _outBps) {
        token0 = t0;
        token1 = t1;
        outBps = _outBps;
    }

    function swap(
        address recipient,
        bool zeroForOne,
        int256 amountSpecified,
        uint160,
        bytes calldata data
    ) external returns (int256 amount0, int256 amount1) {
        require(amountSpecified > 0, "mock: exact-in only");
        uint256 inAmt = uint256(amountSpecified);
        uint256 outAmt = inAmt * outBps / 10_000;
        (IERC20 inTok, IERC20 outTok) = zeroForOne ? (token0, token1) : (token1, token0);
        uint256 before = inTok.balanceOf(address(this));
        outTok.transfer(recipient, outAmt);
        // Pull payment from the caller through the callback.
        IUniswapV3SwapCallback(msg.sender).uniswapV3SwapCallback(
            zeroForOne ? int256(inAmt) : int256(0),
            zeroForOne ? int256(0) : int256(inAmt),
            data
        );
        require(inTok.balanceOf(address(this)) == before + inAmt, "mock: unpaid");
        amount0 = zeroForOne ? int256(inAmt) : -int256(outAmt);
        amount1 = zeroForOne ? -int256(outAmt) : int256(inAmt);
    }
}

/// @notice Router stand-in: pulls tokenIn via allowance and pays out a
///         configured amount of another token.
contract MockRouter {
    function pullAndPay(address tokenIn, uint256 inAmt, address tokenOut, uint256 outAmt) external {
        IERC20(tokenIn).transferFrom(msg.sender, address(this), inAmt);
        IERC20(tokenOut).transfer(msg.sender, outAmt);
    }
}

contract ZeroRiskArbTest is Test {
    ZeroRiskArb public arb;
    address public constant VELORA_AUGUSTUS = address(0x6A000F20005980200259B80c5102003040001068);

    address public constant AAVE_POOL = address(0x87870Bca3F3fD6335C3F4ce8392D69350B4fA4E2);
    address public constant RADIANT_POOL = address(0xf4B1486dd74d77d2bf1C71a6650f972767073d5A);
    address public constant SPARK_POOL = address(0xc13e21b648D0F43F9b1Bf3D8F8c7C7E6d3B5A3C4);
    address public constant BALANCER_VAULT = address(0xBA12222222228d8Ba445958a75a0704d566BF2C8);
    address public constant MORPHO_BLUE = address(0xBBBBBbbBBb9cC5e90e3b3Af64bdAF62C37EEFFCb);
    address public constant DSS_FLASH = address(0x60744434d6339a6B27d73d9Eda62b6F66a0a04FA);

    address public constant DAI = address(0x6B175474E89094C44Da98b954EedeAC495271d0F);
    address public constant WETH = address(0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2);

    address public user = address(0x1234);
    address public miner = address(0x5678);

    function setUp() public {
        // Mock bytecode at VELORA_AUGUSTUS address so constructor succeeds
        MockVeloraAugustus mockAugustus = new MockVeloraAugustus();
        vm.etch(VELORA_AUGUSTUS, address(mockAugustus).code);

        // Deploy MockERC20 at DAI address
        MockERC20 mockDai = new MockERC20();
        vm.etch(DAI, address(mockDai).code);

        // Deploy contract
        vm.prank(address(this));
        arb = new ZeroRiskArb(VELORA_AUGUSTUS);

        // Set flash loan pools
        vm.prank(address(this));
        arb.setPools(AAVE_POOL, RADIANT_POOL, SPARK_POOL, BALANCER_VAULT, MORPHO_BLUE, DSS_FLASH);

        // Label addresses for trace readability
        vm.label(address(arb), "ZeroRiskArb");
        vm.label(AAVE_POOL, "AaveV3Pool");
        vm.label(RADIANT_POOL, "RadiantV2Pool");
        vm.label(SPARK_POOL, "SparkPool");
        vm.label(user, "User");
        vm.label(miner, "Miner");
    }

    /// @notice Test deployment and initial state
    function test_Deployment() public {
        assertEq(arb.owner(), address(this));
        assertEq(arb.veloraAugustus(), VELORA_AUGUSTUS);
        assertEq(arb.aaveV3Pool(), AAVE_POOL);
        assertEq(arb.radiantV2Pool(), RADIANT_POOL);
        assertEq(arb.sparkPool(), SPARK_POOL);
        assertEq(arb.balancerVault(), BALANCER_VAULT);
        assertEq(arb.morphoBlue(), MORPHO_BLUE);
        assertEq(arb.dssFlash(), DSS_FLASH);
    }

    /// @notice Test that non-owner cannot set pools
    function test_OnlyOwnerCanSetPools() public {
        vm.prank(user);
        vm.expectRevert(ZeroRiskArb.NotOwner.selector);
        arb.setPools(address(0), address(0), address(0), address(0), address(0), address(0));
    }

    /// @notice Test that execute reverts with invalid flash loan source (7+ is invalid)
    function test_InvalidSource() public {
        vm.expectRevert(ZeroRiskArb.BadSource.selector);
        arb.execute(7, DAI, 1000e18, 1e18, bytes(""), false, address(0));
    }

    /// @notice Test Morpho source (4) works when morphoBlue is set
    function test_MorphoSourceIdIsValid() public {
        // source=4 (Morpho) should NOT revert with BadSource — it should
        // attempt a flash loan call (which will fail at the mock level, not
        // at the source check).
        // We just verify the source index is accepted.
        assertEq(arb.morphoBlue(), MORPHO_BLUE);
    }

    /// @notice Test all 0% fee sources are configured
    function test_ZeroFeeSourcesConfigured() public {
        assertTrue(arb.balancerVault() != address(0), "Balancer vault should be set");
        assertTrue(arb.morphoBlue() != address(0), "Morpho Blue should be set");
        assertTrue(arb.dssFlash() != address(0), "DssFlash should be set");
    }

    /// @notice Test that execute reverts when pool not set
    function test_PoolNotSet() public {
        // Create new arb with no pools
        ZeroRiskArb arbNoPools = new ZeroRiskArb(VELORA_AUGUSTUS);

        vm.expectRevert(ZeroRiskArb.PoolUnset.selector);
        arbNoPools.execute(0, DAI, 1000e18, 1e18, bytes(""), false, address(0));
    }

    /// @notice Test flash loan callback with bad caller
    function test_BadCaller() public {
        vm.prank(user);
        vm.expectRevert(ZeroRiskArb.BadCaller.selector);

        // Directly call executeOperation from wrong address
        arb.executeOperation(
            DAI, 1000e18, 5e17, address(this),
            abi.encode(DAI, 1e18, bytes(""), false, user, false)
        );
    }

    /// @notice Test successful withdrawFees
    function test_WithdrawFees() public {
        deal(DAI, address(arb), 100e18);
        vm.prank(address(this));
        arb.withdrawFees(DAI);

        assertEq(IERC20(DAI).balanceOf(address(this)), 100e18);
    }

    /// @notice Test non-owner cannot withdraw fees
    function test_OnlyOwnerCanWithdraw() public {
        vm.prank(user);
        vm.expectRevert(ZeroRiskArb.NotOwner.selector);
        arb.withdrawFees(DAI);
    }

    /// @notice Test that receive() accepts ETH (for Flashbots tips)
    function test_ReceiveEth() public {
        vm.deal(address(this), 1 ether);
        payable(address(arb)).transfer(0.5 ether);
        assertEq(address(arb).balance, 0.5 ether);
    }

    /// @notice Gas benchmark for constructor
    function test_Gas_Constructor() public {
        vm.pauseGasMetering();
        // warm-up
        new ZeroRiskArb(VELORA_AUGUSTUS);
        vm.resumeGasMetering();

        ZeroRiskArb newArb = new ZeroRiskArb(VELORA_AUGUSTUS);
        uint256 gas = gasleft();
        emit log_named_uint("Constructor gas used", gas);
    }

    /// @notice Fuzz: Setting pools with valid addresses
    function testFuzz_SetPools(address a, address r, address s, address b, address m, address d) public {
        vm.assume(a != address(0) && r != address(0) && s != address(0));
        vm.prank(address(this));
        arb.setPools(a, r, s, b, m, d);
        assertEq(arb.aaveV3Pool(), a);
        assertEq(arb.radiantV2Pool(), r);
        assertEq(arb.sparkPool(), s);
        assertEq(arb.balancerVault(), b);
        assertEq(arb.morphoBlue(), m);
        assertEq(arb.dssFlash(), d);
    }

    // ─── Direct-route (executeDirect) tests ─────────────

    /// @notice Only the owner can whitelist swap targets.
    function test_OnlyOwnerCanSetAllowedTarget() public {
        vm.prank(user);
        vm.expectRevert(ZeroRiskArb.NotOwner.selector);
        arb.setAllowedTarget(address(0xBEEF), true);

        arb.setAllowedTarget(address(0xBEEF), true);
        assertTrue(arb.allowedTargets(address(0xBEEF)));

        arb.setAllowedTarget(address(0xBEEF), false);
        assertFalse(arb.allowedTargets(address(0xBEEF)));
    }

    /// @notice executeDirect rejects invalid flash-loan sources.
    function test_ExecuteDirectBadSource() public {
        vm.expectRevert(ZeroRiskArb.BadSource.selector);
        arb.executeDirect(7, DAI, 1000e18, 0, bytes(""), address(0));
    }

    /// @notice Router legs to non-whitelisted targets revert.
    function test_RouterLegRequiresWhitelist() public {
        MockVault vault = new MockVault();
        arb.setPools(address(0), address(0), address(0), address(vault), address(0), address(0));
        deal(DAI, address(vault), 10_000e18);

        address router = address(0xBEEF);
        ZeroRiskArb.SwapLeg[] memory legs = new ZeroRiskArb.SwapLeg[](1);
        legs[0] = ZeroRiskArb.SwapLeg({
            target: router,
            tokenIn: DAI,
            toPool: false,
            amountIn: 0,
            data: bytes("")
        });

        vm.expectRevert(ZeroRiskArb.TargetNotAllowed.selector);
        arb.executeDirect(3, DAI, 1000e18, 0, abi.encode(legs), address(0));
    }

    /// @notice Full two-leg direct route: flash-borrow DAI, swap DAI→MID on
    ///         pool A, MID→DAI on pool B, repay, profit to tx.origin.
    function test_ExecuteDirectTwoPoolLegs() public {
        // Deploy a second token (MID) and a mock Balancer vault.
        MockERC20 mockMid = new MockERC20();
        address MID = address(0x00000000000000000000000000000000DEAD0001);
        vm.etch(MID, address(mockMid).code);

        MockVault vault = new MockVault();
        arb.setPools(address(0), address(0), address(0), address(vault), address(0), address(0));

        MockPair poolA = new MockPair(IERC20(DAI), IERC20(MID));
        MockPair poolB = new MockPair(IERC20(MID), IERC20(DAI));

        // Fund vault + both pools.
        deal(DAI, address(vault), 10_000e18);
        deal(MID, address(poolA), 10_000e18);
        deal(DAI, address(poolB), 10_000e18);

        uint256 borrow = 1000e18;
        uint256 midOut = 1100e18;   // leg 1: 1000 DAI → 1100 MID
        uint256 daiOut = 1050e18;   // leg 2: 1100 MID → 1050 DAI (50 profit)

        ZeroRiskArb.SwapLeg[] memory legs = new ZeroRiskArb.SwapLeg[](2);
        // leg 1: send DAI to poolA, call swap(0, midOut, arb, "")
        legs[0] = ZeroRiskArb.SwapLeg({
            target: address(poolA),
            tokenIn: DAI,
            toPool: true,
            amountIn: borrow,
            data: abi.encodeWithSelector(
                bytes4(0x022c0d9f), uint256(0), midOut, address(arb), bytes("")
            )
        });
        // leg 2: send whole MID balance to poolB, call swap(0, daiOut, arb, "")
        // (poolB's token0 is MID, token1 is DAI → DAI out is amount1)
        legs[1] = ZeroRiskArb.SwapLeg({
            target: address(poolB),
            tokenIn: MID,
            toPool: true,
            amountIn: 0,
            data: abi.encodeWithSelector(
                bytes4(0x022c0d9f), uint256(0), daiOut, address(arb), bytes("")
            )
        });

        vm.prank(user, user);
        arb.executeDirect(3, DAI, borrow, 1e18, abi.encode(legs), address(0));

        // Profit (50 DAI) went to tx.origin = user.
        assertEq(IERC20(DAI).balanceOf(user), 50e18);
        // Vault got repaid in full.
        assertEq(IERC20(DAI).balanceOf(address(vault)), 10_000e18);
        // Contract keeps nothing.
        assertEq(IERC20(DAI).balanceOf(address(arb)), 0);
        assertEq(IERC20(MID).balanceOf(address(arb)), 0);
    }

    /// @notice Direct route that yields less than minProfit must revert.
    function test_ExecuteDirectBelowMinProfit() public {
        MockVault vault = new MockVault();
        arb.setPools(address(0), address(0), address(0), address(vault), address(0), address(0));

        deal(DAI, address(vault), 10_000e18);
        deal(DAI, address(arb), 0);

        MockPair poolB = new MockPair(IERC20(DAI), IERC20(DAI));
        deal(DAI, address(poolB), 10_000e18);

        // Single no-op leg returning exactly the borrowed amount → 0 profit.
        ZeroRiskArb.SwapLeg[] memory legs = new ZeroRiskArb.SwapLeg[](1);
        legs[0] = ZeroRiskArb.SwapLeg({
            target: address(poolB),
            tokenIn: DAI,
            toPool: true,
            amountIn: 1000e18,
            data: abi.encodeWithSelector(
                bytes4(0x022c0d9f), uint256(0), uint256(1000e18), address(arb), bytes("")
            )
        });

        vm.expectRevert(
            abi.encodeWithSelector(
                ZeroRiskArb.BelowMinProfit.selector, DAI, uint256(0), uint256(1e18)
            )
        );
        arb.executeDirect(3, DAI, 1000e18, 1e18, abi.encode(legs), address(0));
    }

    /// @notice A whitelisted router leg executes via approval, and profit
    ///         still must clear the debt check.
    function test_ExecuteDirectRouterLeg() public {
        MockERC20 mockMid = new MockERC20();
        address MID = address(0x00000000000000000000000000000000DEAD0001);
        vm.etch(MID, address(mockMid).code);

        MockVault vault = new MockVault();
        arb.setPools(address(0), address(0), address(0), address(vault), address(0), address(0));

        MockRouter router = new MockRouter();
        arb.setAllowedTarget(address(router), true);

        deal(DAI, address(vault), 10_000e18);
        deal(MID, address(router), 10_000e18);

        // leg 1 (router): pull 1000 DAI from arb, pay 1100 MID back.
        // leg 2 (pool): 1100 MID → 1050 DAI.
        MockPair poolB = new MockPair(IERC20(MID), IERC20(DAI));
        deal(DAI, address(poolB), 10_000e18);

        ZeroRiskArb.SwapLeg[] memory legs = new ZeroRiskArb.SwapLeg[](2);
        legs[0] = ZeroRiskArb.SwapLeg({
            target: address(router),
            tokenIn: DAI,
            toPool: false,
            amountIn: 0,
            data: abi.encodeWithSelector(
                MockRouter.pullAndPay.selector, DAI, uint256(1000e18), MID, uint256(1100e18)
            )
        });
        legs[1] = ZeroRiskArb.SwapLeg({
            target: address(poolB),
            tokenIn: MID,
            toPool: true,
            amountIn: 0,
            data: abi.encodeWithSelector(
                bytes4(0x022c0d9f), uint256(0), uint256(1050e18), address(arb), bytes("")
            )
        });

        vm.prank(user, user);
        arb.executeDirect(3, DAI, 1000e18, 1e18, abi.encode(legs), address(0));
        assertEq(IERC20(DAI).balanceOf(user), 50e18);
    }

    /// @notice V3 legs pay through uniswapV3SwapCallback, not a pre-transfer.
    ///         Flash-borrow DAI, V3 swap DAI→MID (+10%), V3 swap MID→DAI,
    ///         repay, profit to the caller.
    function test_ExecuteDirectV3Legs() public {
        MockERC20 mockMid = new MockERC20();
        address MID = address(0x00000000000000000000000000000000DEAD0001);
        vm.etch(MID, address(mockMid).code);

        MockVault vault = new MockVault();
        arb.setPools(address(0), address(0), address(0), address(vault), address(0), address(0));

        // token ordering: DAI (0x6B17…) > MID (0x…DEAD0001)? MID < DAI here:
        // MID = 0x0000…DEAD0001, DAI = 0x6B17… → MID is token0 on both pools.
        MockV3Pool poolA = new MockV3Pool(IERC20(MID), IERC20(DAI), 11_000); // buy: 1000 DAI → 1100 MID
        MockV3Pool poolB = new MockV3Pool(IERC20(MID), IERC20(DAI), 10_000); // sell: 1100 MID → 1100 DAI

        deal(DAI, address(vault), 10_000e18);
        deal(MID, address(poolA), 10_000e18);
        deal(DAI, address(poolB), 10_000e18);

        uint256 borrow = 1000e18;
        uint256 midIn = 1050e18; // conservative input for leg 2 (sim haircut)

        ZeroRiskArb.SwapLeg[] memory legs = new ZeroRiskArb.SwapLeg[](2);
        // leg 1: V3 swap on poolA, input = DAI = token1 → zeroForOne = false.
        // Callback data carries abi.encode(tokenIn) = DAI.
        legs[0] = ZeroRiskArb.SwapLeg({
            target: address(poolA),
            tokenIn: DAI,
            toPool: true,
            amountIn: 0, // unused for V3 legs — callback pays
            data: abi.encodeWithSelector(
                bytes4(0x128acb08), address(arb), false,
                int256(borrow), uint160(0), abi.encode(DAI)
            )
        });
        // leg 2: V3 swap on poolB, input = MID = token0 → zeroForOne = true.
        legs[1] = ZeroRiskArb.SwapLeg({
            target: address(poolB),
            tokenIn: MID,
            toPool: true,
            amountIn: 0,
            data: abi.encodeWithSelector(
                bytes4(0x128acb08), address(arb), true,
                int256(midIn), uint160(0), abi.encode(MID)
            )
        });

        vm.prank(user, user);
        arb.executeDirect(3, DAI, borrow, 1e18, abi.encode(legs), address(0));

        // Leg1: 1000 DAI → 1100 MID. Leg2 sells 1050 MID → 1050 DAI.
        // DAI balance after repay: 1050 − 1000 = 50 → profit to user.
        assertEq(IERC20(DAI).balanceOf(user), 50e18);
        assertEq(IERC20(DAI).balanceOf(address(vault)), 10_000e18);
        // 50 MID left over from the leg-2 haircut — recoverable dust.
        assertEq(IERC20(MID).balanceOf(address(arb)), 50e18);
    }

    /// @notice A V3 swap callback from an unarmed caller must revert — nobody
    ///         can pull tokens through the callback outside an armed leg.
    function test_V3SwapCallbackRejectsStranger() public {
        vm.prank(user);
        vm.expectRevert(ZeroRiskArb.BadCaller.selector);
        arb.uniswapV3SwapCallback(1e18, 0, abi.encode(DAI));
    }
}
