---
name: allbright-engineering
description: Operational engineering playbook for the Allbright multi-chain arbitrage platform — how to audit, design, implement, benchmark, test, optimize and qualify each subsystem. Companion to AGENTS.md (the constitutional contract). Use whenever implementing or modifying Allbright components.
---

# ALLBRIGHT — Engineering Playbook

Elite multi-chain arbitrage, flash-liquidity and MEV engineering skill.

## 1. PURPOSE

This skill defines how an agent performs engineering work on Allbright. Operate as a combination of: senior Rust engineer, EVM/multi-chain engineer, MEV searcher architect, quantitative strategy engineer, performance engineer, security engineer, data engineer, and production reliability engineer.

The objective is not to write code. It is to produce **measurably superior executable arbitrage infrastructure**.

## 2. OPERATING PHILOSOPHY

```
UNDERSTAND → MEASURE → MODEL → IMPLEMENT → TEST → BENCHMARK → COMPARE → OPTIMIZE → QUALIFY
```

Never use:

```
GUESS → CODE → CLAIM
```

## 3. FIRST ACTION — REPOSITORY AUDIT

Before modifying anything, inspect: root workspace, `Cargo.toml`, every crate under `crates/`, `contracts/` (Foundry), `tools/`, `pools.json`, `ecosystem.config.js`, `ENV` schema (never print secrets), tests, benchmarks, docs, and running PM2 state (`pm2 ls`, `netstat`, `~/.pm2/logs/`).

Identify: existing strategy engines, duplicate modules, unused packages, stale dependencies, dead code, conflicting implementations, missing interfaces, undocumented assumptions, **orphan processes holding service ports**.

Report first: current architecture, implementation status, missing/broken components, performance baseline, security concerns, duplicates, recommended change. Do not rewrite before this audit.

## 4. ARCHITECTURE MAP

The live pipeline (all boundaries are BSON-over-WS on `127.0.0.1`):

```
CEX (Binance) / on-chain pool state (Multicall3)
      ↓  :3101 ingest bus
NORMALIZATION + PoolUpdate/CexTick + journal
      ↓                         ↘
OPPORTUNITY DETECTION      IOO (:3106 observatory — threshold-free
(solver :3103 — pool graph,      spread universe, never trades)
 cyclic arb, divergence)
      ↓  SimRequest → :3102
SIMULATION (pool cache + core AMM math) → SimResult
      ↓
NET PROFIT GATE (min_profit_usd)
      ├→ OpportunitySignal → ExecRequest → :3104 execution
      └→ RejectedCandidate (every funnel exit, reason-coded)
                    ↓
            CHALLENGER (:3107 — counterfactual economics:
            live gasPrice + flash premium + slippage buffer
            → Challenge verdict)
                    ↓
      ingest audit fan-in journals governance events
                    ↓
OUTCOME → journal → research (:3105) → dashboard (:3001)
```

The audit ring (observatory + challenger) is the anti-blindness layer
per AGENTS.md §30: no component may be the sole authority on its own
opportunity universe. A Rejected candidate that survives the full cost
model is a `challenged` verdict — a threshold-review signal, never an
automatic threshold change.

Any proposed feature must identify where it belongs in this pipeline. Ports come from `PORT_*` env vars (`config.rs`) — never hardcode.

## 5. REVERSE-ENGINEERING METHOD

When studying an external reference, produce four outputs:

```
Reference → Relevant architecture → Applicable Allbright lesson → Safe adaptation
```

Never copy an implementation blindly; verify the API/version you actually depend on.

- **Artemis** → event-driven collectors, searcher separation → `allbright-ingest`, `allbright-core` events, dashboard telemetry.
- **Rusty-Sando / revm** → cached state, rapid sim → `allbright-simulation` pool cache today; revm playback is the roadmap.
- **MEV-Share** → private orderflow, simulation-first → `allbright-executor` relay trait; verify each chain's builders independently.
- **Hindsight** → historical replay → `allbright-research` incremental scanner.
- **Binance** → streaming freshness; a CEX quote is never an assumed executable DEX price.
- **Uniswap/PancakeSwap V2/V3, Solidly** → verify formulas, fee tiers, ticks, ordering, decimals, rounding.

**Competitive/reference research handoff:** every research output must carry citation, confidence, scope, comparison limits and a testable follow-up, filed so the quarterly gap review (AGENTS §27.1) can consume it. Monitoring guidance is agent-driven — never present it as an implemented real-time feed.

## 6. STRATEGY DEVELOPMENT

Every strategy follows: `DISCOVER → FILTER → MODEL → SIMULATE → OPTIMIZE → RANK → EXECUTE → RECORD` and carries a stable `OppKind` + version so outcomes are attributable.

### P01 — Cross-DEX (live)
Address-keyed venue matching (never symbol — `USDC` vs `USDC.e` collisions are real). Compare executable prices across venues, evaluate multiple sizes (ternary-search optimum), include all costs, simulate the route, rank by net profit. Never rank on spot difference alone.

### P02 — Multi-hop cyclic (live)
Pool graph: token node, pool edge. A→B→A and A→B→C→A. Prune before simulation: insufficient depth, spread can't cover fees, excessive hops, stale state.

### P03 — CEX/DEX divergence (partial)
Compare CEX executable price vs DEX executable price including fees, depth, latency, slippage, gas, hedge assumptions. Never treat a displayed CEX price as executable at size.

### P04 — Correlated assets (partial)
Establish the historical relationship first; a large deviation can be a genuine depeg = risk, not opportunity. Solidly stable pools use `x³y+y³x` math only (see §13 below).

### P05 — Reactive backrun (blocked)
Requires per-block/mempool feed. Detect → reconstruct resulting state → residual imbalance → simulate → survival probability → qualify. A large preceding trade is not sufficient evidence of profit.

### P06 — Long-tail (not started)
Extra controls: token contract behavior, taxes, blacklists, pool authenticity, verified factory relationship, manipulation exposure. Long-tail ≠ unrestricted.

### P07 — Concentrated liquidity (partial)
Model current tick, active liquidity, tick transitions, fee tier. Do not approximate CL pools as constant-product for execution approval; in-range approximation is signal-level only.

### P08 — Liquidations (not started)
Per-protocol adapter: verify interface, eligibility, incentives, collateral behavior, repayment mechanics; test complete atomic execution.

### P09–P11 — Predictive engine (not started)
Must produce measurable outputs: persistence probability, expected executable edge, uncertainty, EV. No unexplained scores; record contributing features where practical.

### P12 — Adaptive portfolio (not started)
Priority ≈ `expected_net_profit × P(execute) × confidence ÷ resource_cost`, calibrated empirically on outcome data from P01–P04. Do not optimize for theoretical profit.

## 7. OPPORTUNITY OBJECT

Every opportunity carries enough information to reconstruct the decision: `strategy_id`, version, timestamp, chain, state reference, tokens, pools, route legs (addresses — never symbols), input amount, expected output, gross profit, fees, gas estimate, costs, expected net, simulation result, risk score, confidence, decision, outcome.

Use unambiguous field names and consistent units — distinguish `input_amount` (human units) from raw/wei amounts; the codebase convention is human units in `PoolState`, raw only at the calldata boundary.

## 8. SIMULATION SKILL

Per candidate: load state → apply route → evaluate each leg through `allbright-core::amm` → verify non-zero outputs → measure → net result. Benchmark cold vs warm cache, concurrent candidates, route lengths. The revm transaction-playback backend will add full EVM semantics; until then the contract's `minProfit` guard is the final authority.

## 9. PERFORMANCE ENGINEERING

Never optimize before measuring. For every change: baseline → change → benchmark → compare → regression check. Report median, p95, p99, throughput, allocations, CPU, memory, dataset, concurrency. One impressive microbenchmark is insufficient.

## 10. LATENCY BOUNDARY MODEL

Measure separately:

```
Market-data → normalization → detection → solver → simulation
            → signing → submission → builder → inclusion
```

Never call the system "sub-120 µs" because one internal stage measures 120 µs. Current known numbers: math sim 1–6 µs/route (PRODUCTION-measured); pool state freshness = polling-bound (minutes) — that, not sim speed, is the competitive bottleneck today.

## 11. ECONOMIC ENGINE

```
Gross − DEX fees − flash-loan premium − gas − execution cost
      − slippage − adverse movement − failure cost = Net
```

Then `EV = P(success)·net − P(failure)·failure_cost` where probabilities exist. All calculations in consistent units; document every added term.

## 12. RISK ENGINE

Gate categories:

- **Market** — volatility, liquidity, price impact, divergence stability.
- **Contract** — verified pool/factory, token behavior, protocol compatibility.
- **Execution** — stale state, builder availability, RPC health, timeouts.
- **Economic** — min net profit, max gas, min execution probability.
- **Operational** — wallet reserve, system health, config validity, process/port integrity.

## 13. METADATA & AMM CORRECTNESS

Never guess addresses. Pools flow: GeckoTerminal live list → `verify_pool()` (bytecode, `token0`/`token1`, `getReserves`/`slot0`, `factory()`, token `decimals`/`symbol`, `stable()` probe) → `pools.json` with `factory`+`source` provenance → runtime re-verification in `evm_stream`. Failures → `pools.unverified.json`.

Math selection by `DexKind`: `v2` → constant-product; `v3` → in-range √P/L (raw units internally, decimals at the boundary); `stable` → Solidly `x³y+y³x` Newton solve. A pool tagged with the wrong kind is a **phantom-profit bug** — the OP `USDC/USDC` sAMM incident is the canonical case.

## 14. EXECUTION SKILL

No shortcut around: simulation → profitability → risk → configuration. Transaction construction must be deterministic and inspectable — the calldata encoder takes `SwapLeg` **addresses** (sending symbols produced zero-address calldata; check it if touching this path).

Pre-submission verify: chain ID, nonce, destination, calldata, value, gas policy, approvals, repayment, expected profit, deadline, signing identity. Executor addresses come from `ALLBRIGHT_EXECUTOR_<CHAIN>` env — empty means "not deployable on this chain", never substitute a default.

## 15. BUILDER / RELAY SKILL

Every chain's relay gets an adapter behind the `BuilderRelay` trait: `submit / simulate / status / cancel_if_supported / health`, recording submission timestamp, response, latency, acceptance, inclusion, failure reason. Never claim private execution = guaranteed inclusion.

## 16. TELEMETRY SKILL

Structured `Event`s at every stage: INGEST, SIGNAL, STRATEGY, SIMULATION, SOLVER, RISK, EXECUTION, BUILDER, OUTCOME, SYSTEM. Support real-time monitoring, historical analysis, strategy comparison, latency and failure analysis.

## 17. RESEARCH SKILL

The scanner answers the funnel: **Detected ≠ Simulated ≠ Qualified ≠ Submitted ≠ Included ≠ Successful ≠ Profitable.** Report each stage separately. Journal scans are incremental and offset-tracked — bounded memory is a hard requirement (the full-rescan implementation OOM-looped at 1.3 GB).

## 18. HISTORICAL REPLAY

A journaled opportunity should replay with market data + chain state + strategy version + config to reproduce the original decision — for regression testing, strategy development, and incident investigation.

## 19. DASHBOARD SKILL

Executive metrics first: net profit, qualified opportunities, execution success, inclusion, sim/solver latency, failed opportunities, gas, strategy leaderboard, system health. Drill-down: strategy, pool, route, token, builder, failure reason, latency stage.

## 20. FAILURE ANALYSIS

Machine-readable failure classes, never bare `FAILED` — carried by
`Event::Failure` (`allbright-core::lineage`), which binds each failure to
its component lineage and a diagnosed *origin*
(`artemis`/`rusty_sando`/`flashbots`/`allbright`/`environment`). Lineage
names what a module descends from; origin names where the defect lives —
an integration bug in a derived module is `allbright`, not the lineage.
See AGENTS.md §30 for the diagnostic pipeline.

`STALE_STATE, INSUFFICIENT_LIQUIDITY, LOW_NET_PROFIT, SIMULATION_REVERT, TOKEN_RISK, RPC_TIMEOUT, BUILDER_REJECTED, NOT_INCLUDED, ONCHAIN_REVERT, PROFIT_BELOW_THRESHOLD, CONFIGURATION_ERROR, PORT_CONFLICT, ORPHANED_PROCESS, MATH_ARTIFACT`

## 21. COMPETITIVE ANALYSIS

Compare on: data speed, state freshness, simulation accuracy, route optimization, strategy diversity, execution reliability, builder connectivity, gas economics, research, adaptivity, security. Never compare headline latency alone. Today the honest gap is **state freshness** (free RPC polling) — naming it precisely is worth more than any microbenchmark.

Use the Competitive Benchmark Register (AGENTS.md §27.1): Flashbots, bloXroute BackRunMe, CoW Protocol, UniswapX, 1inch Pathfinder, Titan Builder. The register is a snapshot across distinct platform categories, not a verified top-tier ranking — do not conflate it with the §5 technical-lineage table.

Evidence/review practice for every register claim and competitive finding: citation, confidence, scope, comparison limits, testable follow-up. Cadence: event-triggered updates on material public changes, monthly source review, quarterly gap review mapped onto the five-dimension scorecard. This is agent-driven monitoring guidance — not an implemented real-time feed.

## 22. TOP-THREE QUALIFICATION FRAMEWORK

- **Tier 3 — Advanced:** stable architecture, multiple working strategies, deterministic simulation, reliable telemetry, measured performance. *(Current standing — evidence: live 6-service PM2 stack, 10 chains, verified metadata, µs sims, honest zero-profit measurement.)*
- **Tier 2 — Elite:** broad portfolio, strong execution reliability, validated economics, historical replay, adaptive selection, low failure costs.
- **Tier 1 — Top-Three Candidate:** independently reproducible benchmarks, superior risk-adjusted net performance, competitive coverage + execution, mature portfolio, exceptional reliability and security.

Never self-label Tier 1 on internal targets alone.

## 23. DECISION RULES

Prefer: correctness over speed (unless speed preserves correctness), measured over theoretical, net profit over gross spread, reliability over isolated wins, reusable architecture over duplication, determinism over opacity, verified integration over assumed compatibility, simple production path over unnecessary abstraction.

## 24. CHANGE IMPLEMENTATION TEMPLATE

```
CURRENT STATE / PROBLEM / ROOT CAUSE / PROPOSED DESIGN / AFFECTED MODULES
/ REFERENCE / EXPECTED BENEFIT / RISK / TEST PLAN / BENCHMARK PLAN
/ IMPLEMENTATION / VALIDATION / RESULT
```

## 25. PROHIBITED BEHAVIOR

Never: invent benchmark results, builder support, protocol addresses, or API responses; hard-code unverified production values; claim guaranteed inclusion/profit or universal zero gas loss; expose secrets; bypass simulation; mix research and execution; duplicate modules; silently change security controls; optimize a benchmark while degrading economics; run binaries built from stale source against newer pools.json.

## 26. RELEASE QUALIFICATION

Before a production release: `cargo check`, `cargo test`, `cargo clippy`, release build (`-j1` if memory-constrained — the workspace has OOMed parallel builds), `forge build` + fork tests, integration tests, journal-replay check, benchmarks, security checks, config validation, execution dry-run/shadow mode. Then: kill orphans → `pm2 delete`/`pm2 start ecosystem.config.js` → verify all six ports listening → watch logs for the fresh-binary signature (Binance connect, `pool verified`, `sims dispatched`, `sim result`).

## 27. AUDIT RING ENGINEERING

The observatory and challenger exist to break self-confirmation.
Rules when working on them:

- **Observatory** must never read solver state, thresholds, or strategy
  decisions — it consumes the raw ingest feed only. Its noise floor
  (0.03% spread) is an observation floor, not a profit signal. Keep its
  emission deduped (`EMIT_COOLDOWN_MS`) so the journal stays bounded.
- **Challenger** applies costs the solver doesn't: live `eth_gasPrice`
  (60 s cache per chain), Aave-style flash premium (0.05%), 10% adverse
  buffer. A `challenged` verdict means "threshold needs review" — never
  auto-lower `min_profit_usd` from it.
- Rejected candidates must keep their route + input + gas so the
  counterfactual is computable later — never emit a bare rejection.
- The funnel must remain enumerable: every exit path emits `Rejected`
  with the furthest `OppState`. Adding a new exit without a reason code
  breaks the audit trail — forbidden.
- Coverage KPIs (research): `observed_universe` vs solver `simulated`,
  per-reason `rejections`, `challenged`, `rescued_usd` — the
  blindness/rescue metrics live here, not in strategy code.

## 28. OPPORTUNITY UNIVERSE EXPANSION

Expansion is a continuous engineering discipline, not a one-time fetch. The core question: **what profitable markets, chains, DEXs, pools, tokens, pairs, routes or market states are currently invisible to Allbright?**

### Search-space engineering

The search space is `Chains × DEXs × Pools × Tokens × Pairs × Routes × Trade Sizes × Market States × Strategies`. It can become enormous — the problem is never "search everything," it is "search the economically relevant universe intelligently."

### Three universe layers — never confuse them

```
OBSERVATION   — everything safely monitorable (all 10 chains today)
QUALIFICATION — passes technical/economic/risk prerequisites
EXECUTION     — authorized + technically prepared (BSC today)
```

### Discovery mechanics

- **Pools** — `tools/fetch_pools.py` is overlap-aware: enumerate venues per chain, group candidates by token pair *before* verification, keep pairs with ≥2 instances (the only cross-arb-capable structure), add connector pools for triangular density, verify on-chain. New pools from factory events enter `DISCOVERED → OBSERVED → VALIDATED → SIMULATED → QUALIFIED → EXECUTION_ELIGIBLE`, never straight to live.
- **Tokens** — discovery pairs with risk intelligence: liquidity, volume, age, verification, transfer/tax/rebase behavior, blacklist/pause/upgradeability/ownership, oracle dependencies. Maximize *validated economically useful* coverage, not token count.
- **Pairs** — rank by expected net value + execution probability + persistence − risk − resource cost. Never `highest % spread = highest priority` — a smaller spread can be more profitable; a larger can be worthless.
- **Routes** — graph search over the pool graph; prune by liquidity, gas, depth, fee accumulation, state consistency, latency. If pruning is required, the pruning policy must be measurable and challengeable — never an arbitrary depth cap adopted for compute convenience.
- **DEXs** — per-DEX registry records AMM model, fee structure, discovery mechanism, swap math, simulation requirements, execution method. Do not copy one DEX's math onto another.
- **Chains** — rank by risk-adjusted opportunity density, not popularity.

### Universe blindness tests (every major release)

Chain / DEX / pool / token / pair / route / state / strategy blindness — is something economically relevant completely absent, undiscovered, ignored, or rejected-that-another-strategy-could-capture? The Observatory provides the market-wide baseline; the Challenger extends to universe audit (AGENTS.md §31.6).

### Compute allocation: exploration vs exploitation

P12 may allocate by `density × EV × P(exec) × uniqueness`, but MUST reserve an exploration budget — new pools, tokens, DEXs, routes, low-frequency strategies. 100% exploitation optimizes around yesterday's opportunities; 100% exploration wastes compute. The balance is measured, not assumed.

### Universe KPIs

Track observed vs qualified vs production per layer (chains, DEXs, pools, tokens, pairs, routes), opportunity density per segment, newly discovered opportunities, opportunities outside the strategy universe, missed-opportunity value, universe blindness rate, exploration yield, exploration-to-production conversion.

**Do not manufacture coverage:** never claim "covers 95% of BSC arbitrage" without a defensible denominator. Report *Known Registry / Observed Market / Validated Opportunity* coverage and leave the unknown unknown.

## 29. MULTI-RPC INTELLIGENCE & VISIBILITY SKILL

RPC infrastructure is a dynamic intelligence resource. Its purpose: prevent RPC scarcity, rate limits, provider failure and endpoint concentration from becoming hidden opportunity blindness. **RPC supports the opportunity universe; it never defines it.**

### Endpoint capability profiling

For every endpoint determine — never assume: chain, transport (HTTP/WSS), region, latency p50/p95/p99, req/s + burst, daily/monthly quota, method restrictions, logs/archive/trace/pending/private-tx support, reliability. Maintain an expandable registry with design capacity for 200+ endpoints (a target, not a guarantee of 200 free endpoints). Example already encoded: `Config.wss_urls` exists because `arb1.arbitrum.io` serves HTTP but rejects WS upgrade — capability is per-endpoint.

### Request-to-RPC matching

Match each request on: required capability, chain, method, urgency, latency requirement, quota, health, cost, reliability. The best endpoint gives the best information service under current constraints — not necessarily the fastest.

### Budget scheduler

Budget-aware scheduling across req/s, burst, daily, monthly, method limits, remaining quota — with separate reserved budgets for: hot path, opportunity discovery, state validation, simulation, research, dashboard, health. Dashboard traffic must never exhaust hot-path quota.

### Visibility classification

Every data domain: `VISIBLE / PARTIALLY_VISIBLE / DEGRADED / INVISIBLE / UNKNOWN`. Never translate INVISIBLE into NO_OPPORTUNITY.

### RPC blindness telemetry (first-class events)

`RPC_BLINDNESS, RPC_QUOTA_EXHAUSTION, RPC_RATE_LIMIT, RPC_STALE_STATE, RPC_METHOD_UNAVAILABLE, RPC_PROVIDER_FAILURE, RPC_DATA_INCONSISTENCY, RPC_LATENCY_EXPIRED` — reportable facts, not silent absence.

### Cross-RPC validation

Critical observations may be quorum-checked (2/3, 3/5, weighted, or latency-first single-source) per information class. Never impose multi-provider waiting on latency-critical paths.

### Endpoint lifecycle

`DISCOVER → VERIFY → PROFILE → BENCHMARK → QUOTA-MEASURE → SHADOW → QUALIFY → PRODUCTION → MONITOR → REASSESS`

Agents must actively research current provider documentation — provider limits, endpoints and capabilities change; stale memorized quotas are prohibited.

### RPC visibility KPIs

Registered / healthy / degraded endpoints, available capacity (req/s, daily, reserved), quota utilization, RPC failure rate, RPC-induced blindness, cross-RPC disagreement, average data freshness, visible/partial/invisible market segments.

### Prohibited

Never: depend on one RPC; hard-code unverified quotas; assume uniform method support; equate endpoint count with coverage; equate RPC failure with no opportunity; starve hot-path quota with dashboard traffic; assume public RPCs are production-grade or private ones automatically superior; blindly fan every request to every endpoint; let RPC scarcity silently shrink the opportunity universe.

**The RPC layer is not a window to one node — it is Allbright's distributed sensory system. Never let infrastructure blindness masquerade as market absence.**

## 30. DEPLOYMENT READINESS & CAPACITY CERTIFICATION SKILL

Deployment readiness is a **capacity certification**, not a deployment checklist. The agent must prove the deployed system can deliver designed KPI capacity — not that binaries start.

### Method: capacity–KPI mapping

For every material KPI produce:

```
Design KPI → Required capacity → Actual capacity → Stress test
→ Evidence → Gap → GO/NO-GO decision
```

| KPI | Required infrastructure | Proof required |
|---|---|---|
| Opportunity detection (candidates/sec) | RPC + feeds + solver | measured dispatch rate vs design |
| Simulation (sims/sec, latency) | CPU + state cache | benchmark median/p95/p99 |
| Execution (qualified→submitted) | solver + calldata + relay + contract | end-to-end dry-run/shadow result |
| RPC visibility (req/s sustained) | endpoint pool + quotas | quota-aware load test |
| State freshness (seconds/block lag) | WSS/poll interval + RPC health | measured block-age distribution |
| Auditability (replay fidelity) | journal completeness | replay reproduces a recorded decision |

A KPI is not ready because the code path exists — its upstream infrastructure must demonstrate capacity at design scale.

### Multi-chain certification scope

Certify against `Chains × DEXs × Pools × Tokens × Pairs × Routes × RPCs × Strategies × Market States`, not the current BSC execution slice. Per chain, report separately: RPC pool (diversity, capability matrix — HTTP/WS/logs/archive/trace/pending/private-tx), state freshness, DEX coverage, pool count, simulation support, execution path status, relay capability. A chain may be observation-live and execution-blocked — show both. Chains add/remove via config (`ENV` + `pools.json`), never code changes; per-chain enable/disable and per-chain failure isolation are testable properties.

### Readiness equation

`Readiness = Universe Capacity × Data Visibility × Simulation Capacity × Execution Capacity × Risk Capacity × KPI Observability` — multiplicative. 50 configured chains observing 3 reliably = 3-chain ready. One RPC per chain, stale state, weak route search, or KPIs on incomplete data each zero their factor.

### Commander readiness surface

Report: design capacity, active/qualified chains, DEX venues, observed/qualified pools, tokens, pairs, RPC endpoints + healthy capacity %, visibility state, sim/exec throughput, KPI coverage n/72, design-capacity readiness %, and the verdict `GO / CONDITIONAL / NO-GO`.

### Master Readiness Score (1,000 pts)

Weighted pillars: architecture 80 · multi-chain 100 · DEX/pool/token/pair coverage 100 · RPC & visibility 120 · opportunity intelligence 100 · simulation/economic truth 100 · strategy & adaptation 80 · execution reliability 100 · performance/latency 70 · security/risk/governance 100 · observability & 72-KPI 50.

**Score capacity, not configuration** — every pillar grades the ladder `Designed → Configured → Observed → Validated → Production`. Thresholds: ≥950 GO · ≥900 GO w/ minor gaps · 800–899 CONDITIONAL (explicitly accepted limitations) · 700–799 NO-GO/remediation · <700 NO-GO.

**Non-compensatory gates** (any → NO-GO regardless of score): unverified calldata/`minProfit` bypass; exposed secret; unverified metadata in execution; broken observatory/challenger independence; failed forced-failure risk test; unmeasured claims presented as measured.

**Commander override:** the score recommends; the Commander decides. Override requires explicit reason + expiry + accepted failed gates + journaled record — never silent, never hides the failure.

### Readiness verdicts

Classify each domain `READY / PARTIAL / BLOCKED / UNKNOWN` with evidence links (log lines, benchmarks, tx hashes, test output). UNKNOWN is a valid verdict — never upgrade it to READY without measurement. Self-certification by the component under test is not evidence; readiness claims are challengeable like any other KPI.

### Required evidence per gate

- Services/ports/process health (`pm2 ls`, port listeners)
- Config verification (redacted) — no guessed addresses
- `cargo check`/`test`/`clippy`, `forge test` results
- Live funnel numbers: candidates dispatched, sims completed, rejections by reason, challenges
- Contract bytecode/address/owner verified on-chain
- Journal replay sanity on recent records
- Stress/degradation notes (what breaks first, how it fails)

## 31. SELF-DIAGNOSIS & SELF-HEAL SKILL

Constitutional basis: AGENTS.md §34. Self-heal is a live control loop —
`OBSERVE → DETECT → DIAGNOSE → DECIDE → ACT → VERIFY → LEARN →
REQUALIFY` — never a decorative monitor. **Certify it by breaking
controlled components, never by inspecting its existence.** No readiness
points for: agent exists, health endpoint responds, healing function
compiles, dashboard says healthy, config flag enabled.

### Control loop contract

`FAULT → DETECTION → DIAGNOSIS → SAFE DECISION → REMEDIATION →
VERIFICATION → RECOVERY → READINESS RECHECK` — an incomplete loop leaves
the system degraded. `remediation command completed` ≠ `recovered`;
`HEALED` requires independent post-action verification (healer ≠ sole
verifier where practical).

### Fault-injection capability

The readiness challenge generates safe, reversible, capital-isolated
faults across: RPC, market feeds, chain state, ingestion, simulation,
strategy, execution, telemetry, configuration, service workers. Each
test runs `baseline → fault → detection → diagnosis → remediation →
verification → recovery` and produces the full evidence record
(AGENTS §34.14).

### Diagnosis method

For every fault: what failed → where → which downstream capabilities are
affected → independent evidence → confidence → safest remediation →
blast radius → rollback. "Opportunity count fell" is a symptom, not a
diagnosis — find the data-layer cause before concluding market absence.

### Remediation ordering

Least-disruptive safe action wins:

```
refresh → reconnect → reroute → fail over → isolate
       → restart → reduce workload → rollback → escalate
```

Never restart/disable the whole system when a local recovery suffices.

### Authority levels

H0 observe · H1 recommend · H2 auto-heal low-risk reversible (current
sentinel scope) · H3 controlled autonomous recovery (blast-radius +
rollback required) · H4 human escalation (irreversible, capital/wallet,
contract, security, repeated failure, low confidence).

### Multi-chain scope

Healing operates at the smallest affected scope — `Chain → DEX → Pool
→ Token/Pair → RPC → Strategy → Execution`. One RPC on chain 7 failing
isolates that RPC, not all chains. Prefer `isolate → reroute → degrade
locally → recover` over global shutdown.

### Recovery verification

Infrastructure recovery ≠ capability recovery. After data-layer
remediation re-verify in order: `RPC health → state freshness → pool
visibility → pair visibility → opportunity observation → simulation →
KPI integrity`. Economic recovery means opportunities detected, sims
valid, net-profit math valid, risk gates live, execution paths
available, KPIs trustworthy.

### Sentinel implementation (`allbright-sentinel`, :3108)

- **Observe**: TCP liveness per service port each tick + classified
  `Failure` stream on the ingest bus.
- **Diagnose**: action chosen by `FailureClass`/`Component` — port-dead
  → PM2 restart; transport failure → fallback already exists; metadata
  defect → quarantine already happened upstream. No remediation the
  failure doesn't justify.
- **Remediate**: bounded actions only. Every action emits `Event::Heal`
  {target, action, reason, success, restarts_in_window} — journaled via
  ingest fan-in.
- **Budget**: ≤3 restarts/service/10 min, then escalate as classified
  failure. Crash-looping services surface to the Commander, not churn.
- **Verify**: post-restart port check before `success=true`; failed
  heal → classified `Failure`.

### Self-heal KPI set

Detection rate · diagnosis accuracy · root-cause rate · remediation
success · recovery success · false-heal rate · recurrence rate · MTTD ·
MTTR · rollback success · escalation accuracy · secondary-damage rate ·
recovery verification rate (100%) · KPI-revalidation rate · audit-trail
completeness (100%) · autonomous recovery coverage.

### Requalification

After remediation re-run the failed readiness test: pass →
`RECOVERED → REQUALIFIED`; fail → `RECOVERY_FAILED → ESCALATE`;
repeated failure → `PERSISTENT_FAILURE → NO-GO / COMMANDER_REVIEW`.

### Prohibited in the heal path

Never lower thresholds, relax gates, mutate config/secrets/contracts,
admit quarantined pools, suppress failure records, retry infinitely, or
self-certify recovery. Self-healing is a **non-compensatory readiness
gate** (AGENTS §34.16): a failed self-heal gate forces `NO-GO`
regardless of weighted score; Commander override is explicit, recorded,
and never falsifies the measured score.

### Verification

Killing a service must produce: port-dead detection → `pm2 restart` →
port live → journaled `Heal(success=true)` → readiness re-test. Test
it, don't assert it.

## 32. SYSTEM WIRING INTELLIGENCE SKILL

Constitutional basis: AGENTS.md §35. Mission: automatically discover,
map, verify and continuously monitor every dependency Allbright needs —
preventing "everything is installed but nothing is truly connected."

### Core rule

`Present ≠ Connected ≠ Correctly Wired ≠ Operationally Verified`.
A component earns production credit only at `OPERATIONAL` — the real
business function exercised end-to-end. Existence earns zero.

### Wiring discovery

Build the inventory automatically from architecture + source + manifests
+ config schemas + env vars + registries + endpoints + runtime
telemetry. Never expect Commander to enumerate it. Represent each
dependency as `SOURCE → CONNECTION → TARGET → CAPABILITY →
VERIFICATION`.

### Wiring states

`DISCOVERED CONFIGURED CONNECTED VERIFIED OPERATIONAL DEGRADED BROKEN
MISSING MISWIRED UNVERIFIED BLOCKED`. Unknown stays visible — never
promote `MISSING→HEALTHY` or `UNVERIFIED→OPERATIONAL` silently.

### Dumb/dormant detection

Actively hunt components that: receive no data, emit unconsumed data,
target invalid endpoints, use incompatible providers, lack config, hold
stale data, display lineage-less metrics, run as agents with no
effective function, or sit installed-but-unused. Classify
`DUMB`/`DORMANT CAPACITY` until repaired and verified.

### Per-domain wiring checks

- **Chain** (≤50): `chain → RPC pool → gas → DEX registry →
  factory/router → pool discovery → token/pair registry → sim model →
  strategy compat → exec path → wallet/signer → risk → telemetry →
  KPI`. Identify missing links automatically.
- **RPC**: chain it serves, supported methods, workload fit, quota,
  latency, reliability, freshness, failover role — match requests to
  endpoints dynamically. URL alive ≠ suitable.
- **DEX/pool**: `pool exists → state accessible → liquidity measurable
  → pricing usable → simulatable → economically evaluable`.
- **Flash-loan provider**: `provider → chain → contract → liquidity →
  interface → sim → tx → signer → execution → confirmation`. Config
  presence = zero credit.
- **Wallet**: `address → chain → state → signer → nonce → gas → tx →
  confirmation → accounting`. Displayed address ≠ operational wallet.
- **Dashboard/KPI**: `UI → API → backend → telemetry → source →
  calculation` traceable, else mark `UNVERIFIED`/`DECORATIVE`.
- **Agent**: `input → decision → output → consumer → telemetry →
  recovery`; a running process without this chain is dormant.

### Fault injection & heal integration

Controlled breaks (remove RPC, kill WS, break provider, disconnect
telemetry/agent/adapter) must produce `FAULT → DETECT → DIAGNOSE →
HEAL/FAILOVER → VERIFY → REQUALIFY`. The wiring graph feeds the
self-heal engine: blast radius → safe remediation → endpoint verify →
business-function verify → state update → readiness recheck.

### Wiring KPIs

Completeness, correctness, operational wiring, miswired/missing/
unverified counts, dormant components, provider connectivity, endpoint
success rate, dependency recovery rate, wiring MTTR, self-healed
connections, failed heal attempts, KPI lineage completeness, E2E
function verification.

### Readiness gate

Wiring Integrity is a non-compensatory 100-pt pillar (AGENTS §33.6-7).
Unwired wallet→execution, flash-loan chain, critical RPC, sim or KPI
lineage = `NO-GO` at any total score. After repair: `repair →
self-diagnose → heal-verify → E2E function test → KPI revalidation →
readiness recalculation`. Commander sees synthesized state + drill-down;
override never conceals failed wiring.

## 33. 200+ RPC DISCOVERY & QUALIFICATION SKILL

Constitutional basis: AGENTS.md §36. Mission: continuously discover,
test, qualify and operate a production RPC universe of **>200 QUALIFIED
endpoints** — a funnel, not a list.

### Targets (capacity, not fake numbers)

`DISCOVERED ≥1,000 · CANDIDATES ≥500 · LIVE ≥300 · QUALIFIED >200 ·
PRODUCTION = dynamically selected`. If infrastructure reality falls
short, report the actual count and the deficit — never pad.

### Sourcing priority

1. Official chain RPCs. 2. Established keyless infra (Pocket
`{chain}.api.pocket.network`, BlockMachine, NodeFlare, SolidRPC).
3. Decentralized networks. 4. Public directories (QuickRPC,
ChainIndex/ChainList). 5. Community endpoints. Every source is
re-qualified independently — directory presence proves nothing.

### Qualification algorithm (per endpoint)

`DNS → TLS → HTTP → JSON-RPC → chainId → latest block → block timestamp
→ eth_call → balance → logs → method capability → latency → error rate
→ quota/rate-limit → controlled throughput → state freshness → market
visibility → failover suitability`, then classify `QUALIFIED /
DEGRADED / RATE_LIMITED / STALE / BROKEN / QUARANTINED`. **No static
trust** — ChainList presence, docs mention, DNS or HTTP-200 alone never
mark an endpoint healthy.

### Diversity & redundancy

Track `provider_count, chain_count, region_count,
provider_concentration` — 200 URLs on one provider is not 200
resources. Per critical chain: `RPC_COUNT, PROVIDER_COUNT,
HEALTHY_COUNT, SAFE_RPS, CURRENT_RPS, HEADROOM, VISIBILITY,
FAILOVER_STATUS`; target ≥5 endpoints / ≥3 providers where available.
`SAFE_RPS = VERIFIED_RPS × 0.90` — the 10% reserve is never ordinary
workload.

### Market-visibility test

Endpoint qualification requires actual workload evidence: `RPC → block
→ contract → pool state → token state → pair state → event/log
visibility → opportunity engine`. An endpoint that cannot serve the
workload is not qualified for it.

### Failover & heal integration

Per critical chain, deliberately test `primary failure → detection →
backup selection → request migration → state verification → market
visibility verification`, recording recovery time. RPC healing runs
through the sentinel's existing `FAULT → DIAGNOSIS → DECISION →
REMEDIATION → VERIFICATION → RECOVERY → READINESS RECHECK` loop —
**never an independent RPC healing subsystem**.

### Acceptance test

`>200 QUALIFIED + multi-provider + multi-chain + live ping + method
validation + capacity validation + 90% safe capacity + 10% reserve +
tested failover + market visibility + self-diagnosis + self-healing` —
only then is the RPC subsystem `DEPLOYMENT READY`.

## 34. OPPORTUNITY VISIBILITY RPC SKILL

**Purpose:** repeatable procedure ensuring AllBright's RPC infrastructure does not
create invisible arbitrage opportunities. This skill EXTENDS the existing RPC
Intelligence (§29), System Wiring (§32), Opportunity Universe (§28),
Self-Diagnosis (§31) and Deployment Readiness (§30) skills — it is NOT a
standalone RPC subsystem.

### 34.1 Build the opportunity visibility layer

`RPC → CHAIN STATE → DEX STATE → POOL STATE → PAIR STATE → EVENTS → ROUTES → PRICES → OPPORTUNITY OBSERVATION → SIMULATION` — every stage exposes health and completeness.

### 34.2 RPC capability registry

Per RPC record: `rpc_id, provider, endpoint, chain_id, transport, rpc_type,
read_capability, log_capability, historical_capability, trace_capability,
websocket_capability, submission_capability, block_freshness, state_freshness,
latency, capacity, quota, health, visibility_score, qualification_state`.

### 34.3 Visibility test engine

Per chain verify: chain ID, latest block, block timestamp, finalized/safe block;
contracts (`eth_getCode`, `eth_call`, `eth_getStorageAt`); DEX factory/router
deployment and state; pool existence, token0/token1, reserves/liquidity, fee,
slot/state; events (Swap/Mint/Burn/Sync/PoolCreated or chain equivalent); and
that discovered market data reaches the opportunity engine.

### 34.4 Differential testing

For important chains/markets query ≥3 qualified RPCs for the same pool state;
compare block, reserves, liquidity, token state, events. Material difference
raises `RPC_PROVIDER_DIVERGENCE`.

### 34.5 False-zero guard (mandatory)

```
IF opportunity_count == 0 AND visibility_health < threshold
THEN classify DATA_BLIND and initiate diagnosis
     — NEVER classify NO_MARKET_OPPORTUNITY
```

### 34.6 Visibility-aware opportunity states

`VISIBLE_OPPORTUNITY`, `NO_MARKET_OPPORTUNITY`, `NO_DETECTED_OPPORTUNITY`,
`NO_QUALIFIED_OPPORTUNITY`, `FILTERED_BY_POLICY`, `RPC_BLINDNESS`, `STALE_DATA`,
`INCOMPLETE_DATA`, `SIMULATION_DATA_FAILURE`, `EXECUTION_DATA_FAILURE` — this
distinction is mandatory for KPI integrity.

### 34.7 Self-diagnosis procedure

On visibility deterioration: detect anomaly → market activity → RPC health →
block freshness → state freshness → DEX/pool/pair/event visibility → route
generation → simulation inputs → root cause. The diagnosis identifies
`ROOT_CAUSE, AFFECTED_CHAIN, AFFECTED_RPC, AFFECTED_DEX, AFFECTED_POOL,
AFFECTED_PAIR, FAILED_CAPABILITY, SEVERITY, CONFIDENCE`.

### 34.8 Self-healing actions

Use the existing healing orchestration: reduce low-priority RPC traffic, switch
endpoint/provider, activate backup, reconnect WebSocket, refresh chain/pool/pair
state, re-run event discovery, quarantine endpoint, rebalance RPC allocation.
No uncontrolled retry loops.

### 34.9 Verify healing

After remediation retest `RPC → BLOCK → STATE → DEX → POOL → PAIR → EVENT →
OPPORTUNITY → SIMULATION`. Only then may `HEALING_FAILED` become `HEALED`.

### 34.10 Qualification & capacity

`>200 QUALIFIED` endpoints, each with `LIVE=TRUE, CHAIN/CAPABILITY/VISIBILITY/
CAPACITY=VERIFIED` — directory membership does not count. Per endpoint:
`safe_capacity = verified_capacity × 0.90`, `reserve = × 0.10`; monitor
`current_usage, headroom, reserve`.

**QUALIFIED = V4 market-visible, not V3.** The scout enforces this: every
Qualified/Degraded endpoint gets the market probe (real `getReserves()`/
`slot0()` read against a pools.json sample, all endpoints — bounded by
CONCURRENCY, not top-N). A live endpoint that cannot read pool state is
economically blind → demoted to `Degraded` (fallback tier) with the
blindness recorded in `last_error`. A chain with live endpoints but zero
V4 emits `RpcOpportunityBlind`; zero qualified at all emits
`RpcChainVisibilityFailure` — never silently `NO_MARKET_OPPORTUNITY`.

### 34.11 KPI integrity

Every opportunity KPI retains lineage `KPI → Opportunity Engine → Market
Observation → Pool/Pair State → RPC → Blockchain`. A KPI based on an RPC-blind
market segment must be flagged.

### 34.12 Dashboard requirements

Display RPC visibility score, chain/DEX/pool/pair/opportunity visibility,
RPC-induced blindness, stale state, provider divergence, current capacity, the
90% operating limit, 10% reserve, failover state, self-healing state.

### 34.13 Deployment certification

Evidence required: `>200 RPCs + multi-provider redundancy + live capability tests
+ market visibility tests + 90% capacity certification + 10% reserve +
cross-RPC validation + failure injection + self-diagnosis + self-healing +
post-healing verification` — all passing. Only then may
`RPC / Opportunity Visibility = DEPLOYMENT READY` be asserted.

### 34.14 Non-negotiable rule

Forbidden logic: `RPC works → therefore market visible → therefore no
opportunity means none exists`. Correct logic: `RPC works → required
capabilities verified → market state visible → pool/pair state visible →
opportunity universe observable → opportunity result trusted`. The RPC layer is
part of the arbitrage intelligence system, not merely an Internet connection.

## 35. DASHBOARD IMPLEMENTATION SKILL (MANDATORY)

**Authority:** AGENTS.md §38 is binding. Audit the Master Build before
touching the dashboard; reuse existing KPI/strategy/registry/telemetry
systems; no duplicate engines; no mock data; every metric keeps lineage
`UI → API → Backend → Telemetry → Source → Blockchain`.

### 35.1 Page architecture

`PROFIT METRICS → BLOCKCHAIN STREAM → STRATEGY → DEPLOYMENT →
INFRASTRUCTURE → REPORTS → COMPETITIVE ANALYSIS → WALLET`.
Profit Metrics is Page 1 (default landing view). Collapsible sidebar (icon-only + tooltip + active
indicator collapsed; logo top-left; Kill Switch bottom). Header: Profit
Pulse (realized vs wallet-balance vs unrealized strictly distinguished),
currency selector, period indicator, refresh selector, system status
(HEALTHY/DEGRADED/SAFE-HALT), day/night.

### 35.2 Profit page implementation

Dedicated control bar above the visualization with FOUR synchronized
controls:

1. **Time range** — 1m–12m ladder + Today/All Time/Since Deployment/
  Custom. The range is a *backend query parameter*: refetch/re-aggregate,
  never client-side row hiding.
2. **Chain** — ALL CHAINS + every enabled chain from the core chain
  registry (`Chain::ALL`); never hard-code chains.
3. **Strategy Variant** — ALL + the 12 Master Build variants read from
  the strategy registry (P01–P12 in AGENTS §7); no invented names.
4. **Presentation** — TABLE | CHART | BAR, one at a time.

**Single-dataset rule (non-negotiable):** table, chart and bar are three
presentations of ONE P&L query result (time×chain×variant context). Chart
= cumulative/bucketed net-profit time series; bar = P&L by bucket or
variant. Any control change refetches once and repaints both surfaces.

**Summary strip** computed from the same context: Gross Profit, Total
Costs, Net Profit, Margin, ROI, Successful/Failed trades, Win Rate, Avg
Profit/Trade, Lifetime. Table groups: Identity | Revenue | Costs |
Economic Result | Execution | Performance | Data Quality. TOTAL row is
computed from the dataset, reconciling gross/costs/net/trades.

### 35.3 Universal table engine

One shared component: asc/desc sort on functional headings, column
groups, category/subcategory expand-collapse + Expand/Collapse All,
filter, search, pagination 10/25/50/100, column visibility, sticky
headers, horizontal scroll, freshness/stale badges, tooltips, drill-down.
Large datasets → server-side sort/filter/paginate.

### 35.4 Time-range persistence

Per-page last-used range (e.g. localStorage keyed by page); pages never
share time state unless the user changes them together explicitly.

### 35.5 Other pages

- **Strategy:** variant selector (All+12), time selector, comparison
  table (Opportunity|Economic|Execution|Latency|Risk|Adaptation|
  Profitability groups).
- **Deployment:** time-filtered register with full identifiers +
  pipeline stages (Pre-Flight→…→Monitoring) each carrying status/start/
  end/duration/evidence/errors/warnings.
- **Infrastructure:** RPC counts (live/qualified/production/degraded/
  quarantined), provider diversity, per-chain, RPS vs verified capacity
  with 90% safe / 10% reserve, p50/p95/p99 latency, error/timeout/
  rate-limit rates, block lag, visibility classes V0–V6, self-diagnosis
  and self-healing panels.
- **Reports:** 1H–Since-Deployment ranges across Profit/Opportunity/
  Strategy/Execution/Infrastructure/Risk-Security categories.
- **Competitive Analysis:** `/api/competitive` serves the versioned
  `benchmark_register.json` (AGENTS §27.1) plus the live measured
  AllBright baseline (chains, verified pools, qualified RPCs, funnel
  counts, realized P&L). Sortable KPI table — rows = comparison axes
  mapped to the five dimensions, columns = AllBright ★ + registered
  platforms; cells = rating badges with confidence + evidence tooltips.
  `UNVERIFIED`/`NOT DISCLOSED` where evidence is absent; missing register
  file renders `DATA UNAVAILABLE`, never fabricated scores. Public claims
  must never be presented as measured benchmarks.
- **Wallet:** MetaMask detection → account select → address/chain/
  balance → wallet table + TOTAL row; NEVER seed/private key; auto-
  transfer controls (ON/OFF, threshold, eligible amount, status) with
  manual transfer separate; opening the page NEVER initiates a transfer.

### 35.6 Kill switch & status semantics

Kill Switch: RUNNING→STOPPING→HALTED→RECOVERING via the approved halt
mechanism; halts NEW execution only, never implies cancellation of
submitted transactions; monitoring stays live. Status tooltip =
visibility + RPC + execution + wallet + diagnosis + healing rollup.

### 35.7 Data-quality enforcement

Every rendered metric either resolves through the lineage chain or
renders `DATA UNAVAILABLE` / `NOT VERIFIED` with reason. RPC surfaces
distinguish AVAILABLE / HEALTHY / MARKET-VISIBLE / OPPORTUNITY-VISIBLE.
Diagnosis cards carry Issue ID/Timestamp/Severity/Component/Chain/
Root Cause/Confidence/State/Remediation; healing cards carry Incident
ID/Detection/Diagnosis/Remediation/Endpoint Changed/Chain/Recovery Time/
Verification/Final State over `DETECTED→DIAGNOSING→REMEDIATING→
VERIFYING→HEALED/FAILED/ESCALATED`.

### 35.7b Blockchain Stream implementation

Battlefield page (AGENTS §38.15): opportunity cards carry a persistent
`OPP-*` ID and illuminate stages `DETECT→QUALIFY→SIMULATE→DEFEND→
EXECUTE→CONFIRM→PROFIT→WALLET` purely from indexed events
(`Opportunity`, `Rejected`, `ExecRequest`, `ExecResult`, `Failure`,
`Heal`, `RpcScan`). Unproven stages render grey `UNVERIFIED`. The funnel
waterfall, soldier-activity feed (component-attributed lines),
defense strip (green/yellow/red/grey), opportunity chart and capital-
protection summary all read the same index — no parallel data path.
Feed interruption shows `STREAM DEGRADED — RECONNECTING` and reconciles
on reconnect; never fabricate continuity.

### 35.8 Acceptance verification

Run the AGENTS §38.14 checklist end-to-end: all time ranges hit the
backend; all-chain totals reconcile with per-variant sums; all 12
variants selectable and match registry names; each of TABLE/CHART/BAR
renders the same filtered dataset; table sort/expand/paginate/search/
column-visibility all functional; wallet detection + total reconcile;
kill switch halts without false claims.

**Branding (mandatory, AGENTS §38.13):** logo = the word `AllBright`
alone — minimalist blue/white SVG, no crypto iconography, legible at
small size; placed top-left of header and footer-center. Motto
`Autonomous Wealth Engine. 24/7. Like No One.` displayed separately
from the logo. Footer identity: `AllBright DeFi Software Engineering
Ltd. © 2026`.

## 36. FINAL MINDSET

Every change must answer: *does this increase Allbright's ability to detect, verify, simulate, execute and learn from profitable opportunities under real multi-chain conditions?*

The optimization target:

```
MORE QUALIFIED OPPORTUNITIES + FASTER VERIFIED DECISIONS
+ HIGHER EXECUTION RELIABILITY + LOWER UNPRODUCTIVE COST
+ BETTER RISK-ADJUSTED NET PROFIT + CONTINUOUS LEARNING
```

**Detect faster. Simulate accurately. Optimize economically. Execute selectively. Learn continuously.**
