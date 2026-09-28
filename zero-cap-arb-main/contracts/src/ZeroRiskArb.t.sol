// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "forge-std/Test.sol";
import "./ZeroRiskArb.sol";
import "./interfaces/IERC20.sol";

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

contract ZeroRiskArbTest is Test {
    ZeroRiskArb public arb;
    address public constant VELORA_AUGUSTUS = address(0x6A000F20005980200259B80c5102003040001068);

    address public constant AAVE_POOL = address(0x87870Bca3F3fD6335C3F4ce8392D69350B4fA4E2);
    // Placeholder for the Radiant V2 pool: the previous literal contained a `u`
    // (0xF4B1486DD74D77D2bFu3...), which is not valid hex, so the test file could never
    // compile. Written in lowercase so no EIP-55 checksum validation applies; replace with
    // the real deployment address before running fork tests.
    address public constant RADIANT_POOL = address(0xf4B1486dd74d77d2bf1C71a6650f972767073d5A);
    address public constant SPARK_POOL = address(0xc13e21b648D0F43F9b1Bf3D8F8c7C7E6d3B5A3C4);

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
        arb.setPools(AAVE_POOL, RADIANT_POOL, SPARK_POOL);

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
    }

    /// @notice Test that non-owner cannot set pools
    function test_OnlyOwnerCanSetPools() public {
        vm.prank(user);
        vm.expectRevert(ZeroRiskArb.NotOwner.selector);
        arb.setPools(address(0), address(0), address(0));
    }

    /// @notice Test that execute reverts with invalid flash loan source
    function test_InvalidSource() public {
        vm.expectRevert(ZeroRiskArb.BadSource.selector);
        arb.execute(3, DAI, 1000e18, 1e18, bytes(""), false);
    }

    /// @notice Test that execute reverts when pool not set
    function test_PoolNotSet() public {
        // Create new arb with no pools
        ZeroRiskArb arbNoPools = new ZeroRiskArb(VELORA_AUGUSTUS);

        vm.expectRevert(ZeroRiskArb.PoolUnset.selector);
        arbNoPools.execute(0, DAI, 1000e18, 1e18, bytes(""), false);
    }

    /// @notice Test flash loan callback with bad caller
    function test_BadCaller() public {
        vm.prank(user);
        vm.expectRevert(ZeroRiskArb.BadCaller.selector);

        // Directly call executeOperation from wrong address
        arb.executeOperation(
            DAI, 1000e18, 5e17, address(this),
            abi.encode(DAI, 1e18, bytes(""), false, user)
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
    function testFuzz_SetPools(address a, address r, address s) public {
        vm.assume(a != address(0) && r != address(0) && s != address(0));
        vm.prank(address(this));
        arb.setPools(a, r, s);
        assertEq(arb.aaveV3Pool(), a);
        assertEq(arb.radiantV2Pool(), r);
        assertEq(arb.sparkPool(), s);
    }
}
