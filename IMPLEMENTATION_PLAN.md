# Allbright — Implementation Plan

> Renamed from **Mega3** → **Allbright**. Reflects the ToR's
> "Reverse-Engineering Source & Design Lineage", the `ENV` runtime contract,
> PM2/localhost deployment, **pure Rust binaries**, a **flashloan-funded
> executor contract**, **multi-chain from day one**, and **Pimlico gasless
> account abstraction** for the ops layer.

## Mission profile — "above the top three"

Allbright studies Artemis, Rusty-Sando, and Flashbots MEV-Share as lineage —
then exceeds each on a concrete axis:

| Reference      | Their ceiling                                | Allbright's edge |
|----------------|----------------------------------------------|------------------|
| Artemis        | Framework only; ships no strategies          | Production strategies + contracts + ops layer |
| Rusty-Sando    | Single-strategy, single-chain, fast sim      | Multi-strategy, multi-chain, same sim discipline |
| Flashbots MEV-Share | Ethereum-centric private orderflow      | Per-chain private execution abstraction incl. BSC |

Engineering bar that none of the three ships out of the box:

- **Deterministic replay** — every journal replays into the identical sim
  pipeline (record → re-simulate → same result, bitwise).
- **Differential correctness** — AMM math verified against on-chain quoter
  contracts, not just internal consistency.
- **Funnel observability** — seen → simulated → profitable → submitted →
  included → settled, with per-stage latency histograms.
- **On-chain profit guarantee** — the executor contract enforces
  `minProfit` atomically; a losing trade is impossible by construction.

## Design lineage (from ToR, unchanged)

| Allbright component        | Reference                          | Influence                                  |
|----------------------------|------------------------------------|--------------------------------------------|
| `allbright-ingest`         | Paradigm Artemis                   | Async collectors, typed events             |
| `allbright-ingest/cex`     | Binance market data                | External predictive signal                 |
| `allbright-ingest/dex`     | Velora (ex-ParaSwap) API           | Aggregated DEX route/price signal          |
| `allbright-simulation`     | Rusty-Sando + revm                 | Cached EVM state, ephemeral sim contexts   |
| `allbright-simulation`     | PancakeSwap V2/V3 + generic AMMs   | AMM pricing mathematics                    |
| `allbright-solver`         | Artemis/Rusty-Sando + original     | Path/sizing optimization, divergence       |
| `allbright-executor`       | Flashbots MEV-Share                | Private tx / bundle submission, relays     |
| `contracts/`               | Original (flashloan arb pattern)   | Atomic execution, on-chain profit guard    |
| `allbright-research`       | Original                           | Historical competitor/tip analysis         |
| `allbright-dashboard`      | Artemis telemetry philosophy       | Binary BSON + WebSocket observability      |
| AA layer                   | Pimlico / ZeroDev / ERC-4337       | Gasless non-hot-path ops only              |
| Primitives                 | Alloy                              | EVM types, providers, ABI                  |
| EVM execution              | revm                               | Pinned version, documented                 |

Every crate root carries a `//!` source-lineage doc comment stating its
architectural lineage and that it is an independent implementation.

## Workspace layout

```
allbright/
├── Cargo.toml                  # Rust workspace
├── contracts/                  # Foundry: AllbrightExecutor + flashloan adapters
│   ├── src/AllbrightExecutor.sol
│   ├── src/flashloans/         # AaveV3, DODO, PancakeV3-flash adapters
│   └── test/                   # fork tests per chain
├── crates/
│   ├── allbright-core/         # typed events, config, chain registry, secrets
│   ├── allbright-ingest/       # cex_stream, dex_stream (Velora), evm_stream, recorder
│   ├── allbright-simulation/   # revm CacheDB, AMM math, tx playback
│   ├── allbright-solver/       # graph search, divergence engine, profit model
│   ├── allbright-executor/     # relay adapters, contract calldata, AA ops
│   ├── allbright-research/     # scanner, backtesting over journals
│   └── allbright-dashboard/    # BSON telemetry → WebSocket (PORT=3001)
├── ecosystem.config.js         # PM2 process definitions
├── ENV                         # runtime config (never commit secrets)
└── IMPLEMENTATION_PLAN.md
```

## Configuration contract (from `ENV`)

```text
allbright-core/src/config.rs loads:

Chains      ETH_RPC_URL, ARB_RPC_URL, OP_RPC_URL,
            POLY_RPC_URL, BSC_RPC_URL, AVAX_RPC_URL   (all publicnode free tier)
Signals     VELORA_API_KEY          (aggregator quotes)
Execution   FLASHBOOTS_RELAY, FLASHBOOTS_AUTH_KEY
AA          PIMLICO_API_KEY, ZERODEV_PROJECT_ID
Wallet      PRIVATE_KEY → EOA owner of ERC-4337 smart account
            (0x2eF34d88EC4EBBd5543fFF2784D5AdbC01f14D56)
Economics   NATIVE_USD          (wrapped-native USD price for profit calc)
Profit      PROFIT_TRANSFER_MODE=AUTO, PROFIT_TRANSFER_MIN_USD=100,
            PROFIT_WALLET
Runtime     AUTO_EXECUTE, MIN_PROFIT_USD=5,
            SCAN_INTERVAL_SECS=5, PORT=3001
New (planned):
            ALLBRIGHT_EXECUTOR_{CHAIN}   executor contract address per chain
            ALLBRIGHT_CHAINLIST          e.g. "bsc,arb,eth,op,poly,avax"
            PIMLICO_SPONSORSHIP_POLICY   paymaster policy id (optional)
```

Note: env vars are unprefixed (`AUTO_EXECUTE`, `MIN_PROFIT_USD`, `NATIVE_USD`, `SCAN_INTERVAL_SECS`)
for consistency — a config-layer decision, no functional impact.

## Strategy surface — recommended set

**Primary (v1): atomic intra-chain DEX arbitrage.** Cyclic 2–3 hop routes
across V2/V3-style pools, executed via the flashloan executor contract.
Atomic (all-or-nothing), capital-free, and on-chain `minProfit` makes losses
impossible. This is the highest risk-adjusted strategy for the stack.

**Secondary (v1): CEX-DEX divergence.** Binance `CexTick` is a *predictive*
signal — when CEX moves, on-chain pools lag for seconds. Solver pre-computes
candidate routes before on-chain state confirms; the executor fires the
moment divergence clears `MIN_PROFIT_USD`.

**Tertiary (v1): aggregator-quote divergence.** Velora quotes vs internal
pool math — a free second opinion that catches mispriced pools our math
models poorly.

**Deferred:** backrunning (requires a real pending-tx feed — free public
RPCs don't provide one; revisit with dedicated node infrastructure).
**Excluded:** sandwiching — adversarial to users, flagged/blocked by
several relays and builders, and not needed for the revenue model.
**Explicit non-goal:** cross-chain "atomic" arb — no atomicity exists
across chains; multi-chain means *independent opportunities per chain*.

## Phases

### Phase 0 — Scaffold
- Rust workspace + Foundry project; pin `revm`, `alloy`, `forge-std` versions
- `allbright-core`: config loader for the env contract; `PRIVATE_KEY` into a
  secrets boundary (`secrecy` crate, memory zeroized, never logged)

### Phase 1 — `allbright-core`
- Typed events: `CexTick`, `DexQuote`, `EvmPoolEvent`, `OpportunitySignal`,
  `ExecutionRequest`, `ExecutionResult`, `TelemetryFrame`, `JournalEntry`
- `ChainRegistry`: all six `*_RPC_URL`s → providers; `ALLBRIGHT_CHAINLIST`
  controls which chains are live (all six day one, each independently
  kill-switchable)
- `DexRegistry`: per-chain DEX adapter descriptors (see multi-chain table)
- Shared wire codec: BSON-serialized typed events over localhost WebSocket —
  the ingestion ↔ compute boundary is a **process boundary** under PM2

### Phase 2 — `allbright-ingest` (Artemis lineage)
- `Collector` trait: async, restartable, typed-event emitter
- `cex_stream.rs`: Binance WebSocket → decode → validate → `CexTick`
- `dex_stream.rs`: Velora API polling aligned to `SCAN_INTERVAL_SECS=5`
- `evm_stream.rs`: one collector per chain — newHeads subscription where
  available, else paced polling; pool-state deltas via batched `eth_call`
- `recorder.rs`: append-only journal — every event + every sim result,
  for replay/backtest parity

### Phase 3 — `allbright-simulation` (Rusty-Sando + revm lineage)
- Persistent `CacheDB` **per chain**; ephemeral sim contexts per opportunity
- Generic AMM math layer: `V2Pool` (reserves), `V3Pool` (tick/sqrt-price/
  liquidity/fee tiers/slippage), `StablePool` (Curve-style, phase 2)
- **Contract-aware playback**: simulate the exact `AllbrightExecutor`
  calldata — flash loan → swap legs → repay → profit check — so sim
  output equals on-chain outcome
- Benchmarks vs the **<120 s engineering target** for full-cycle
  simulation (a target, not an achieved Rusty-Sando figure);
  per-opportunity sim goal on warm cache: **<50 ms**

### Phase 4 — `allbright-solver`
- Pool graph per chain; cyclic path search (2–3 hop), optimal input sizing
  (V2: closed-form optimum; V3: iterative with liquidity-depth awareness)
- Divergence engine: CEX signal and Velora quote vs on-chain pool prices
- Net-profit model: gas + flashloan fee + relay/builder tip; gate on
  `MIN_PROFIT_USD` ($5), USD-denominated via `NATIVE_USD`
- Output is `ExecutionRequest` containing the exact contract calldata the
  simulator already validated — executor never recomputes routes

### Phase 5 — `contracts/` — AllbrightExecutor (Foundry)
- `execute(route, minProfit, flashloan)`: atomic swap legs, flashloan
  repayment, `require(profit >= minProfit)` — revert-safe by construction
- Flashloan adapters behind `IFlashProvider`: Aave V3 (all six chains),
  DODO (BSC/ETH), PancakeSwap V3 `flash()` — cheapest source selected per
  opportunity by the solver
- Access control: `onlyOwner` execute; owner = the ERC-4337 smart account
  (AA custody — see Phase 7); `pause()` kill switch; `sweep()` for profits
- No token approvals held at rest beyond flashloan repayment; dust sweepable
- Fork tests per chain: replay a known profitable block and assert landed
  profit matches simulation within a tolerance bound

### Phase 6 — `allbright-executor` (Flashbots lineage)
- `BuilderRelay` trait; adapters: `FLASHBOOTS_RELAY` (Ethereum), BSC
  builder/relay adapters (bloXroute/48Club-class — **each verified
  independently; zero assumption Ethereum semantics carry over**), public
  mempool fallback for L2s where no private relay exists
- Submit the pre-validated contract calldata; honor `AUTO_EXECUTE`
  (simulate-only when false); inclusion monitoring; per-chain revert stats
- Gas strategy per chain: EIP-1559 caps on L2s, fixed-price on BSC/Polygon

### Phase 7 — Account abstraction layer — **Pimlico gasless mode**

**Mode: sponsored (gasless) UserOperations.** Pimlico supplies both roles:

- `pimlico_sendUserOperation` / bundler RPC via `PIMLICO_API_KEY`
- `pimlico_sponsorUserOperation` — verifying paymaster that **pays gas on
  our behalf**: ERC-4337 UserOps execute with zero native-token spend from
  the smart account (Pimlico bills off-chain against the API-key plan;
  optional `PIMLICO_SPONSORSHIP_POLICY` for spend limits/allowlists)

**Account model (ZeroDev):** the ERC-4337 smart account is a Kernel account
owned by the EOA `0x2eF3…4D56` (`PRIVATE_KEY` signs UserOps off-chain —
signing is free; gas is what the paymaster covers).

**What runs gasless on AA (non-hot-path, per ToR):**

| Operation | Mechanism |
|---|---|
| Profit sweep | `PROFIT_TRANSFER_MODE=AUTO`: accumulated profit ≥ `PROFIT_TRANSFER_MIN_USD` ($100) → gasless UserOp calling `sweep()` on AllbrightExecutor → `PROFIT_WALLET` |
| Contract admin | `pause()`/config UserOps — gasless, even on a chain where the EOA holds no native token |
| Smart-account custody | Contract `owner` = smart account, not the raw EOA — key rotation/policies without redeploying |

**Hard boundary (per ToR):** AA never sits in the execution path.
UserOps have bundler-mempool latency (seconds) — unusable for arb hot path.
Any future exception requires measured latency evidence first.

### Phase 8 — `allbright-research`
- `scanner.rs`: landed-vs-simulated accuracy, competitor tip analysis,
  per-chain profitability stats over journals; replay harness feeds the
  identical sim pipeline

### Phase 9 — `allbright-dashboard`
- BSON `TelemetryFrame`s → WebSocket on `PORT=3001`
- **Opportunity funnel** visualization: seen → simulated → profitable →
  submitted → included → settled, per chain, with per-stage latency p50/p99
- Strictly observability — never in the hot path

### Phase 10 — Integration & hardening
- Fork-test each chain end-to-end; then live dry-run
  (`AUTO_EXECUTE=false`) for ≥1 week of observed accuracy before
  enabling execution
- Kill switches: per-chain flag, contract `pause()`, process-level
- Startup config validation: all env vars, contract addresses, RPC health

## Deployment architecture — PM2, localhost ports

Pure Rust binaries, one PM2 app per process. Inter-process typed events
travel as BSON frames over localhost WebSocket — the same codec the
dashboard uses for telemetry.

```text
ecosystem.config.js — one app per binary

PM2 app                Port   Role
---------------------  -----  --------------------------------------------
allbright-ingest       3101   Typed event feed (WS pub: CEX/DEX/EVM events)
allbright-simulation   3102   Simulation request/response endpoint
allbright-solver       3103   Subscribes to 3101, calls 3102, publishes
                              OpportunitySignal
allbright-executor     3104   Receives ExecutionRequest, owns relays + AA
allbright-research     3105   Scanner / backtest API over journals
allbright-dashboard    3001   Telemetry WS + UI (PORT env — the only port
                              in the env contract today)
```

Rules:

- All internal ports bind to `127.0.0.1` only; `PORT=3001` is the single
  reachable surface (defaults to localhost).
- Ports are convention — `PORT_*` env override per service; 3001 fixed.
- `dotenv`-style loading: each process reads `ENV` via `allbright-core`;
  PM2 `env:` blocks carry only per-process overrides.
- PM2 per app: `autorestart: true`, `max_memory_restart` (highest for
  `allbright-simulation` — six chain CacheDBs), `wait_ready`/
  `listen_timeout`, cron-restart optional for journal rotation.
- Soft startup ordering: consumers reconnect with backoff — any process
  restarts independently.
- Contracts deploy once per chain via `forge script` — outside PM2;
  addresses land in `ALLBRIGHT_EXECUTOR_{CHAIN}`.

## Multi-chain DEX registry (day one)

| Chain    | V2-style              | V3-style                  | Flashloan source      | Private relay        |
|----------|-----------------------|---------------------------|-----------------------|----------------------|
| BSC      | PancakeSwap V2, Biswap| PancakeSwap V3, Uniswap V3| DODO, PCS-V3 flash    | BSC builders (verify)|
| Ethereum | Uniswap V2, Sushi     | Uniswap V3                | Aave V3, Balancer     | Flashbots            |
| Arbitrum | Sushi, Camelot(V2-ish)| Uniswap V3, Camelot V3    | Aave V3, Balancer     | Public mempool       |
| Optimism | Velodrome(v2-ish)     | Uniswap V3                | Aave V3               | Public mempool       |
| Polygon  | QuickSwap, Sushi      | Uniswap V3, QuickSwap V3  | Aave V3, Balancer     | Public mempool       |
| Avalanche| TraderJoe V1, Pangolin| Uniswap V3, TraderJoe V2.1*| Aave V3              | Public mempool       |

\* TraderJoe Liquidity Book is a different curve — adapter phase 2.

Every DEX is an adapter over the generic `V2Pool`/`V3Pool` math with
per-DEX fee tables and factory addresses — verified against current
deployments at startup, never hardcoded blindly.

## Free-RPC constraint (publicnode) — design response

Free public endpoints on all six chains: rate limits, ~100–500 ms latency,
**no pending-tx stream**. Fine for development and low-frequency scanning;
a hard ceiling for competitive block-level MEV.

1. **Provider abstraction** — `ChainProvider` trait: retry/backoff,
   per-endpoint quota tracking, failover across the configured URLs.
2. **Aggressive caching** — per-chain `CacheDB`, batched `eth_call`,
   pool-state deltas instead of full refetches.
3. **Paced scanning** — `SCAN_INTERVAL_SECS=5` fits free-tier budgets.
4. **Documented upgrade path** — competitive execution later needs
   dedicated nodes / premium RPC; a config change, not a refactor.

## Latency budget (internal targets)

| Stage | Target |
|---|---|
| Event decode + validate | < 1 ms |
| Pool-state lookup (warm cache) | < 0.1 ms |
| Opportunity simulation (warm) | < 50 ms (full-cycle worst case: ToR's <120 s) |
| Solver evaluation per cycle | < 5 ms |
| Decision → relay submission | < 10 ms (excl. network) |
| End-to-end tick → submit | < 100 ms on dedicated infra; seconds-scale on free RPC |

## Testing rigor

- **Differential tests**: every AMM quote vs on-chain `quoter`/`getAmountsOut`
  on forked state, randomized inputs
- **Property/fuzz**: V3 tick math round-trips, conservation invariants,
  `proptest`/`forge fuzz`
- **Fork E2E**: replay profitable historical blocks per chain → contract
  lands profit within tolerance of sim
- **Replay parity**: journal → re-sim → identical results (CI gate)
- **CI**: fmt, clippy, tests, fork tests (cached RPC), criterion
  benchmark thresholds as merge gates

## Security notes

- `PRIVATE_KEY`/`PIMLICO_API_KEY` in plaintext `ENV` — gitignore it;
  secrets boundary in `allbright-core`; never logged/serialized/telemetried.
- Custody separation: contract `owner` = ERC-4337 smart account; hot EOA
  key only signs UserOps/txs; profits auto-sweep ≥$100 to `PROFIT_WALLET`.
- On-chain `minProfit` guard means a bad off-chain calc cannot lose funds —
  worst case is gas + flashloan fee, and only if the tx lands.
- `FLASHBOOTS_AUTH_KEY`, `VELORA_API_KEY`, `ZERODEV_PROJECT_ID` empty —
  degrade gracefully (skip source / simulate-only), never fail startup.

## Risk register

| Risk | Mitigation |
|---|---|
| Free RPC rate limits/latency | Provider abstraction + caching + paced scans |
| No mempool feed on free RPCs | Atomic contract arb needs no mempool; backrunning deferred |
| Relay semantics differ per chain | Trait-bound adapters, each verified independently |
| L2 public-mempool frontrunning | Atomic contract + `minProfit`; private orderflow where available |
| V3 math correctness | Differential tests vs on-chain quoters |
| Executor contract bugs | Foundry fork tests, `pause()`, minimal surface, audit before real size |
| Flashloan economics | Fee modeled in solver profit calc; revert = no loss |
| Stale pool state → false opportunities | Re-validate at latest block in sim; revert-safe on-chain anyway |
| Latency targets unproven | Benchmark first; publish measured numbers only |
| AA latency unsuitable for hot path | Gasless UserOps confined to ops/profit-transfer per ToR |
| PM2 process-boundary latency | Compact BSON codec; benchmark WS hop; minimal hops in hot path |
| Secrets in plaintext | Gitignore ENV; secrets boundary; custody separation via smart account |

## Diagnostics layer (added post-launch)

- `allbright-core::lineage` — machine-readable source-origin registry:
  every component mapped to its architectural lineage
  (`artemis`/`rusty_sando`/`flashbots`/`allbright`/`environment`);
  `FailureEvent` (component, lineage, class, origin, stage, chain) rides
  the bus as `Event::Failure` and lands in the journal.
- Ingest audit fan-in journals governance events from sibling buses
  (Rejected/Challenge/Observed/Universe/Opportunity/ExecResult/Failure).
- `allbright-observatory` (:3106) — Independent Opportunity Observatory:
  records cross-venue spreads from the raw ingest feed without applying
  strategy thresholds. `UniverseStats` every 30 s.
- `allbright-challenger` (:3107) — Strategy Challenger: applies the full
  cost model (live `eth_gasPrice`, 0.05% flash premium, 10% adverse
  buffer) to every economically-material `Rejected` candidate;
  `challenged` verdicts flag threshold recalibration review.
- Solver emits `Event::Rejected` at every funnel exit with the furthest
  `OppState` reached — "zero opportunity" is now distinguishable from
  "zero qualified opportunity".
- Research aggregates `observed_universe`, per-reason rejections,
  `challenged` count, `rescued_usd`; dashboard surfaces failures,
  observed universe, and last verdict.

## Blueprint gap register (audit 2026-10-10)

Authoritative register: AGENTS.md Appendix B. Five tracked gaps:

| ID | Gap | Economic cost | Closure |
|---|---|---|---|
| GAP-1 | No EVM-level simulation | 13,028 failed execs, 0 inclusions (bait class) | `eth_call` dry-run gate → revm |
| GAP-2 | Shallow pool registry | Challenger max ~$0.03 net — dust ceiling | Depth-aware re-fetch, on-chain verify |
| GAP-3 | Block-cadence ingestion | Seconds-scale state staleness | WSS Swap/Sync log subs |
| GAP-4 | Flashbots submit stub | Fabricated hash risk | Real bundle or fail-closed |
| GAP-5 | UniV3-flash + ERC-3156 unused | Two 0% providers dead | Wire into provider selection |

Build order: **GAP-2 → GAP-1 → GAP-4 → GAP-3 → GAP-5** (coverage → truth
→ honesty → speed → capacity).
