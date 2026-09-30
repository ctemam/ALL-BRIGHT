# ZeroCap Arbitrage - Project Rules

## Build & Verify Commands

```bash
# Backend
cd backend && cargo check           # Type check (fast)
cd backend && cargo test             # 101 unit tests
cd backend && cargo build --release  # Release binary

# Contracts
cd contracts && forge build          # Compile Solidity
cd contracts && forge test -vv       # 12 tests (incl. 10k fuzz)

# PM2 Production
npx pm2 restart zcab-backend-production
npx pm2 logs zcab-backend-production --lines 100 --nostream
```

## Production Lock

All modules are locked via `PRODUCTION_LOCK.json`. The pre-commit hook at `.git/hooks/pre-commit` enforces this. To edit locked files:

- `COMMANDER_UNLOCK=<module_name>` in commit message
- `COMMANDER_OVERRIDE=ALL` environment variable (emergency)

Module names: `radar_pricing_fabric`, `rpc_pool_catalog`, `execution_aggregation`, `simulation_portfolio`, `automation_rules_treasury`, `control_plane_transport`, `discovery_verification`, `smart_contracts`

## Architecture

- **Backend**: Rust/Axum on port 4001 (production), 3001 (simulation)
- **Frontend**: Next.js 14 on port 4000/3000
- **Contracts**: Solidity 0.8.20 / Foundry
- **PM2**: Process supervisor with auto-restart

## Flash Loan Sources (7 total, 3 at 0%)

| # | Source | Fee | Source ID |
|---|--------|-----|-----------|
| 0 | Aave V3 | 0.05% | 0 |
| 1 | Radiant V2 | 0.03% | 1 |
| 2 | Spark | 0%/0.05% | 2 |
| 3 | Balancer V2 | 0% | 3 |
| 4 | Morpho Blue | 0% | 4 |
| 5 | MakerDAO DssFlash | 0% (DAI) | 5 |
| 6 | Uniswap V3 Flash | pool fee | 6 |

## Key Addresses

- Profit wallet: `0x2eF34d88EC4EBBd5543fFF2784D5AdbC01f14D56`
- Auto-transfer threshold: $100 USD
- Balancer Vault: `0xBA12222222228d8Ba445958a75a0704d566BF2C8`
- Morpho Blue: `0xBBBBBbbBBb9cC5e90e3b3Af64bdAF62C37EEFFCb`
- MakerDAO DssFlash: `0x60744434d6339a6B27d73d9Eda62b6F66a0a04FA`

## CEX Benchmark Policy

Binance prices are informational only. They do NOT lower spread thresholds. Profitability is determined by actual DEX-to-DEX spreads minus on-chain costs (gas + flash loan fee + slippage + routing fee).
