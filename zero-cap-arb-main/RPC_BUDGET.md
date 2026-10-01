# RPC Budget — Zero-Cap Arbitrage

How the scanner spends its public-RPC allowance across all 10 chains, and how
to read the numbers below. Last audit: **2026-10-01** (D:\allbright).

## Demand model

Each scan cycle (every `ZCA_SCAN_INTERVAL_SECS`, default **5 s**) queries every
token on every chain through MultiCall3 batched `eth_call`s:

| Phase | Calls per token | Purpose |
|---|---|---|
| Pool resolution | 1 | `getPair` / `getPool` per venue, batched |
| State read | 1 | `getReserves` / `slot0` per pool, batched |
| **Baseline per token** | **2** | |

Per-chain token counts after the 2026-10-01 expansion (each symbol resolves a
real per-chain contract; symbols absent on a chain are skipped):

| Chain | Scanned tokens | Calls/cycle |
|---|---|---|
| Ethereum | ~52 | ~104 |
| BSC | ~37 | ~74 |
| Arbitrum | ~46 | ~92 |
| Base | ~41 | ~82 |
| Optimism | ~33 | ~66 |
| Polygon | ~30 | ~60 |
| Avalanche | ~25 | ~50 |
| Linea | ~13 | ~26 |
| Gnosis | ~15 | ~30 |
| Celo | ~10 | ~20 |
| **Total** | **~302 token-chain mappings** | **~604** |

Across all chains ≈ **604 calls per 5 s cycle** ≈ **121 calls/sec** total.
Thin chains (Celo/Gnosis/Linea) are intentionally below 50 — tokens with no
real liquidity only waste pool-resolution calls; the runtime DEX-Screener
discovery module appends trending tokens dynamically on top.

A failed `eth_call` retries on up to **3 endpoints** (round-robin pick each
try), so a chain whose first-pick endpoint is unhealthy can draw up to
3× baseline (216 calls/chain/cycle).

## Per-chain budget table

| Chain | ID | Catalog EPs | Baseline calls/cycle | Calls/sec | Calls/hour | Calls/day | Verified failures (4-min audit) |
|---|---|---|---|---|---|---|---|
| Ethereum | 1 | **39** | 72 | 14.4 | 51,840 | 1,244,160 | 35 (mixed 403/404/429) |
| Arbitrum | 42161 | **23** | 72 | 14.4 | 51,840 | 1,244,160 | 6 |
| Optimism | 10 | **20** | 72 | 14.4 | 51,840 | 1,244,160 | 14 (429-heavy) |
| Polygon | 137 | **17** | 72 | 14.4 | 51,840 | 1,244,160 | 19 + pool reverts (fixed) |
| BSC | 56 | **44** | 72 | 14.4 | 51,840 | 1,244,160 | 12 |
| Avalanche | 43114 | **19** | 72 | 14.4 | 51,840 | 1,244,160 | 8 |
| Base | 8453 | **24** | 72 | 14.4 | 51,840 | 1,244,160 | 11 |
| Celo | 42220 | **14** | 72 | 14.4 | 51,840 | 1,244,160 | 4 |
| Gnosis | 100 | **16** | 72 | 14.4 | 51,840 | 1,244,160 | 130 (-32602 — **fixed**) |
| Linea | 59144 | **14** | 72 | 14.4 | 51,840 | 1,244,160 | 5 |
| **Total** | | **230** | **720** | **144** | **518,400** | **12.4 M** | **245** |

Plus up to 6 operator endpoints via env (`ETH_RPC_URL`, `ARB_RPC_URL`,
`OP_RPC_URL`, `POLY_RPC_URL`, `BSC_RPC_URL`, `AVAX_RPC_URL`, each with `*_1`–`*_5`
numbered fallbacks) merged ahead of the catalog → **~236 effective endpoints**.

## Capacity vs demand

- Conservative public-endpoint throughput: ~5 req/s sustained → **capacity ≈ 1,150 req/s** across the pool.
- Observed demand: ~121 req/s baseline after token expansion → **~11% utilization**, ~9× headroom.
- Per-endpoint share under pure round-robin: 121 ÷ 230 ≈ **0.53 req/s** — well inside free-tier norms.

## Failure taxonomy (what each error means)

| Error | Meaning | Disposition |
|---|---|---|
| `-32602 invalid params` | Endpoint rejected request **shape** — was caused by a missing `eth_call` block tag (fixed in `multicall.rs`) | Fixed |
| `-32000 eth_call is not supported` | Endpoint is a tx-relay, not a read node (e.g. meowrpc, some MEV relays) | Removed from catalog |
| `403 / 404` | Cloudflare-blocked or retired endpoint | Removed / retry-eligible |
| `429` / `-32001 usage limit` | Free-tier quota exhausted **at that moment** | Kept — rotation rides over it |
| `error sending request` | DNS dead or network timeout | Removed |
| Pool-resolution reverts | Wrong venue factory/signature (Algebra ≠ Uniswap V3) | Fixed — Polygon venue list corrected |

## Endpoint verification method

Every catalog entry passed, at probe time:

1. `eth_chainId` → must return the **exact** target chain id (rejects
   wrong-chain impostors — e.g. `arbitrum-nova.publicnode.com` returns 42170,
   `api.avax-test.network` returns testnet 43113; both dropped).
2. `eth_call` on Multicall3 `getCurrentBlockTimestamp()` → must return a
   32-byte result (rejects read-broken endpoints and tx-only relays).

Endpoint families verified working: publicnode, drpc, thirdweb, pocket.network,
onfinality, blastapi, blockpi, tenderly, sentio, swiftnodes, hostdefi,
nodeflare, blockmachine, fastnode, xrpc.cl, stakely, routeme (rate-limited),
1rpc (rate-limited), dataseed/binance (BSC), official chain RPCs, and public
demo keys (nodereal/dwellir).

## Adding more endpoints

- **Env override**: set `<CHAIN>_RPC_URL` (comma-separated list) plus
  `<CHAIN>_RPC_URL_1..5` fallbacks in `backend/.env` — merged ahead of the
  catalog, deduplicated.
- **Catalog**: add to `backend/src/rpc_catalog.rs` `default_endpoints()` and
  re-verify with an `eth_chainId` + `eth_call` probe before committing.
- **Auth-gated providers** (Alchemy, Infura, QuickNode, Ankr, GetBlock keys):
  supply via env vars — never commit keys to the repo.

## Known cost ceiling & tuning knobs

| Knob | Default | Effect |
|---|---|---|
| `ZCA_SCAN_INTERVAL_SECS` | 5 | 10 s halves all RPC calls (12.4 M → 6.2 M/day) |
| Retry attempts | 3 | Each retry re-picks an endpoint; lowering to 2 cuts worst-case draw by 33% |
| Token set per chain | ~36 | Trimming low-liquidity tokens reduces calls linearly |
| `RPC_MAX_CONCURRENCY` | 16 | Caps parallel in-flight calls |

## Honest limitations

- The public free-endpoint space for these 10 chains yielded **230 verified
  entries** after ~700 URL probes across chainlist.org, ethereum-lists/chains,
  and provider-pattern expansion. Reaching 500+ requires either paid free-tier
  keys (Alchemy/Infura/QuickNode demo keys via env) or an aggregator layer
  (eRPC) presenting remote capacity as a local gateway.
- Rate-limited entries (1rpc, routeme, some dwellir/nodereal demo keys) are
  kept because round-robin retries absorb their intermittent 429s; treat their
  contribution as opportunistic capacity, not guaranteed.
