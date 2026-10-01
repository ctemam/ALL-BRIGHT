# ZCA Chain Matrix — 15-Chain Merged Configuration & Expected Yield

Generated 2026-10-01. All venue addresses verified on-chain (`getCode` +
`getPool`/`getPair` returning live pools). All RPC endpoints verified via
`eth_chainId` match + real `eth_call`; wrong-chain impostors excluded.

## Config Parameters

| Chain | id | RPC endpoints | Tokens mapped | Venues | Quote leg | USD rate source | Min spread | Depth floor | Gas est/tx | Execution path |
|---|---|---|---|---|---|---|---|---|---|---|
| Ethereum | 1 | ~30 | 57 | 4 (UniV2/V3, SushiV2, PCS V3) | WETH | live pool + env | 0.20% | $5,000 | ~$8.00 | Velora |
| Arbitrum | 42161 | ~20 | 47 | 5 (UniV2/V3, Camelot, SushiV3, PCS V3) | WETH | live pool + env | 0.10% | $1,500 | ~$0.05 | Velora |
| Optimism | 10 | ~20 | 36 | 4 (UniV2/V3, Velodrome V2, PCS) | WETH | live pool + env | 0.10% | $1,500 | ~$0.05 | Velora |
| Polygon | 137 | ~20 | 35 | 6 (QuickSwap, SushiV2/V3, ApeSwap, UniV3, PCS) | WMATIC | live pool + env | 0.15% | $2,500 | ~$0.02 | Velora |
| BSC | 56 | ~20 | 41 | 7 (PCS V2/V3, UniV3, Biswap, ApeSwap, BakerySwap, MDEX) | WBNB | live pool + env | 0.15% | $2,500 | ~$0.10 | Velora |
| Avalanche | 43114 | ~15 | 27 | 5 (TraderJoe, Pangolin, SushiV2, UniV2/V3) | WAVAX | live pool + env | 0.20% | $2,000 | ~$0.15 | Velora |
| Base | 8453 | ~20 | 34 | 7 (UniV2/V3, Aerodrome, BaseSwap, PCS V3, SushiV3, AlienBase) | WETH | live pool + env | 0.10% | $1,500 | ~$0.05 | Velora |
| Celo | 42220 | ~10 | 10 | 3 (Ubeswap, UniV3, Velodrome Slipstream) | CELO | live pool + env | 0.15% | $2,500 | ~$0.01 | Direct (no Velora) |
| Gnosis | 100 | ~15 | 16 | 5 (SushiV2, Swapr, Honeyswap, UniV3, CowSwap-adj) | xDAI | $1 hardcoded | 0.10% | $1,500 | ~$0.005 | Velora |
| Linea | 59144 | ~15 | 15 | 4 (UniV2/V3, PCS V2/V3) | WETH | live pool + env | 0.10% | $1,500 | ~$0.03 | Direct (no Velora) |
| **Sonic** | **146** | 5 | 15 | 3 (UniV3, Wagmi V3, SpookySwap V3) | wS | live wS/USDC.e pool | 0.10% | $1,500 | ~$0.02 | Velora |
| **Unichain** | **130** | 5 | 5 | 2 (UniV3, UniV2) | WETH | live pool + env | 0.10% | $1,500 | ~$0.03 | Velora |
| **Scroll** | **534352** | 5 | 14 | 3 (UniV3, KyberSwap Elastic, Nuri CL) | WETH | live pool + env | 0.10% | $1,500 | ~$0.05 | Direct (no Velora) |
| **zkSync Era** | **324** | 4 | 16 | 2 (PCS V3, UniV3) | WETH | live pool + env | 0.10% | $1,500 | ~$0.05 | Direct (no Velora) |
| **Mantle** | **5000** | 5 | 13 | 2 (Agni V3, UniV3) | bridged WETH | ETH rate (quote leg) | 0.10% | $1,500 | ~$0.05 | Direct (no Velora) |

Total: **255 RPC endpoints**, ~310 token-chain mappings, 61 venues, 15 chains.

## Pipeline corrections applied this pass

- **Per-chain token resolution**: `comprehensive_scan` previously passed the
  scan list's canonical (mainnet) address to every chain; `resolve_token_symbol`
  now resolves each symbol to that chain's own contract before pool queries.
- **Sonic USD rate bug**: chain 146 was incorrectly grouped with ETH-priced
  chains (~7700× mispricing). It now reads the live wS/USDC.e pool.
- **RPC pool wired into scanning**: `RadarScanner` now selects endpoints via
  `RpcPool` health scoring + per-request concurrency permits instead of blind
  round-robin — rate-limited endpoints cool down instead of absorbing retries.
- **Sorted-pair factory lookups**: V3/Solidly `getPool` keys by sorted
  addresses; fixed so pools for tokens sorting above the wrapped native
  resolve instead of silently missing.
- **Phantom filters**: FX-peg tokens (CEUR/CREAL/EURE) skipped; raw spreads
  >30% rejected as stale-pool; depth floors raised to $1.5k–$5k.
- **Profit transfer**: PM2 carried stale `MANUAL`/`$25` env; process
  recreated — now `AUTO` at `$100` to `0x2eF3…4D56`, sweep every 60s.

## Live-observed spread activity (post-fix sample)

| Chain | Example observed spread | Venues involved |
|---|---|---|
| Sonic | USDC 0.54% net (raw 0.89%) | SpookySwap V3 500↔3000 |
| Celo | WETH_E 2.41% net | Ubeswap V2 ↔ UniV3 [100] |
| Celo | CUSD 2.43% net | Velodrome Slipstream ↔ UniV3 |
| Avalanche | USDT 0.97% net | UniV3 [10000] ↔ SushiSwap V2 |
| Avalanche | JOE 0.64% net | UniV3 ↔ Pangolin |

## Expected daily profit — honest assessment

**Current realized expectation: ~$0/day**, and any higher claim would be
fabrication. Reasons, per stage of the pipeline:

1. **Detection**: real spreads fire every few minutes across chains (healthy).
2. **Depth**: observed executable depth is typically **$0.02–$700** — below the
   $1.5k floor on most candidates; a 1% spread on $50 of depth is $0.50 gross,
   less than fixed costs.
3. **Validation**: Velora round-trip rejects every candidate seen so far
   (quote nets negative after both legs' impact). Direct-path chains need the
   upgraded `ZeroRiskArb` deployed + `ZERO_RISK_ARB_ADDRESS` set per chain.
4. **Execution**: no confirmed on-chain arbitrage transaction yet; gasless
   UserOp path is code-complete but Pimlico keys + contract deployment are
   pending.

Realistic near-term ceiling once ZeroRiskArb is deployed: thin-chain spreads
like Celo's 2.4% on ~$500 depth ≈ **$10–12 per occurrence, a few/day at most
→ order $10–50/day**, dominated by how often deep pools actually diverge.
The binding constraints are pool depth and execution path — not token count,
not RPC capacity.
