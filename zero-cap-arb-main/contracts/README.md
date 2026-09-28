# Contracts — build notes

## Dependencies

`forge test` needs exactly one dependency: **`forge-std`**, vendored as plain files in
`lib/forge-std` (its nested `.git` was removed, so it is committed with this repo and CI
needs no network access).

`remappings.txt` previously declared four remappings pointing at directories that were
never present in the repository:

```text
@openzeppelin/=lib/openzeppelin-contracts/
@aave/=lib/aave-v3-core/
@radiant/=lib/radiant-v2-core/
@spark/=lib/spark-protocol/
```

None of them is required: `src/ZeroRiskArb.sol` imports only the vendored local interfaces
in `src/interfaces/` (`IERC20`, `IAaveV3Pool`, `IRadiantV2Pool`, `ISparkPool`,
`IVeloraAugustus`), and the test imports only `forge-std/Test.sol` plus the local sources.
The unresolved remappings are what made the only test suite in this project unrunnable
(defect **D-05** in `docs/ARBITRAGE-COMPARISON.md`). If a real dependency on OpenZeppelin,
Aave, Radiant or Spark is added later, install the library into `lib/` and re-add the
matching remapping line (the format is strictly `<key>=<value>` — `remappings.txt` does
**not** accept comments).

## Commands

```bash
forge build          # compile (solc 0.8.20, via-ir, optimizer_runs = 1000000)
forge test           # run the suite (fuzz runs = 10000)
forge test --gas-report
forge fmt --check
```
