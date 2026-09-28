// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/// @title ISparkPool
/// @notice Spark Protocol uses the same interface as Aave V3
/// @dev Spark is a fork of Aave V3; the flashLoanSimple interface is identical
import {IAaveV3Pool, IFlashLoanSimpleReceiver} from "./IAaveV3Pool.sol";

interface ISparkPool is IAaveV3Pool {}
